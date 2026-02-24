/**
 * flipper_mcp.c — Flipper Zero companion app for the Flipper MCP WiFi Dev Board.
 *
 * Communicates with the ESP32 over UART using a simple line-based protocol
 * (not Flipper CLI). The app takes control of the UART expansion header by
 * calling expansion_disable() and acquiring the serial handle directly
 * (same pattern as WiFi Marauder).
 *
 * Protocol (ESP32 <-> FAP, 115200 baud, \n-terminated lines, | delimited):
 *   ESP32 -> FAP: STATUS|key=val|..., LOG|msg, TOOLS|name,name,..., ACK|cmd=X|result=ok, PONG
 *                 CLI|<command>          (relay: execute via Flipper SDK)
 *                 WRITE_FILE|path|content (relay: write to SD card)
 *   FAP -> ESP32: CMD|start, CMD|stop, CONFIG|ssid=X|password=Y|..., PING
 *                 CLI_OK|result          (relay response: success)
 *                 CLI_ERR|error          (relay response: failure)
 *
 * Screens:
 *   Status         — shows latest STATUS fields from ESP32
 *   Start/Stop/Restart — sends CMD|X, waits for ACK
 *   Reboot Board   — sends CMD|reboot, waits for ACK
 *   Configure WiFi — 3-step keyboard: SSID -> Password -> Relay URL;
 *                    sends CONFIG message over UART + saves config.txt as backup
 *   View Logs      — scrollable LOG lines received from ESP32
 *   Tools List     — scrollable TOOLS list from ESP32
 *   Refresh Modules — sends CMD|refresh_modules, waits for ACK
 *
 * Build:  cd flipper-app && ufbt
 * Deploy: ufbt launch   (USB) or copy dist/flipper_mcp.fap -> SD:/apps/Tools/
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
#include <expansion/expansion.h>
#include <furi_hal_serial.h>
#include <furi_hal_serial_control.h>
#include <furi_hal_version.h>
#include <furi_hal_power.h>
#include <furi_hal_gpio.h>
#include <furi_hal_resources.h>
#include <furi_hal_rtc.h>
#include <bt/bt_service/bt.h>
#include <toolbox/version.h>

#include <string.h>
#include <stdio.h>
#include <stdlib.h>

#define TAG "FlipperMCP"

#define DATA_DIR    EXT_PATH("apps_data/flipper_mcp")
#define CONFIG_FILE EXT_PATH("apps_data/flipper_mcp/config.txt")

#define TEXT_BUF_LEN   1536  /* shared for status / log / tools display */
#define RESULT_BUF_LEN 128
#define SSID_MAX_LEN   33    /* 32 chars + NUL */
#define PASS_MAX_LEN   65    /* 64 chars + NUL */
#define RELAY_MAX_LEN  129   /* 128 chars + NUL */
#define ACK_BUF_LEN    128
#define RX_STREAM_SIZE 2048
#define LINE_BUF_SIZE  512

#define UART_BAUD_RATE 115200

// -- View IDs -----------------------------------------------------------------

typedef enum {
    ViewIdMenu = 0,
    ViewIdResult,
    ViewIdTextInput,
    ViewIdScrollText,  /* reused for Status, Logs, and Tools List */
} ViewId;

// -- Menu item indices --------------------------------------------------------

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
    MenuLoadSdConfig,
} MenuItem;

typedef enum {
    ConfigStateNone,
    ConfigStateSsid,
    ConfigStatePass,
    ConfigStateRelay,
} ConfigState;

// -- App state ----------------------------------------------------------------

typedef struct {
    Gui*             gui;
    ViewDispatcher*  view_dispatcher;
    Storage*         storage;
    NotificationApp* notifications;

    Submenu*   menu;
    TextInput* text_input;
    View*      result_view;
    View*      scroll_view;

    char result[RESULT_BUF_LEN];
    char text_buf[TEXT_BUF_LEN];  /* current content for scroll_view */
    char scroll_title[32];

    char ssid_buf[SSID_MAX_LEN];
    char pass_buf[PASS_MAX_LEN];
    char relay_buf[RELAY_MAX_LEN];
    ConfigState config_state;

    uint8_t scroll_offset;
    ViewId  current_view;

    /* UART communication */
    Expansion*         expansion;
    FuriHalSerialHandle* serial_handle;
    FuriThread*        uart_worker;
    FuriStreamBuffer*  rx_stream;  /* ISR -> worker thread */
    volatile bool      worker_running;

    /* Parsed data from ESP32 (updated by worker thread) */
    char  status_buf[TEXT_BUF_LEN];   /* latest parsed STATUS fields */
    char  log_buf[TEXT_BUF_LEN];      /* accumulated LOG lines */
    char  tools_buf[TEXT_BUF_LEN];    /* latest TOOLS list */
    char  ack_buf[ACK_BUF_LEN];      /* latest ACK */
    volatile bool ack_received;
    volatile uint32_t rx_bytes;       /* debug: total bytes received from UART */
    volatile uint32_t rx_lines;       /* debug: total lines parsed */
    char  last_raw[128];              /* debug: last raw line received */
    FuriMutex* data_mutex;            /* protects status/log/tools/ack buffers */
} FlipperMcpApp;

// -- UART helpers -------------------------------------------------------------

/** Send a \n-terminated line to the ESP32 over UART. */
static void uart_send(FlipperMcpApp* app, const char* line) {
    size_t len = strlen(line);
    furi_hal_serial_tx(app->serial_handle, (const uint8_t*)line, len);
    uint8_t nl = '\n';
    furi_hal_serial_tx(app->serial_handle, &nl, 1);
}

