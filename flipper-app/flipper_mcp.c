/**
 * flipper_mcp.c — Flipper Zero companion app for the Flipper MCP WiFi Dev Board.
 *
 * Appears in Apps → Tools → Flipper MCP.
 *
 * Features:
 *   • Status screen — reads /ext/apps_data/flipper_mcp/status.txt written by the
 *     ESP32 every 30 s (ip, ssid, server state, firmware version).
 *   • Start / Stop / Restart — writes a command to server.cmd which the ESP32
 *     picks up within 5 seconds.
 *
 * Build with ufbt:  cd flipper-app && ufbt
 * Install:          copy flipper_mcp.fap to SD:/apps/Tools/
 */

#include <furi.h>
#include <gui/gui.h>
#include <gui/elements.h>
#include <input/input.h>
#include <storage/storage.h>
#include <notification/notification.h>
#include <notification/notification_messages.h>

#include <string.h>
#include <stdio.h>

#define TAG "FlipperMCP"

#define DATA_DIR   EXT_PATH("apps_data/flipper_mcp")
#define STATUS_FILE EXT_PATH("apps_data/flipper_mcp/status.txt")
#define CMD_FILE    EXT_PATH("apps_data/flipper_mcp/server.cmd")

#define STATUS_BUF_LEN 512
#define RESULT_BUF_LEN 128

// ── Views ────────────────────────────────────────────────────────────────────

typedef enum {
    ViewMenu,
    ViewStatus,
    ViewResult,
} AppView;

// ── Menu items ───────────────────────────────────────────────────────────────

typedef enum {
    MenuStatus = 0,
    MenuStart,
    MenuStop,
    MenuRestart,
    MenuCount,
} MenuItem;

static const char* const MENU_LABELS[MenuCount] = {
    "Status",
    "Start Server",
    "Stop Server",
    "Restart Server",
};

static const char* const MENU_CMDS[MenuCount] = {
    NULL,        // Status — handled separately
    "start",
    "stop",
    "restart",
};

static const char* const MENU_CONFIRM[MenuCount] = {
    NULL,
    "Start sent.\nESP32 picks up\nin ~5 seconds.",
    "Stop sent.\nESP32 picks up\nin ~5 seconds.",
    "Restart sent.\nESP32 picks up\nin ~5 seconds.",
};

// ── App state ────────────────────────────────────────────────────────────────

typedef struct {
    Gui*              gui;
    ViewPort*         view_port;
    FuriMessageQueue* event_queue;
    Storage*          storage;
    NotificationApp*  notifications;

    AppView view;
    uint8_t selected;

    char status[STATUS_BUF_LEN];
    char result[RESULT_BUF_LEN];
} FlipperMcpApp;

// ── Draw callbacks ───────────────────────────────────────────────────────────

static void draw_menu(Canvas* canvas, FlipperMcpApp* app) {
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, "Flipper MCP");
    canvas_draw_line(canvas, 0, 13, 128, 13);

    canvas_set_font(canvas, FontSecondary);
    for(uint8_t i = 0; i < MenuCount; i++) {
        uint8_t y = 25 + i * 11;
        if(i == app->selected) {
            canvas_draw_box(canvas, 0, y - 9, 128, 11);
            canvas_set_color(canvas, ColorWhite);
            canvas_draw_str(canvas, 4, y, MENU_LABELS[i]);
            canvas_set_color(canvas, ColorBlack);
        } else {
            canvas_draw_str(canvas, 4, y, MENU_LABELS[i]);
        }
    }
    canvas_draw_str(canvas, 2, 63, "[OK] Select  [Back] Exit");
}

static void draw_status(Canvas* canvas, FlipperMcpApp* app) {
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, "Status");
    canvas_draw_line(canvas, 0, 13, 128, 13);

    canvas_set_font(canvas, FontSecondary);

    // Parse key=value lines and display them in a friendlier format
    char buf[STATUS_BUF_LEN];
    strncpy(buf, app->status, STATUS_BUF_LEN - 1);
    buf[STATUS_BUF_LEN - 1] = '\0';

    uint8_t y = 25;
    char* line = buf;
    char* nl;
    while((nl = strchr(line, '\n')) != NULL && y <= 56) {
        *nl = '\0';
        // Render each key=value pair
        char* eq = strchr(line, '=');
        if(eq) {
            *eq = '\0';
            char pretty[64];
            snprintf(pretty, sizeof(pretty), "%s: %s", line, eq + 1);
            canvas_draw_str(canvas, 2, y, pretty);
            y += 10;
        }
        line = nl + 1;
    }

    if(y == 25) {
        // Nothing rendered — status file missing or empty
        elements_multiline_text_aligned(canvas, 64, 40, AlignCenter, AlignCenter,
            "No status yet.\nWait 30s after boot.");
    }

    canvas_draw_str(canvas, 2, 63, "[Back] Menu");
}

