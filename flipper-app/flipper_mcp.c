/**
 * flipper_mcp.c — Flipper Zero companion app for the Flipper MCP WiFi Dev Board.
 *
 * Appears in Apps → Tools → Flipper MCP.
 *
 * Screens:
 *   Status         — reads status.txt on-demand; scrollable display of all ESP32 fields
 *                    (ip, ssid, server, uart_ok, relay, heap_free, …).
 *   Start/Stop/Restart — writes server.cmd; waits for server.ack confirmation (up to 6 s).
 *   Reboot Board   — writes "reboot" to server.cmd; waits for ack before ESP32 restarts.
 *   Configure WiFi — 3-step on-screen keyboard: SSID → Password → Relay URL (optional);
 *                    writes config.txt. This is the first-boot setup wizard.
 *   View Logs      — scrollable log.txt (last 20 lines, includes remote tool call audit).
 *   Tools List     — scrollable tools.txt listing all registered MCP tools.
 *   Refresh Modules — writes "refresh_modules"; waits for ack then shows result.
 *
 * Build:  cd flipper-app && ufbt
 * Deploy: ufbt launch   (USB) or copy dist/flipper_mcp.fap → SD:/apps/Tools/
 */

#include <furi.h>
#include <gui/gui.h>
#include <gui/view.h>
#include <gui/view_dispatcher.h>
#include <gui/modules/submenu.h>
#include <gui/modules/text_input.h>
#include <gui/elements.h>
#include <input/input.h>
#include <storage/storage.h>
#include <notification/notification.h>
#include <notification/notification_messages.h>

#include <string.h>
#include <stdio.h>

#define TAG "FlipperMCP"

#define DATA_DIR    EXT_PATH("apps_data/flipper_mcp")
#define STATUS_FILE EXT_PATH("apps_data/flipper_mcp/status.txt")
#define CMD_FILE    EXT_PATH("apps_data/flipper_mcp/server.cmd")
#define ACK_FILE    EXT_PATH("apps_data/flipper_mcp/server.ack")
#define CONFIG_FILE EXT_PATH("apps_data/flipper_mcp/config.txt")
#define LOG_FILE    EXT_PATH("apps_data/flipper_mcp/log.txt")
#define TOOLS_FILE  EXT_PATH("apps_data/flipper_mcp/tools.txt")

#define TEXT_BUF_LEN   1536  /* shared for status / log / tools display */
#define RESULT_BUF_LEN 128
#define SSID_MAX_LEN   33    /* 32 chars + NUL */
#define PASS_MAX_LEN   65    /* 64 chars + NUL */
#define RELAY_MAX_LEN  129   /* 128 chars + NUL */
#define ACK_BUF_LEN    64

// ── View IDs ──────────────────────────────────────────────────────────────────

typedef enum {
    ViewIdMenu = 0,
    ViewIdResult,
    ViewIdTextInput,
    ViewIdScrollText,  /* reused for Status, Logs, and Tools List */
} ViewId;

// ── Menu item indices ─────────────────────────────────────────────────────────

typedef enum {
    MenuStatus = 0,
    MenuStart,
    MenuStop,
    MenuRestart,
    MenuReboot,
    MenuConfigure,
    MenuLogs,
    MenuTools,
    MenuRefresh,
} MenuItem;

typedef enum {
    ConfigStateNone,
    ConfigStateSsid,
    ConfigStatePass,
    ConfigStateRelay,  /* optional relay URL — third step in the Configure flow */
} ConfigState;

// ── App state ─────────────────────────────────────────────────────────────────

typedef struct {
    Gui*             gui;
    ViewDispatcher*  view_dispatcher;
    Storage*         storage;
    NotificationApp* notifications;

    Submenu*   menu;
    TextInput* text_input;
    View*      result_view;
    View*      scroll_view;  /* reused for status, logs, and tools */

    char result[RESULT_BUF_LEN];
    char text_buf[TEXT_BUF_LEN];  /* current content for scroll_view */
    char scroll_title[32];         /* header shown on scroll_view */

    char ssid_buf[SSID_MAX_LEN];
    char pass_buf[PASS_MAX_LEN];
    char relay_buf[RELAY_MAX_LEN];
    ConfigState config_state;

    uint8_t scroll_offset;   /* first visible line in scroll_view */
    ViewId  current_view;
} FlipperMcpApp;

// ── File helpers ──────────────────────────────────────────────────────────────