/** ISR callback -- push received byte into the stream buffer. */
static void uart_rx_cb(
    FuriHalSerialHandle* handle,
    FuriHalSerialRxEvent event,
    void* context) {
    FlipperMcpApp* app = context;
    if(event == FuriHalSerialRxEventData) {
        uint8_t byte = furi_hal_serial_async_rx(handle);
        furi_stream_buffer_send(app->rx_stream, &byte, 1, 0);
    }
}

// -- CLI relay: escape/unescape helpers ---------------------------------------

/** Escape newlines in src as literal "\\n" for UART transport. */
static void escape_newlines(const char* src, char* dst, size_t dst_size) {
    size_t di = 0;
    for(size_t si = 0; src[si] && di + 3 < dst_size; si++) {
        if(src[si] == '\n') {
            dst[di++] = '\\';
            dst[di++] = 'n';
        } else {
            dst[di++] = src[si];
        }
    }
    dst[di] = '\0';
}

// -- CLI relay: GPIO pin lookup table -----------------------------------------

typedef struct {
    const char* name;
    const GpioPin* pin;
} GpioPinEntry;

static const GpioPinEntry mcp_gpio_pins[] = {
    {"PA7", &gpio_ext_pa7},
    {"PA6", &gpio_ext_pa6},
    {"PA4", &gpio_ext_pa4},
    {"PB3", &gpio_ext_pb3},
    {"PB2", &gpio_ext_pb2},
    {"PC3", &gpio_ext_pc3},
    {"PC1", &gpio_ext_pc1},
    {"PC0", &gpio_ext_pc0},
};
#define MCP_GPIO_PIN_COUNT (sizeof(mcp_gpio_pins) / sizeof(mcp_gpio_pins[0]))

static const GpioPin* gpio_lookup(const char* name) {
    for(size_t i = 0; i < MCP_GPIO_PIN_COUNT; i++) {
        if(strcasecmp(mcp_gpio_pins[i].name, name) == 0) {
            return mcp_gpio_pins[i].pin;
        }
    }
    return NULL;
}

// -- CLI relay: command handlers -----------------------------------------------

static bool cmd_device_info(char* result, size_t result_size) {
    const Version* fw_ver = furi_hal_version_get_firmware_version();
    const char* branch = fw_ver ? version_get_gitbranch(fw_ver) : NULL;
    const char* build_date = fw_ver ? version_get_builddate(fw_ver) : NULL;
    const char* fw_version = fw_ver ? version_get_version(fw_ver) : NULL;
    snprintf(
        result,
        result_size,
        "name: %s\nhw_version: %d\nhw_target: %d\nfw_version: %s\nfw_branch: %s\nfw_build_date: %s",
        furi_hal_version_get_name_ptr() ? furi_hal_version_get_name_ptr() : "unknown",
        furi_hal_version_get_hw_version(),
        furi_hal_version_get_hw_target(),
        fw_version ? fw_version : "unknown",
        branch ? branch : "unknown",
        build_date ? build_date : "unknown");
    return true;
}

static bool cmd_power_info(char* result, size_t result_size) {
    snprintf(
        result,
        result_size,
        "battery_voltage: %.2fV\nbattery_current: %.1fmA\nbattery_temp: %.1fC\ncharging: %s\ncharge_pct: %d%%\nusb_connected: %s",
        (double)furi_hal_power_get_battery_voltage(FuriHalPowerICFuelGauge),
        (double)furi_hal_power_get_battery_current(FuriHalPowerICFuelGauge),
        (double)furi_hal_power_get_battery_temperature(FuriHalPowerICFuelGauge),
        furi_hal_power_is_charging() ? "yes" : "no",
        furi_hal_power_get_pct(),
        furi_hal_power_is_otg_enabled() ? "yes" : "no");
    return true;
}

static bool cmd_free(char* result, size_t result_size) {
    snprintf(
        result,
        result_size,
        "free_heap: %zu\ntotal_heap: %zu",
        memmgr_get_free_heap(),
        memmgr_get_total_heap());
    return true;
}

static bool cmd_uptime(char* result, size_t result_size) {
    uint32_t ticks = furi_get_tick();
    uint32_t secs = ticks / 1000;
    uint32_t mins = secs / 60;
    uint32_t hours = mins / 60;
    snprintf(
        result,
        result_size,
        "uptime: %luh %lum %lus (%lu ticks)",
        (unsigned long)hours,
        (unsigned long)(mins % 60),
        (unsigned long)(secs % 60),
        (unsigned long)ticks);
    return true;
}

static bool cmd_gpio(const char* subcmd, char* result, size_t result_size) {
    /* Parse: "set PA7 1", "read PA7", "mode PA7 1" */
    char action[16] = {0};
    char pin_name[8] = {0};
    int value = 0;

    int parsed = sscanf(subcmd, "%15s %7s %d", action, pin_name, &value);
    if(parsed < 2) {
        snprintf(result, result_size, "Usage: gpio <set|read|mode> <pin> [value]");
        return false;
    }

    const GpioPin* pin = gpio_lookup(pin_name);
    if(!pin) {
        snprintf(
            result,
            result_size,
            "Unknown pin: %s\nValid: PA7,PA6,PA4,PB3,PB2,PC3,PC1,PC0",
            pin_name);
        return false;
    }

    if(strcmp(action, "set") == 0) {
        if(parsed < 3) {
            snprintf(result, result_size, "Usage: gpio set <pin> <0|1>");
            return false;
        }
        furi_hal_gpio_init(pin, GpioModeOutputPushPull, GpioPullNo, GpioSpeedLow);
        furi_hal_gpio_write(pin, value != 0);
        snprintf(result, result_size, "%s = %d", pin_name, value != 0);
        return true;
    } else if(strcmp(action, "read") == 0) {
        furi_hal_gpio_init(pin, GpioModeInput, GpioPullNo, GpioSpeedLow);
        bool state = furi_hal_gpio_read(pin);
        snprintf(result, result_size, "%s = %d", pin_name, state ? 1 : 0);
        return true;
    } else if(strcmp(action, "mode") == 0) {
        if(parsed < 3) {
            snprintf(result, result_size, "Usage: gpio mode <pin> <0=in|1=out>");
            return false;
        }
        if(value == 0) {
            furi_hal_gpio_init(pin, GpioModeInput, GpioPullNo, GpioSpeedLow);
        } else {
            furi_hal_gpio_init(pin, GpioModeOutputPushPull, GpioPullNo, GpioSpeedLow);
        }
        snprintf(result, result_size, "%s mode = %s", pin_name, value ? "output" : "input");
        return true;
    } else {
        snprintf(result, result_size, "Unknown gpio action: %s (use set/read/mode)", action);
        return false;
    }
}