static void draw_result(Canvas* canvas, FlipperMcpApp* app) {
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, "Flipper MCP");
    canvas_draw_line(canvas, 0, 13, 128, 13);

    elements_multiline_text_aligned(canvas, 64, 38, AlignCenter, AlignCenter, app->result);

    canvas_draw_str(canvas, 2, 63, "[Back] Menu");
}

static void app_draw_callback(Canvas* canvas, void* context) {
    FlipperMcpApp* app = context;
    canvas_clear(canvas);
    canvas_set_color(canvas, ColorBlack);
    switch(app->view) {
    case ViewMenu:   draw_menu(canvas, app);   break;
    case ViewStatus: draw_status(canvas, app); break;
    case ViewResult: draw_result(canvas, app); break;
    }
}

// ── Actions ──────────────────────────────────────────────────────────────────

static void action_read_status(FlipperMcpApp* app) {
    File* f = storage_file_alloc(app->storage);
    bool ok = storage_file_open(f, STATUS_FILE, FSAM_READ, FSOM_OPEN_EXISTING);
    if(ok) {
        uint16_t n = storage_file_read(f, app->status, STATUS_BUF_LEN - 1);
        app->status[n] = '\0';
        storage_file_close(f);
    } else {
        strncpy(app->status,
            "No status file found.\nIs the ESP32 powered\nand running firmware?",
            STATUS_BUF_LEN - 1);
    }
    storage_file_free(f);
}

static bool action_write_command(FlipperMcpApp* app, const char* cmd) {
    storage_simply_mkdir(app->storage, DATA_DIR);
    File* f = storage_file_alloc(app->storage);
    bool ok = storage_file_open(f, CMD_FILE, FSAM_WRITE, FSOM_CREATE_ALWAYS);
    if(ok) {
        storage_file_write(f, cmd, strlen(cmd));
        storage_file_close(f);
    }
    storage_file_free(f);
    return ok;
}

// ── Input callback ───────────────────────────────────────────────────────────

static void app_input_callback(InputEvent* event, void* context) {
    FlipperMcpApp* app = context;
    furi_message_queue_put(app->event_queue, event, 0);
}

// ── Entry point ──────────────────────────────────────────────────────────────

int32_t flipper_mcp_app(void* p) {
    UNUSED(p);

    FlipperMcpApp* app = malloc(sizeof(FlipperMcpApp));
    furi_check(app);
    memset(app, 0, sizeof(FlipperMcpApp));

    app->event_queue   = furi_message_queue_alloc(8, sizeof(InputEvent));
    app->view          = ViewMenu;
    app->selected      = 0;
    app->storage       = furi_record_open(RECORD_STORAGE);
    app->notifications = furi_record_open(RECORD_NOTIFICATION);
    app->gui           = furi_record_open(RECORD_GUI);

    app->view_port = view_port_alloc();
    view_port_draw_callback_set(app->view_port, app_draw_callback, app);
    view_port_input_callback_set(app->view_port, app_input_callback, app);
    gui_add_view_port(app->gui, app->view_port, GuiLayerFullscreen);

    InputEvent event;
    bool running = true;

    while(running) {
        if(furi_message_queue_get(app->event_queue, &event, 100) != FuriStatusOk) {
            continue;
        }

        // Only act on short presses (ignore long/repeat for the menu)
        if(event.type != InputTypeShort && event.type != InputTypeRepeat) {
            continue;
        }

        switch(app->view) {

        case ViewMenu:
            switch(event.key) {
            case InputKeyUp:
                if(app->selected > 0) app->selected--;
                break;
            case InputKeyDown:
                if(app->selected < MenuCount - 1) app->selected++;
                break;
            case InputKeyOk:
                if(app->selected == MenuStatus) {
                    action_read_status(app);
                    app->view = ViewStatus;
                } else {
                    const char* cmd   = MENU_CMDS[app->selected];
                    const char* label = MENU_CONFIRM[app->selected];
                    if(cmd && label) {
                        bool ok = action_write_command(app, cmd);
                        if(ok) {
                            strncpy(app->result, label, RESULT_BUF_LEN - 1);
                            notification_message(app->notifications, &sequence_success);
                        } else {
                            strncpy(app->result,
                                "Write failed.\nIs SD card inserted?",
                                RESULT_BUF_LEN - 1);
                            notification_message(app->notifications, &sequence_error);
                        }
                        app->view = ViewResult;
                    }
                }
                break;
            case InputKeyBack:
                running = false;
                break;
            default:
                break;
            }
            break;

        case ViewStatus:
        case ViewResult:
            if(event.key == InputKeyBack) {
                app->view = ViewMenu;
            }
            break;
        }

        view_port_update(app->view_port);
    }

    // Cleanup
    gui_remove_view_port(app->gui, app->view_port);
    view_port_free(app->view_port);
    furi_message_queue_free(app->event_queue);
    furi_record_close(RECORD_GUI);
    furi_record_close(RECORD_STORAGE);
    furi_record_close(RECORD_NOTIFICATION);
    free(app);

    return 0;
}