static uint16_t read_file_to_buf(
    FlipperMcpApp* app,
    const char* path,
    char* buf,
    uint16_t max_len) {
    File* f = storage_file_alloc(app->storage);
    uint16_t n = 0;
    if(storage_file_open(f, path, FSAM_READ, FSOM_OPEN_EXISTING)) {
        n = storage_file_read(f, buf, max_len - 1);
        buf[n] = '\0';
        storage_file_close(f);
    } else {
        buf[0] = '\0';
    }
    storage_file_free(f);
    return n;
}

static bool write_file_str(FlipperMcpApp* app, const char* path, const char* content) {
    storage_simply_mkdir(app->storage, DATA_DIR);
    File* f = storage_file_alloc(app->storage);
    bool ok = storage_file_open(f, path, FSAM_WRITE, FSOM_CREATE_ALWAYS);
    if(ok) {
        storage_file_write(f, content, strlen(content));
        storage_file_close(f);
    }
    storage_file_free(f);
    return ok;
}

// ── Actions ───────────────────────────────────────────────────────────────────

/**
 * Send "status" command, wait 2 s for ESP32 to write a fresh status.txt, then
 * format each "key=value" line as "key: value" in text_buf for the scroll view.
 * All fields (ip, ssid, server, uart_ok, relay, heap_free, …) are shown.
 */
static void action_request_and_read_status(FlipperMcpApp* app) {
    write_file_str(app, CMD_FILE, "status");
    furi_delay_ms(2000);

    char raw[512];
    uint16_t n = read_file_to_buf(app, STATUS_FILE, raw, sizeof(raw));

    strncpy(app->scroll_title, "Status", sizeof(app->scroll_title) - 1);
    app->scroll_offset = 0;

    if(n == 0) {
        strncpy(
            app->text_buf,
            "No status file.\nIs ESP32 powered\nand running firmware?",
            TEXT_BUF_LEN - 1);
        return;
    }

    /* Format "key=value\n" lines as "key: value\n" */
    app->text_buf[0] = '\0';
    char* p = raw;
    size_t out_pos = 0;
    while(*p && out_pos + 32 < TEXT_BUF_LEN) {
        char* nl = strchr(p, '\n');
        if(nl) *nl = '\0';
        char* eq = strchr(p, '=');
        if(eq) {
            *eq = '\0';
            int written = snprintf(
                app->text_buf + out_pos,
                TEXT_BUF_LEN - out_pos - 1,
                "%.20s: %.90s\n",
                p, eq + 1);
            if(written > 0) out_pos += (size_t)written;
        }
        if(!nl) break;
        p = nl + 1;
    }
    if(out_pos == 0)
        strncpy(app->text_buf, "(empty status file)", TEXT_BUF_LEN - 1);
}

/**
 * Write cmd to server.cmd, then poll server.ack for up to 6 s (12 × 500 ms).
 * Fills app->result with a human-readable confirmation or timeout message.
 */
static void action_write_cmd_and_wait_ack(FlipperMcpApp* app, const char* cmd) {
    /* Remove stale ack from a previous command so we don't read a false positive */
    storage_simply_remove(app->storage, ACK_FILE);

    if(!write_file_str(app, CMD_FILE, cmd)) {
        strncpy(app->result, "Write failed.\nIs SD card\ninserted?", RESULT_BUF_LEN - 1);
        notification_message(app->notifications, &sequence_error);
        return;
    }
    notification_message(app->notifications, &sequence_success);

    /* Poll every 500 ms, up to 12 attempts = 6 s max */
    char ack_buf[ACK_BUF_LEN];
    bool got_ack = false;
    for(int i = 0; i < 12; i++) {
        furi_delay_ms(500);
        uint16_t n = read_file_to_buf(app, ACK_FILE, ack_buf, ACK_BUF_LEN);
        if(n > 0) {
            got_ack = true;
            break;
        }
    }

    if(got_ack) {
        char* result_line = strstr(ack_buf, "result=");
        if(result_line) {
            result_line += 7; /* skip "result=" */
            char* nl = strchr(result_line, '\n');
            if(nl) *nl = '\0';
            if(strncmp(result_line, "ok", 2) == 0) {
                snprintf(
                    app->result, RESULT_BUF_LEN,
                    "%.12s: OK\nConfirmed by ESP32.", cmd);
            } else {
                snprintf(
                    app->result, RESULT_BUF_LEN,
                    "%.12s: Error\n%.90s", cmd, result_line);
                notification_message(app->notifications, &sequence_error);
            }
        } else {
            snprintf(app->result, RESULT_BUF_LEN, "%.12s sent.\nACK received.", cmd);
        }
    } else {
        snprintf(
            app->result, RESULT_BUF_LEN,
            "%.12s sent.\nNo ACK in 6s.\nCheck Status screen.", cmd);
    }
}