static bool
    cmd_storage(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    char action[16] = {0};
    char path[256] = {0};

    int parsed = sscanf(subcmd, "%15s %255s", action, path);
    if(parsed < 2) {
        snprintf(result, result_size, "Usage: storage <read|list|stat|mkdir> <path>");
        return false;
    }

    if(strcmp(action, "read") == 0) {
        File* f = storage_file_alloc(app->storage);
        if(!storage_file_open(f, path, FSAM_READ, FSOM_OPEN_EXISTING)) {
            snprintf(result, result_size, "Cannot open: %s", path);
            storage_file_free(f);
            return false;
        }
        size_t n = storage_file_read(f, result, result_size - 1);
        result[n] = '\0';
        storage_file_close(f);
        storage_file_free(f);
        return true;
    } else if(strcmp(action, "list") == 0) {
        File* dir = storage_file_alloc(app->storage);
        if(!storage_dir_open(dir, path)) {
            snprintf(result, result_size, "Cannot open dir: %s", path);
            storage_file_free(dir);
            return false;
        }
        FileInfo info;
        char name[128];
        size_t pos = 0;
        while(storage_dir_read(dir, &info, name, sizeof(name)) && pos + 60 < result_size) {
            bool is_dir = (info.flags & FSF_DIRECTORY);
            int written = snprintf(
                result + pos,
                result_size - pos,
                "%s%s %lu\n",
                is_dir ? "[D] " : "",
                name,
                (unsigned long)info.size);
            if(written > 0) pos += (size_t)written;
        }
        if(pos == 0) snprintf(result, result_size, "(empty directory)");
        storage_dir_close(dir);
        storage_file_free(dir);
        return true;
    } else if(strcmp(action, "stat") == 0) {
        FileInfo info;
        if(storage_common_stat(app->storage, path, &info) != FSE_OK) {
            snprintf(result, result_size, "Not found: %s", path);
            return false;
        }
        snprintf(
            result,
            result_size,
            "path: %s\nsize: %lu\ntype: %s",
            path,
            (unsigned long)info.size,
            (info.flags & FSF_DIRECTORY) ? "directory" : "file");
        return true;
    } else if(strcmp(action, "mkdir") == 0) {
        if(storage_simply_mkdir(app->storage, path)) {
            snprintf(result, result_size, "Created: %s", path);
            return true;
        } else {
            snprintf(result, result_size, "Failed to create: %s", path);
            return false;
        }
    } else if(strcmp(action, "write") == 0) {
        /* "storage write /path content..." -- content is everything after path + space */
        const char* content_start = subcmd + 6; /* skip "write " */
        /* skip the path */
        const char* space = strchr(content_start, ' ');
        if(!space || !space[1]) {
            snprintf(result, result_size, "Usage: storage write <path> <content>");
            return false;
        }
        const char* content = space + 1;
        storage_simply_mkdir(app->storage, DATA_DIR);
        File* f = storage_file_alloc(app->storage);
        if(!storage_file_open(f, path, FSAM_WRITE, FSOM_CREATE_ALWAYS)) {
            snprintf(result, result_size, "Cannot write: %s", path);
            storage_file_free(f);
            return false;
        }
        storage_file_write(f, content, strlen(content));
        storage_file_close(f);
        storage_file_free(f);
        snprintf(result, result_size, "Written %zu bytes to %s", strlen(content), path);
        return true;
    } else {
        snprintf(
            result, result_size, "Unknown storage action: %s (use read/list/stat/mkdir/write)", action);
        return false;
    }
}

static bool cmd_ble(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    UNUSED(app);
    /* BLE commands are dispatched to the Flipper's BT service.
     * BLE scanning requires temporarily disconnecting the mobile app. */

    if(strncmp(subcmd, "scan", 4) == 0) {
        /* Parse duration: "scan --duration 5" or "scan 5" */
        int duration = 5;
        const char* dur_str = strstr(subcmd, "--duration ");
        if(dur_str) {
            duration = atoi(dur_str + 11);
        } else if(subcmd[4] == ' ') {
            duration = atoi(subcmd + 5);
        }
        if(duration < 1) duration = 1;
        if(duration > 30) duration = 30;

        /* Open BT service */
        Bt* bt = furi_record_open(RECORD_BT);
        bt_disconnect(bt);

        /* Wait for disconnect */
        furi_delay_ms(500);

        /* BLE GAP scanning is not directly exposed in the public FAP SDK.
         * For now, report that the BT service was toggled and return a
         * placeholder. Full scan requires STM32WB BLE stack integration. */
        snprintf(
            result,
            result_size,
            "BLE scan (%ds): BT service disconnected.\n"
            "Note: GAP scanning requires STM32WB BLE stack\n"
            "integration (pending full implementation).\n"
            "BT service restored.",
            duration);

        /* Re-enable BT */
        furi_delay_ms(200);
        furi_record_close(RECORD_BT);
        return true;

    } else if(strncmp(subcmd, "connect", 7) == 0) {
        snprintf(result, result_size, "BLE connect: not yet implemented (requires GAP central role)");
        return false;
    } else if(strncmp(subcmd, "disconnect", 10) == 0) {
        snprintf(result, result_size, "BLE disconnect: not yet implemented");
        return false;
    } else if(strncmp(subcmd, "gatt_discover", 13) == 0) {
        snprintf(result, result_size, "BLE GATT discover: not yet implemented");
        return false;
    } else if(strncmp(subcmd, "gatt_read", 9) == 0) {
        snprintf(result, result_size, "BLE GATT read: not yet implemented");
        return false;
    } else if(strncmp(subcmd, "gatt_write", 10) == 0) {
        snprintf(result, result_size, "BLE GATT write: not yet implemented");
        return false;
    } else {
        snprintf(result, result_size, "Unknown BLE command: %.40s", subcmd);
        return false;
    }
}

// -- CLI relay: dispatcher ----------------------------------------------------

/** Handle a CLI command from ESP32. Executes the command and sends CLI_OK or CLI_ERR. */
static void cli_dispatch(FlipperMcpApp* app, const char* command) {
    char result[512];
    result[0] = '\0';
    bool ok = false;

    FURI_LOG_I(TAG, "CLI dispatch: %.80s", command);

    if(strncmp(command, "device_info", 11) == 0) {
        ok = cmd_device_info(result, sizeof(result));
    } else if(strncmp(command, "power info", 10) == 0) {
        ok = cmd_power_info(result, sizeof(result));
    } else if(strncmp(command, "power off", 9) == 0) {
        furi_hal_power_off();
        snprintf(result, sizeof(result), "powering off");
        ok = true;
    } else if(strncmp(command, "power reboot", 12) == 0) {
        snprintf(result, sizeof(result), "rebooting");
        ok = true;
        /* Send response before reboot */
        char escaped[1024];
        escape_newlines(result, escaped, sizeof(escaped));
        char response[1100];
        snprintf(response, sizeof(response), "CLI_OK|%s", escaped);
        uart_send(app, response);
        furi_delay_ms(100);
        furi_hal_power_reset();
        return; /* won't reach here */
    } else if(strncmp(command, "gpio ", 5) == 0) {
        ok = cmd_gpio(command + 5, result, sizeof(result));
    } else if(strncmp(command, "storage ", 8) == 0) {
        ok = cmd_storage(app, command + 8, result, sizeof(result));
    } else if(strncmp(command, "ble ", 4) == 0) {
        ok = cmd_ble(app, command + 4, result, sizeof(result));
    } else if(strcmp(command, "free") == 0) {
        ok = cmd_free(result, sizeof(result));
    } else if(strcmp(command, "uptime") == 0) {
        ok = cmd_uptime(result, sizeof(result));
    } else if(strcmp(command, "ps") == 0) {
        /* Thread enumeration is limited in FAP context — report what we can */
        snprintf(
            result,
            sizeof(result),
            "free_heap: %zu\ntotal_heap: %zu\n(thread list requires OS-level access)",
            memmgr_get_free_heap(),
            memmgr_get_total_heap());
        ok = true;
    } else {
        snprintf(result, sizeof(result), "Unknown command: %.100s", command);
        ok = false;
    }

    /* Escape newlines and send response */
    char escaped[1024];
    escape_newlines(result, escaped, sizeof(escaped));
    char response[1100];
    snprintf(response, sizeof(response), "%s|%s", ok ? "CLI_OK" : "CLI_ERR", escaped);
    uart_send(app, response);
}

/* Forward declaration — defined further down in the file */
static bool write_file_str(FlipperMcpApp* app, const char* path, const char* content);

/** Handle WRITE_FILE|path|content from ESP32 */
static void handle_write_file(FlipperMcpApp* app, const char* payload) {
    /* payload = "path|escaped_content" */
    char payload_copy[4096];
    strncpy(payload_copy, payload, sizeof(payload_copy) - 1);
    payload_copy[sizeof(payload_copy) - 1] = '\0';

    char* pipe = strchr(payload_copy, '|');
    if(!pipe) {
        uart_send(app, "CLI_ERR|Invalid WRITE_FILE format (no pipe)");
        return;
    }
    *pipe = '\0';
    const char* path = payload_copy;
    const char* escaped_content = pipe + 1;

    /* Unescape \\n -> \n */
    char content[4096];
    size_t ci = 0;
    for(size_t i = 0; escaped_content[i] && ci + 1 < sizeof(content); i++) {
        if(escaped_content[i] == '\\' && escaped_content[i + 1] == 'n') {
            content[ci++] = '\n';
            i++; /* skip the 'n' */
        } else {
            content[ci++] = escaped_content[i];
        }
    }
    content[ci] = '\0';

    /* Ensure parent directory exists */
    char dir_path[256];
    strncpy(dir_path, path, sizeof(dir_path) - 1);
    dir_path[sizeof(dir_path) - 1] = '\0';
    char* last_slash = strrchr(dir_path, '/');
    if(last_slash) {
        *last_slash = '\0';
        storage_simply_mkdir(app->storage, dir_path);
    }

    if(write_file_str(app, path, content)) {
        char resp[256];
        snprintf(resp, sizeof(resp), "CLI_OK|written %zu bytes", ci);
        uart_send(app, resp);
    } else {
        uart_send(app, "CLI_ERR|write failed");
    }
}