/** Load the log file into text_buf for the scroll view. */
static void action_load_logs(FlipperMcpApp* app) {
    strncpy(app->scroll_title, "Logs", sizeof(app->scroll_title) - 1);
    uint16_t n = read_file_to_buf(app, LOG_FILE, app->text_buf, TEXT_BUF_LEN);
    if(n == 0) strncpy(app->text_buf, "(no log file yet)", TEXT_BUF_LEN - 1);
    app->scroll_offset = 0;
}

/** Load the tools list file into text_buf for the scroll view. */
static void action_load_tools(FlipperMcpApp* app) {
    strncpy(app->scroll_title, "Tools", sizeof(app->scroll_title) - 1);
    uint16_t n = read_file_to_buf(app, TOOLS_FILE, app->text_buf, TEXT_BUF_LEN);
    if(n == 0)
        strncpy(
            app->text_buf,
            "(no tools.txt yet)\nUse Refresh Modules\nto generate it.",
            TEXT_BUF_LEN - 1);
    app->scroll_offset = 0;
}

/**
 * Pre-fill SSID and relay URL from existing config.txt (best-effort).
 * Password is intentionally left blank for security.
 */
static void action_prefill_config(FlipperMcpApp* app) {
    char existing[512];
    read_file_to_buf(app, CONFIG_FILE, existing, sizeof(existing));
    app->ssid_buf[0]  = '\0';
    app->relay_buf[0] = '\0';
    char* p = existing;
    while(*p) {
        char* nl = strchr(p, '\n');
        if(nl) *nl = '\0';
        if(strncmp(p, "wifi_ssid=", 10) == 0) {
            strncpy(app->ssid_buf, p + 10, SSID_MAX_LEN - 1);
        } else if(strncmp(p, "relay_url=", 10) == 0) {
            strncpy(app->relay_buf, p + 10, RELAY_MAX_LEN - 1);
        }
        if(!nl) break;
        p = nl + 1;
    }
}

/**
 * Write config.txt with SSID, password, and relay URL.
 * relay_url may be empty — the ESP32 treats an empty value as "relay disabled".
 */
static void action_save_config(FlipperMcpApp* app) {
    char content[300];
    snprintf(
        content,
        sizeof(content),
        "wifi_ssid=%s\nwifi_password=%s\nrelay_url=%s\n",
        app->ssid_buf,
        app->pass_buf,
        app->relay_buf);
    bool ok = write_file_str(app, CONFIG_FILE, content);
    if(ok) {
        strncpy(
            app->result,
            "Config saved!\nSelect Reboot Board\nto apply.",
            RESULT_BUF_LEN - 1);
        notification_message(app->notifications, &sequence_success);
    } else {
        strncpy(
            app->result,
            "Save failed.\nIs SD card inserted?",
            RESULT_BUF_LEN - 1);
        notification_message(app->notifications, &sequence_error);
    }
}

// ── Draw callbacks ────────────────────────────────────────────────────────────

static void draw_result(Canvas* canvas, void* model) {
    FlipperMcpApp* app = *(FlipperMcpApp**)model;
    canvas_clear(canvas);
    canvas_set_color(canvas, ColorBlack);
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, "Flipper MCP");
    canvas_draw_line(canvas, 0, 13, 128, 13);
    elements_multiline_text_aligned(canvas, 64, 38, AlignCenter, AlignCenter, app->result);
    canvas_draw_str(canvas, 2, 63, "[Back] Menu");
}

static bool input_result(InputEvent* event, void* context) {
    UNUSED(context);
    return event->key != InputKeyBack; /* let Back propagate to navigation callback */
}