/** Parse a complete line received from ESP32. Called by the worker thread. */
static void uart_parse_line(FlipperMcpApp* app, const char* line) {
    furi_mutex_acquire(app->data_mutex, FuriWaitForever);

    if(strncmp(line, "STATUS|", 7) == 0) {
        /* Parse pipe-delimited key=value pairs into "key: value\n" for display */
        const char* payload = line + 7;
        app->status_buf[0] = '\0';
        size_t out_pos = 0;
        const char* p = payload;
        while(*p && out_pos + 40 < TEXT_BUF_LEN) {
            const char* pipe = strchr(p, '|');
            size_t seg_len = pipe ? (size_t)(pipe - p) : strlen(p);
            /* Find '=' in this segment */
            const char* eq = memchr(p, '=', seg_len);
            if(eq) {
                size_t key_len = (size_t)(eq - p);
                size_t val_len = seg_len - key_len - 1;
                int written = snprintf(
                    app->status_buf + out_pos,
                    TEXT_BUF_LEN - out_pos - 1,
                    "%.*s: %.*s\n",
                    (int)(key_len < 20 ? key_len : 20), p,
                    (int)(val_len < 90 ? val_len : 90), eq + 1);
                if(written > 0) out_pos += (size_t)written;
            }
            if(!pipe) break;
            p = pipe + 1;
        }
        FURI_LOG_D(TAG, "STATUS parsed (%zu bytes)", out_pos);

    } else if(strncmp(line, "LOG|", 4) == 0) {
        const char* msg = line + 4;
        size_t cur_len = strlen(app->log_buf);
        size_t msg_len = strlen(msg);
        /* If buffer is getting full, remove oldest lines to make room */
        if(cur_len + msg_len + 2 >= TEXT_BUF_LEN) {
            /* Find a cutpoint past the first quarter and discard everything before */
            char* cutpoint = app->log_buf + TEXT_BUF_LEN / 4;
            char* nl_ptr = strchr(cutpoint, '\n');
            if(nl_ptr && nl_ptr[1]) {
                size_t keep_len = strlen(nl_ptr + 1);
                memmove(app->log_buf, nl_ptr + 1, keep_len + 1);
                cur_len = keep_len;
            } else {
                app->log_buf[0] = '\0';
                cur_len = 0;
            }
        }
        snprintf(app->log_buf + cur_len, TEXT_BUF_LEN - cur_len, "%s\n", msg);

    } else if(strncmp(line, "TOOLS|", 6) == 0) {
        /* Comma-separated tool names -> one per line */
        const char* payload = line + 6;
        app->tools_buf[0] = '\0';
        size_t out_pos = 0;
        const char* p = payload;
        while(*p && out_pos + 40 < TEXT_BUF_LEN) {
            const char* comma = strchr(p, ',');
            size_t name_len = comma ? (size_t)(comma - p) : strlen(p);
            int written = snprintf(
                app->tools_buf + out_pos,
                TEXT_BUF_LEN - out_pos - 1,
                "%.*s\n",
                (int)(name_len < 80 ? name_len : 80), p);
            if(written > 0) out_pos += (size_t)written;
            if(!comma) break;
            p = comma + 1;
        }
        FURI_LOG_D(TAG, "TOOLS parsed (%zu bytes)", out_pos);

    } else if(strncmp(line, "ACK|", 4) == 0) {
        strncpy(app->ack_buf, line + 4, ACK_BUF_LEN - 1);
        app->ack_buf[ACK_BUF_LEN - 1] = '\0';
        app->ack_received = true;
        FURI_LOG_D(TAG, "ACK: %s", app->ack_buf);

    } else if(strncmp(line, "PONG", 4) == 0) {
        FURI_LOG_D(TAG, "PONG received");

    } else if(strncmp(line, "CLI|", 4) == 0) {
        /* CLI relay: execute command and send response.
         * Release mutex first — command execution may take time. */
        furi_mutex_release(app->data_mutex);
        cli_dispatch(app, line + 4);
        return; /* mutex already released */

    } else if(strncmp(line, "WRITE_FILE|", 11) == 0) {
        /* File write relay: write to SD card.
         * Release mutex first — file I/O may take time. */
        furi_mutex_release(app->data_mutex);
        handle_write_file(app, line + 11);
        return; /* mutex already released */

    } else {
        FURI_LOG_W(TAG, "Unknown UART line: %.80s", line);
    }

    furi_mutex_release(app->data_mutex);
}

/** Worker thread -- assembles lines from the RX stream and dispatches them. */
static int32_t uart_worker_thread(void* context) {
    FlipperMcpApp* app = context;
    char line_buf[LINE_BUF_SIZE];
    size_t line_pos = 0;

    FURI_LOG_I(TAG, "UART worker started");

    while(app->worker_running) {
        uint8_t byte;
        size_t received = furi_stream_buffer_receive(app->rx_stream, &byte, 1, 100);
        if(received == 0) continue;
        app->rx_bytes++;

        if(byte == '\n') {
            if(line_pos > 0) {
                /* Strip trailing \r if present */
                if(line_pos > 0 && line_buf[line_pos - 1] == '\r') line_pos--;
                line_buf[line_pos] = '\0';
                app->rx_lines++;
                /* Save last raw line for debug display */
                furi_mutex_acquire(app->data_mutex, FuriWaitForever);
                strncpy(app->last_raw, line_buf, sizeof(app->last_raw) - 1);
                app->last_raw[sizeof(app->last_raw) - 1] = '\0';
                furi_mutex_release(app->data_mutex);
                uart_parse_line(app, line_buf);
                line_pos = 0;
            }
        } else if(byte == '\r') {
            /* ignore standalone \r */
        } else {
            if(line_pos < LINE_BUF_SIZE - 1) {
                line_buf[line_pos++] = (char)byte;
            }
        }
    }

    FURI_LOG_I(TAG, "UART worker stopped");
    return 0;
}

// -- File helpers (minimal -- only for config.txt backup) ---------------------

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

// -- Actions ------------------------------------------------------------------

/** Copy latest STATUS data into text_buf for display. */
static void action_show_status(FlipperMcpApp* app) {
    /* Request a fresh status push from ESP32 */
    uart_send(app, "CMD|status");

    strncpy(app->scroll_title, "Status", sizeof(app->scroll_title) - 1);
    app->scroll_offset = 0;

    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    if(app->status_buf[0] != '\0') {
        /* Copy status then append debug counters */
        strncpy(app->text_buf, app->status_buf, TEXT_BUF_LEN / 2);
        app->text_buf[TEXT_BUF_LEN / 2] = '\0';
        size_t pos = strlen(app->text_buf);
        snprintf(
            app->text_buf + pos, TEXT_BUF_LEN - pos - 1,
            "\n-- debug --\nrx_bytes: %lu\nrx_lines: %lu",
            (unsigned long)app->rx_bytes,
            (unsigned long)app->rx_lines);
    } else {
        snprintf(
            app->text_buf, TEXT_BUF_LEN - 1,
            "No status yet.\n\nrx_bytes: %lu\nrx_lines: %lu\nlast: %.60s",
            (unsigned long)app->rx_bytes,
            (unsigned long)app->rx_lines,
            app->last_raw[0] ? app->last_raw : "(none)");
    }
    furi_mutex_release(app->data_mutex);
}

/**
 * Send CMD|X over UART, then poll for ACK for up to 6 s (12 x 500 ms).
 * Fills app->result with a human-readable confirmation or timeout message.
 */
static void action_send_cmd_and_wait_ack(FlipperMcpApp* app, const char* cmd) {
    /* Clear previous ACK */
    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    app->ack_received = false;
    app->ack_buf[0] = '\0';
    furi_mutex_release(app->data_mutex);

    /* Send command */
    char cmd_line[64];
    snprintf(cmd_line, sizeof(cmd_line), "CMD|%.50s", cmd);
    uart_send(app, cmd_line);
    notification_message(app->notifications, &sequence_success);

    /* Poll for ACK */
    bool got_ack = false;
    for(int i = 0; i < 12; i++) {
        furi_delay_ms(500);
        if(app->ack_received) {
            got_ack = true;
            break;
        }
    }

    if(got_ack) {
        furi_mutex_acquire(app->data_mutex, FuriWaitForever);
        /* Parse result from ack_buf (format: "cmd=X|result=ok") */
        char* result_field = strstr(app->ack_buf, "result=");
        if(result_field) {
            result_field += 7; /* skip "result=" */
            if(strncmp(result_field, "ok", 2) == 0) {
                snprintf(
                    app->result, RESULT_BUF_LEN,
                    "%.12s: OK\nConfirmed by ESP32.", cmd);
            } else {
                snprintf(
                    app->result, RESULT_BUF_LEN,
                    "%.12s: Error\n%.90s", cmd, result_field);
                notification_message(app->notifications, &sequence_error);
            }
        } else {
            snprintf(app->result, RESULT_BUF_LEN, "%.12s sent.\nACK received.", cmd);
        }
        furi_mutex_release(app->data_mutex);
    } else {
        snprintf(
            app->result, RESULT_BUF_LEN,
            "%.12s sent.\nNo ACK in 6s.\nCheck Status screen.", cmd);
    }
}

/** Copy latest LOG data into text_buf for display. */
static void action_show_logs(FlipperMcpApp* app) {
    strncpy(app->scroll_title, "Logs", sizeof(app->scroll_title) - 1);
    app->scroll_offset = 0;

    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    if(app->log_buf[0] != '\0') {
        strncpy(app->text_buf, app->log_buf, TEXT_BUF_LEN - 1);
    } else {
        strncpy(app->text_buf, "(no logs yet)", TEXT_BUF_LEN - 1);
    }
    furi_mutex_release(app->data_mutex);
}

/** Copy latest TOOLS data into text_buf for display. */
static void action_show_tools(FlipperMcpApp* app) {
    strncpy(app->scroll_title, "Tools", sizeof(app->scroll_title) - 1);
    app->scroll_offset = 0;

    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    if(app->tools_buf[0] != '\0') {
        strncpy(app->text_buf, app->tools_buf, TEXT_BUF_LEN - 1);
    } else {
        strncpy(
            app->text_buf,
            "(no tools yet)\nUse Refresh Modules\nto request list.",
            TEXT_BUF_LEN - 1);
    }
    furi_mutex_release(app->data_mutex);
}

/**
 * Pre-fill SSID and relay URL from existing config.txt on SD (best-effort).
 * Password is intentionally left blank for security.
 */
static void action_prefill_config(FlipperMcpApp* app) {
    char file_buf[512];
    read_file_to_buf(app, CONFIG_FILE, file_buf, sizeof(file_buf));
    app->ssid_buf[0]  = '\0';
    app->relay_buf[0] = '\0';
    char* p = file_buf;
    while(*p) {
        char* nl_ptr = strchr(p, '\n');
        if(nl_ptr) *nl_ptr = '\0';
        if(strncmp(p, "wifi_ssid=", 10) == 0) {
            strncpy(app->ssid_buf, p + 10, SSID_MAX_LEN - 1);
        } else if(strncmp(p, "relay_url=", 10) == 0) {
            strncpy(app->relay_buf, p + 10, RELAY_MAX_LEN - 1);
        }
        if(!nl_ptr) break;
        p = nl_ptr + 1;
    }
}