/** Shared draw callback for status, logs, and tools — scrollable line list. */
static void draw_scroll(Canvas* canvas, void* model) {
    FlipperMcpApp* app = *(FlipperMcpApp**)model;
    canvas_clear(canvas);
    canvas_set_color(canvas, ColorBlack);
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, app->scroll_title);
    canvas_draw_line(canvas, 0, 13, 128, 13);
    canvas_set_font(canvas, FontSecondary);

    char buf[TEXT_BUF_LEN];
    strncpy(buf, app->text_buf, TEXT_BUF_LEN - 1);
    buf[TEXT_BUF_LEN - 1] = '\0';

    /* Collect pointers to each line */
    const char* lines[64];
    uint8_t lc = 0;
    char* p = buf;
    while(*p && lc < 64) {
        lines[lc++] = p;
        char* nl = strchr(p, '\n');
        if(!nl) break;
        *nl = '\0';
        p = nl + 1;
    }

    if(lc == 0) {
        elements_multiline_text_aligned(canvas, 64, 38, AlignCenter, AlignCenter, "(empty)");
    } else {
        uint8_t y = 24;
        for(uint8_t i = app->scroll_offset; i < lc && y <= 56; i++, y += 10) {
            char trimmed[28];
            strncpy(trimmed, lines[i], sizeof(trimmed) - 1);
            trimmed[sizeof(trimmed) - 1] = '\0';
            canvas_draw_str(canvas, 2, y, trimmed);
        }
        if(app->scroll_offset > 0) canvas_draw_str(canvas, 119, 22, "^");
        if((uint8_t)(app->scroll_offset + 4) < lc) canvas_draw_str(canvas, 119, 54, "v");
    }
    canvas_draw_str(canvas, 0, 63, "[Ud]Scroll [Back]Menu");
}

static bool input_scroll(InputEvent* event, void* context) {
    FlipperMcpApp* app = context;
    if(event->type != InputTypeShort && event->type != InputTypeRepeat) return false;
    if(event->key == InputKeyBack) return false;
    if(event->key == InputKeyUp && app->scroll_offset > 0) {
        app->scroll_offset--;
        return true;
    }
    if(event->key == InputKeyDown && app->scroll_offset < 60) {
        app->scroll_offset++;
        return true;
    }
    return false;
}

// ── TextInput callbacks ───────────────────────────────────────────────────────

static void text_input_done_cb(void* context) {
    FlipperMcpApp* app = context;
    if(app->config_state == ConfigStateSsid) {
        /* SSID accepted — move to password */
        app->config_state = ConfigStatePass;
        app->pass_buf[0] = '\0';
        text_input_reset(app->text_input);
        text_input_set_header_text(app->text_input, "WiFi Password");
        text_input_set_result_callback(
            app->text_input, text_input_done_cb, app, app->pass_buf, PASS_MAX_LEN, false);
        /* Stay on ViewIdTextInput — it redraws itself */
    } else if(app->config_state == ConfigStatePass) {
        /* Password accepted — move to relay URL (optional, step 3) */
        app->config_state = ConfigStateRelay;
        text_input_reset(app->text_input);
        text_input_set_header_text(app->text_input, "Relay URL (opt.)");
        text_input_set_result_callback(
            app->text_input, text_input_done_cb, app, app->relay_buf, RELAY_MAX_LEN, true);
        /* Stay on ViewIdTextInput */
    } else if(app->config_state == ConfigStateRelay) {
        /* Relay URL accepted (may be empty) — save everything and show result */
        app->config_state = ConfigStateNone;
        action_save_config(app);
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
    }
}

// ── Menu callback ─────────────────────────────────────────────────────────────