/**
 * Send CONFIG message to ESP32 over UART and save config.txt as SD backup.
 */
static void action_save_config(FlipperMcpApp* app) {
    /* Send CONFIG over UART -- ESP32 saves to NVS */
    char config_line[320];
    snprintf(
        config_line, sizeof(config_line),
        "CONFIG|ssid=%s|password=%s|relay=%s",
        app->ssid_buf,
        app->pass_buf,
        app->relay_buf);
    uart_send(app, config_line);

    /* Also write config.txt to SD as a human-readable backup */
    char file_content[320];
    snprintf(
        file_content, sizeof(file_content),
        "wifi_ssid=%s\nwifi_password=%s\nrelay_url=%s\n",
        app->ssid_buf,
        app->pass_buf,
        app->relay_buf);
    write_file_str(app, CONFIG_FILE, file_content);

    /* Wait briefly for ACK */
    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    app->ack_received = false;
    furi_mutex_release(app->data_mutex);

    bool got_ack = false;
    for(int i = 0; i < 6; i++) {
        furi_delay_ms(500);
        if(app->ack_received) {
            got_ack = true;
            break;
        }
    }

    if(got_ack) {
        strncpy(
            app->result,
            "Config saved to\nESP32 + SD card!\nSelect Reboot Board\nto apply.",
            RESULT_BUF_LEN - 1);
        notification_message(app->notifications, &sequence_success);
    } else {
        strncpy(
            app->result,
            "Config saved to SD.\nNo ACK from ESP32.\nIs the board powered?",
            RESULT_BUF_LEN - 1);
    }
}

/**
 * Read config.txt from SD and send it as CONFIG message to ESP32 over UART.
 */
static void action_load_sd_config(FlipperMcpApp* app) {
    char file_buf[512];
    uint16_t n = read_file_to_buf(app, CONFIG_FILE, file_buf, sizeof(file_buf));
    if(n == 0) {
        strncpy(
            app->result,
            "No config.txt found\non SD card.\nUse Configure WiFi\nor create manually.",
            RESULT_BUF_LEN - 1);
        return;
    }

    /* Parse config.txt key=value lines into CONFIG pipe-delimited format */
    char ssid[SSID_MAX_LEN] = {0};
    char pass[PASS_MAX_LEN] = {0};
    char device[64] = {0};
    char relay[RELAY_MAX_LEN] = {0};

    char* p = file_buf;
    while(*p) {
        char* nl_ptr = strchr(p, '\n');
        if(nl_ptr) *nl_ptr = '\0';
        /* Strip trailing \r */
        size_t line_len = strlen(p);
        if(line_len > 0 && p[line_len - 1] == '\r') p[line_len - 1] = '\0';

        if(strncmp(p, "wifi_ssid=", 10) == 0)
            strncpy(ssid, p + 10, sizeof(ssid) - 1);
        else if(strncmp(p, "wifi_password=", 14) == 0)
            strncpy(pass, p + 14, sizeof(pass) - 1);
        else if(strncmp(p, "device_name=", 12) == 0)
            strncpy(device, p + 12, sizeof(device) - 1);
        else if(strncmp(p, "relay_url=", 10) == 0)
            strncpy(relay, p + 10, sizeof(relay) - 1);

        if(!nl_ptr) break;
        p = nl_ptr + 1;
    }

    if(ssid[0] == '\0') {
        strncpy(
            app->result,
            "config.txt has no\nwifi_ssid= entry.",
            RESULT_BUF_LEN - 1);
        return;
    }

    /* Build and send CONFIG message */
    char config_line[384];
    snprintf(
        config_line, sizeof(config_line),
        "CONFIG|ssid=%s|password=%s|device=%s|relay=%s",
        ssid, pass, device[0] ? device : "flipper-mcp", relay);
    uart_send(app, config_line);

    /* Wait for ACK */
    furi_mutex_acquire(app->data_mutex, FuriWaitForever);
    app->ack_received = false;
    furi_mutex_release(app->data_mutex);

    bool got_ack = false;
    for(int i = 0; i < 6; i++) {
        furi_delay_ms(500);
        if(app->ack_received) {
            got_ack = true;
            break;
        }
    }

    if(got_ack) {
        snprintf(
            app->result, RESULT_BUF_LEN - 1,
            "Config sent to ESP32!\nSSID: %.20s\nReboot Board to apply.",
            ssid);
        notification_message(app->notifications, &sequence_success);
    } else {
        snprintf(
            app->result, RESULT_BUF_LEN - 1,
            "Config sent (no ACK).\nSSID: %.20s",
            ssid);
    }
}

// -- Draw callbacks -----------------------------------------------------------

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
    return event->key != InputKeyBack;
}