static void menu_cb(void* context, uint32_t index) {
    FlipperMcpApp* app = context;

    switch((MenuItem)index) {

    case MenuStatus:
        action_request_and_read_status(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuStart:
        action_write_cmd_and_wait_ack(app, "start");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuStop:
        action_write_cmd_and_wait_ack(app, "stop");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuRestart:
        action_write_cmd_and_wait_ack(app, "restart");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuReboot:
        action_write_cmd_and_wait_ack(app, "reboot");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuConfigure:
        action_prefill_config(app);
        app->pass_buf[0] = '\0';
        app->config_state = ConfigStateSsid;
        text_input_reset(app->text_input);
        text_input_set_header_text(app->text_input, "WiFi SSID");
        text_input_set_result_callback(
            app->text_input, text_input_done_cb, app, app->ssid_buf, SSID_MAX_LEN, true);
        app->current_view = ViewIdTextInput;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdTextInput);
        break;

    case MenuLogs:
        action_load_logs(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuTools:
        action_load_tools(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuRefresh:
        action_write_cmd_and_wait_ack(app, "refresh_modules");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    default:
        break;
    }
}

// ── Navigation (Back) callback ────────────────────────────────────────────────

static bool navigation_back_cb(void* context) {
    FlipperMcpApp* app = context;
    app->config_state = ConfigStateNone;
    if(app->current_view == ViewIdMenu) {
        view_dispatcher_stop(app->view_dispatcher);
    } else {
        app->current_view = ViewIdMenu;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdMenu);
    }
    return true;
}

// ── Custom view allocator ─────────────────────────────────────────────────────

static View* alloc_custom_view(
    FlipperMcpApp* app,
    ViewDrawCallback draw_cb,
    ViewInputCallback input_cb) {
    View* v = view_alloc();
    view_allocate_model(v, ViewModelTypeLockFree, sizeof(FlipperMcpApp*));
    with_view_model(v, FlipperMcpApp * *model, { *model = app; }, false);
    view_set_draw_callback(v, draw_cb);
    view_set_input_callback(v, input_cb);
    view_set_context(v, app);
    return v;
}

// ── Entry point ───────────────────────────────────────────────────────────────

int32_t flipper_mcp_app(void* p) {
    UNUSED(p);

    FlipperMcpApp* app = malloc(sizeof(FlipperMcpApp));
    furi_check(app);
    memset(app, 0, sizeof(FlipperMcpApp));
    app->current_view = ViewIdMenu;

    app->gui           = furi_record_open(RECORD_GUI);
    app->storage       = furi_record_open(RECORD_STORAGE);
    app->notifications = furi_record_open(RECORD_NOTIFICATION);

    app->view_dispatcher = view_dispatcher_alloc();
    view_dispatcher_set_event_callback_context(app->view_dispatcher, app);
    view_dispatcher_set_navigation_event_callback(
        app->view_dispatcher, navigation_back_cb);
    view_dispatcher_attach_to_gui(
        app->view_dispatcher, app->gui, ViewDispatcherTypeFullscreen);

    /* Menu */
    app->menu = submenu_alloc();
    submenu_set_header(app->menu, "Flipper MCP");
    submenu_add_item(app->menu, "Status",          MenuStatus,    menu_cb, app);
    submenu_add_item(app->menu, "Start Server",    MenuStart,     menu_cb, app);
    submenu_add_item(app->menu, "Stop Server",     MenuStop,      menu_cb, app);
    submenu_add_item(app->menu, "Restart Server",  MenuRestart,   menu_cb, app);
    submenu_add_item(app->menu, "Reboot Board",    MenuReboot,    menu_cb, app);
    submenu_add_item(app->menu, "Configure WiFi",  MenuConfigure, menu_cb, app);
    submenu_add_item(app->menu, "View Logs",       MenuLogs,      menu_cb, app);
    submenu_add_item(app->menu, "Tools List",      MenuTools,     menu_cb, app);
    submenu_add_item(app->menu, "Refresh Modules", MenuRefresh,   menu_cb, app);
    view_dispatcher_add_view(
        app->view_dispatcher, ViewIdMenu, submenu_get_view(app->menu));

    /* Text input (shared for SSID, password, and relay URL entry) */
    app->text_input = text_input_alloc();
    view_dispatcher_add_view(
        app->view_dispatcher, ViewIdTextInput, text_input_get_view(app->text_input));

    /* Custom views */
    app->result_view = alloc_custom_view(app, draw_result, input_result);
    view_dispatcher_add_view(app->view_dispatcher, ViewIdResult, app->result_view);

    app->scroll_view = alloc_custom_view(app, draw_scroll, input_scroll);
    view_dispatcher_add_view(app->view_dispatcher, ViewIdScrollText, app->scroll_view);

    view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdMenu);
    view_dispatcher_run(app->view_dispatcher); /* blocks until view_dispatcher_stop() */

    /* Cleanup */
    view_dispatcher_remove_view(app->view_dispatcher, ViewIdMenu);
    view_dispatcher_remove_view(app->view_dispatcher, ViewIdTextInput);
    view_dispatcher_remove_view(app->view_dispatcher, ViewIdResult);
    view_dispatcher_remove_view(app->view_dispatcher, ViewIdScrollText);

    submenu_free(app->menu);
    text_input_free(app->text_input);
    view_free(app->result_view);
    view_free(app->scroll_view);
    view_dispatcher_free(app->view_dispatcher);

    furi_record_close(RECORD_GUI);
    furi_record_close(RECORD_STORAGE);
    furi_record_close(RECORD_NOTIFICATION);
    free(app);

    return 0;
}