/** Shared draw callback for status, logs, and tools -- scrollable line list. */
static void draw_scroll(Canvas* canvas, void* model) {
    FlipperMcpApp* app = *(FlipperMcpApp**)model;
    canvas_clear(canvas);
    canvas_set_color(canvas, ColorBlack);
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 10, app->scroll_title);
    canvas_draw_line(canvas, 0, 13, 128, 13);
    canvas_set_font(canvas, FontSecondary);

    const char* line_start[48];
    uint8_t     line_len[48];
    uint8_t     lc = 0;
    const char* p = app->text_buf;
    while(*p && lc < 48) {
        const char* nl_ptr = strchr(p, '\n');
        line_start[lc] = p;
        if(nl_ptr) {
            size_t span = (size_t)(nl_ptr - p);
            line_len[lc] = (uint8_t)(span < 255 ? span : 255);
            lc++;
            p = nl_ptr + 1;
        } else {
            size_t span = strlen(p);
            line_len[lc] = (uint8_t)(span < 255 ? span : 255);
            lc++;
            break;
        }
    }

    if(lc == 0) {
        elements_multiline_text_aligned(canvas, 64, 38, AlignCenter, AlignCenter, "(empty)");
    } else {
        uint8_t y = 24;
        for(uint8_t i = app->scroll_offset; i < lc && y <= 56; i++, y += 10) {
            char trimmed[28];
            uint8_t len = line_len[i] < 27 ? line_len[i] : 27;
            memcpy(trimmed, line_start[i], len);
            trimmed[len] = '\0';
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

// -- TextInput callbacks ------------------------------------------------------

static void text_input_done_cb(void* context) {
    FlipperMcpApp* app = context;
    if(app->config_state == ConfigStateSsid) {
        app->config_state = ConfigStatePass;
        app->pass_buf[0] = '\0';
        text_input_reset(app->text_input);
        text_input_set_header_text(app->text_input, "Password (^key=caps)");
        text_input_set_result_callback(
            app->text_input, text_input_done_cb, app, app->pass_buf, PASS_MAX_LEN, false);
    } else if(app->config_state == ConfigStatePass) {
        app->config_state = ConfigStateRelay;
        text_input_reset(app->text_input);
        text_input_set_header_text(app->text_input, "Relay URL (opt.)");
        text_input_set_result_callback(
            app->text_input, text_input_done_cb, app, app->relay_buf, RELAY_MAX_LEN, true);
    } else if(app->config_state == ConfigStateRelay) {
        app->config_state = ConfigStateNone;
        action_save_config(app);
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
    }
}

// -- Menu callback ------------------------------------------------------------

static void menu_cb(void* context, uint32_t index) {
    FlipperMcpApp* app = context;

    switch((MenuItem)index) {

    case MenuStatus:
        action_show_status(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuStart:
        action_send_cmd_and_wait_ack(app, "start");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuStop:
        action_send_cmd_and_wait_ack(app, "stop");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuRestart:
        action_send_cmd_and_wait_ack(app, "restart");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuReboot:
        action_send_cmd_and_wait_ack(app, "reboot");
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
        action_show_logs(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuTools:
        action_show_tools(app);
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;

    case MenuRefresh:
        action_send_cmd_and_wait_ack(app, "refresh_modules");
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    case MenuLoadSdConfig:
        action_load_sd_config(app);
        app->current_view = ViewIdResult;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdResult);
        break;

    default:
        break;
    }
}

// -- Navigation (Back) callback -----------------------------------------------

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

// -- Custom view allocator ----------------------------------------------------

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

// -- UART init / cleanup ------------------------------------------------------

static void uart_init(FlipperMcpApp* app) {
    /* Disable the expansion module protocol so we can use UART directly */
    app->expansion = furi_record_open(RECORD_EXPANSION);
    expansion_disable(app->expansion);

    /* Allocate stream buffer for ISR -> worker communication */
    app->rx_stream = furi_stream_buffer_alloc(RX_STREAM_SIZE, 1);

    /* Acquire UART and configure */
    app->serial_handle = furi_hal_serial_control_acquire(FuriHalSerialIdUsart);
    furi_check(app->serial_handle);
    furi_hal_serial_init(app->serial_handle, UART_BAUD_RATE);

    /* Start async RX with ISR callback */
    furi_hal_serial_async_rx_start(app->serial_handle, uart_rx_cb, app, false);

    /* Start worker thread */
    app->worker_running = true;
    app->data_mutex = furi_mutex_alloc(FuriMutexTypeNormal);
    app->uart_worker = furi_thread_alloc_ex("McpUartWorker", 2048, uart_worker_thread, app);
    furi_thread_start(app->uart_worker);

    FURI_LOG_I(TAG, "UART initialized at %d baud", UART_BAUD_RATE);

    /* Send initial PING to let ESP32 know we're alive */
    uart_send(app, "PING");
}

static void uart_cleanup(FlipperMcpApp* app) {
    /* Stop worker thread */
    app->worker_running = false;
    furi_thread_join(app->uart_worker);
    furi_thread_free(app->uart_worker);

    furi_mutex_free(app->data_mutex);

    /* Stop async RX and release serial */
    furi_hal_serial_async_rx_stop(app->serial_handle);
    furi_hal_serial_deinit(app->serial_handle);
    furi_hal_serial_control_release(app->serial_handle);

    furi_stream_buffer_free(app->rx_stream);

    /* Re-enable expansion module protocol */
    expansion_enable(app->expansion);
    furi_record_close(RECORD_EXPANSION);

    FURI_LOG_I(TAG, "UART cleaned up");
}

// -- Entry point --------------------------------------------------------------

int32_t flipper_mcp_app(void* p) {
    UNUSED(p);

    FlipperMcpApp* app = malloc(sizeof(FlipperMcpApp));
    furi_check(app);
    memset(app, 0, sizeof(FlipperMcpApp));
    app->current_view = ViewIdMenu;

    app->gui           = furi_record_open(RECORD_GUI);
    app->storage       = furi_record_open(RECORD_STORAGE);
    app->notifications = furi_record_open(RECORD_NOTIFICATION);

    /* Initialize UART before GUI -- ESP32 starts pushing data immediately */
    uart_init(app);

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
    submenu_add_item(app->menu, "Refresh Modules", MenuRefresh,      menu_cb, app);
    submenu_add_item(app->menu, "Load SD Config",  MenuLoadSdConfig, menu_cb, app);
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

    uart_cleanup(app);

    furi_record_close(RECORD_GUI);
    furi_record_close(RECORD_STORAGE);
    furi_record_close(RECORD_NOTIFICATION);
    free(app);

    return 0;
}
