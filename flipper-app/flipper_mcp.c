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
#include <furi_hal_bt.h>
#include <extra_profiles/hid_profile.h>
#include "hid_usage_keyboard.h"
#include <toolbox/version.h>

/* RF tool includes */
#include <infrared/encoder_decoder/infrared.h>
#include <infrared/worker/infrared_transmit.h>
#include <ibutton/ibutton_worker.h>
#include <ibutton/ibutton_key.h>
#include <ibutton/ibutton_protocols.h>
#include <lfrfid/lfrfid_worker.h>
#include <lfrfid/protocols/lfrfid_protocols.h>
#include <lfrfid/lfrfid_dict_file.h>
#include <toolbox/protocols/protocol_dict.h>
#include <nfc/nfc.h>
#include <nfc/nfc_scanner.h>
#include <nfc/nfc_device.h>
#include <nfc/nfc_poller.h>
#include <nfc/nfc_listener.h>
#include <nfc/protocols/nfc_protocol.h>
#include <subghz/devices/devices.h>
#include <subghz/devices/cc1101_int/cc1101_int_interconnect.h>
#include <subghz/transmitter.h>
#include <subghz/receiver.h>
#include <subghz/environment.h>
#include <subghz/subghz_protocol_registry.h>
#include <subghz/subghz_file_encoder_worker.h>

#include <string.h>
#include <stdio.h>
#include <stdlib.h>

#define TAG "FlipperMCP"

#define DATA_DIR    EXT_PATH("apps_data/flipper_mcp")
#define CONFIG_FILE EXT_PATH("apps_data/flipper_mcp/config.txt")
#define LOG_FILE    EXT_PATH("apps_data/flipper_mcp/mcp.log")
#define LOG_MAX_SIZE (64 * 1024)  /* 64 KB max log file size */
#define LOG_TRIM_TO  (32 * 1024)  /* keep last 32 KB on trim */

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
    MenuSettings,
    MenuToggleSdLog,
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
    volatile bool esp_ready;          /* set true when PONG received from ESP32 */
    volatile bool log_to_sd;          /* when true, LOG| lines also written to SD */
    char log_file_path[256];          /* configurable SD log file path */
    int log_level;                    /* 0=errors, 1=normal (default), 2=verbose */

    /* BLE HID profile state (NULL when not active) */
    FuriHalBleProfileBase* ble_hid_profile;
    Bt* bt_held;  /* BT service handle, held open during HID session */
} FlipperMcpApp;

/* Forward declarations for functions used before their definition */
static void sd_log_append(FlipperMcpApp* app, const char* msg);

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

// -- BLE helper tables and functions ------------------------------------------

/** Parse a hex string "0201061A..." into a byte array.
 *  Returns the number of bytes parsed, or -1 on error. */
static int hex_to_bytes(const char* hex, uint8_t* out, size_t max_len) {
    size_t hex_len = strlen(hex);
    if(hex_len % 2 != 0) return -1;
    size_t byte_count = hex_len / 2;
    if(byte_count > max_len || byte_count == 0) return -1;
    for(size_t i = 0; i < byte_count; i++) {
        char byte_str[3] = {hex[i * 2], hex[i * 2 + 1], '\0'};
        char* endptr;
        unsigned long val = strtoul(byte_str, &endptr, 16);
        if(*endptr != '\0') return -1;
        out[i] = (uint8_t)val;
    }
    return (int)byte_count;
}

/** ASCII-to-HID key mapping: [ascii - 0x20] = { hid_keycode, needs_shift }
 *  US keyboard layout (standard for HID injection). */
typedef struct {
    uint8_t keycode;
    bool shift;
} AsciiToHid;

static const AsciiToHid ascii_hid_map[95] = {
    /* 0x20 ' ' */ {HID_KEYBOARD_SPACEBAR, false},
    /* 0x21 '!' */ {HID_KEYBOARD_1, true},
    /* 0x22 '"' */ {HID_KEYBOARD_APOSTROPHE, true},
    /* 0x23 '#' */ {HID_KEYBOARD_3, true},
    /* 0x24 '$' */ {HID_KEYBOARD_4, true},
    /* 0x25 '%' */ {HID_KEYBOARD_5, true},
    /* 0x26 '&' */ {HID_KEYBOARD_7, true},
    /* 0x27 ''' */ {HID_KEYBOARD_APOSTROPHE, false},
    /* 0x28 '(' */ {HID_KEYBOARD_9, true},
    /* 0x29 ')' */ {HID_KEYBOARD_0, true},
    /* 0x2A '*' */ {HID_KEYBOARD_8, true},
    /* 0x2B '+' */ {HID_KEYBOARD_EQUAL_SIGN, true},
    /* 0x2C ',' */ {HID_KEYBOARD_COMMA, false},
    /* 0x2D '-' */ {HID_KEYBOARD_MINUS, false},
    /* 0x2E '.' */ {HID_KEYBOARD_DOT, false},
    /* 0x2F '/' */ {HID_KEYBOARD_SLASH, false},
    /* 0x30-0x39: '0'-'9' */
    {HID_KEYBOARD_0, false}, {HID_KEYBOARD_1, false}, {HID_KEYBOARD_2, false},
    {HID_KEYBOARD_3, false}, {HID_KEYBOARD_4, false}, {HID_KEYBOARD_5, false},
    {HID_KEYBOARD_6, false}, {HID_KEYBOARD_7, false}, {HID_KEYBOARD_8, false},
    {HID_KEYBOARD_9, false},
    /* 0x3A ':' */ {HID_KEYBOARD_SEMICOLON, true},
    /* 0x3B ';' */ {HID_KEYBOARD_SEMICOLON, false},
    /* 0x3C '<' */ {HID_KEYBOARD_COMMA, true},
    /* 0x3D '=' */ {HID_KEYBOARD_EQUAL_SIGN, false},
    /* 0x3E '>' */ {HID_KEYBOARD_DOT, true},
    /* 0x3F '?' */ {HID_KEYBOARD_SLASH, true},
    /* 0x40 '@' */ {HID_KEYBOARD_2, true},
    /* 0x41-0x5A: 'A'-'Z' (shifted) */
    {HID_KEYBOARD_A, true}, {HID_KEYBOARD_B, true}, {HID_KEYBOARD_C, true},
    {HID_KEYBOARD_D, true}, {HID_KEYBOARD_E, true}, {HID_KEYBOARD_F, true},
    {HID_KEYBOARD_G, true}, {HID_KEYBOARD_H, true}, {HID_KEYBOARD_I, true},
    {HID_KEYBOARD_J, true}, {HID_KEYBOARD_K, true}, {HID_KEYBOARD_L, true},
    {HID_KEYBOARD_M, true}, {HID_KEYBOARD_N, true}, {HID_KEYBOARD_O, true},
    {HID_KEYBOARD_P, true}, {HID_KEYBOARD_Q, true}, {HID_KEYBOARD_R, true},
    {HID_KEYBOARD_S, true}, {HID_KEYBOARD_T, true}, {HID_KEYBOARD_U, true},
    {HID_KEYBOARD_V, true}, {HID_KEYBOARD_W, true}, {HID_KEYBOARD_X, true},
    {HID_KEYBOARD_Y, true}, {HID_KEYBOARD_Z, true},
    /* 0x5B '[' */ {HID_KEYBOARD_OPEN_BRACKET, false},
    /* 0x5C '\' */ {HID_KEYBOARD_BACKSLASH, false},
    /* 0x5D ']' */ {HID_KEYBOARD_CLOSE_BRACKET, false},
    /* 0x5E '^' */ {HID_KEYBOARD_6, true},
    /* 0x5F '_' */ {HID_KEYBOARD_MINUS, true},
    /* 0x60 '`' */ {HID_KEYBOARD_GRAVE_ACCENT, false},
    /* 0x61-0x7A: 'a'-'z' (unshifted) */
    {HID_KEYBOARD_A, false}, {HID_KEYBOARD_B, false}, {HID_KEYBOARD_C, false},
    {HID_KEYBOARD_D, false}, {HID_KEYBOARD_E, false}, {HID_KEYBOARD_F, false},
    {HID_KEYBOARD_G, false}, {HID_KEYBOARD_H, false}, {HID_KEYBOARD_I, false},
    {HID_KEYBOARD_J, false}, {HID_KEYBOARD_K, false}, {HID_KEYBOARD_L, false},
    {HID_KEYBOARD_M, false}, {HID_KEYBOARD_N, false}, {HID_KEYBOARD_O, false},
    {HID_KEYBOARD_P, false}, {HID_KEYBOARD_Q, false}, {HID_KEYBOARD_R, false},
    {HID_KEYBOARD_S, false}, {HID_KEYBOARD_T, false}, {HID_KEYBOARD_U, false},
    {HID_KEYBOARD_V, false}, {HID_KEYBOARD_W, false}, {HID_KEYBOARD_X, false},
    {HID_KEYBOARD_Y, false}, {HID_KEYBOARD_Z, false},
    /* 0x7B '{' */ {HID_KEYBOARD_OPEN_BRACKET, true},
    /* 0x7C '|' */ {HID_KEYBOARD_BACKSLASH, true},
    /* 0x7D '}' */ {HID_KEYBOARD_CLOSE_BRACKET, true},
    /* 0x7E '~' */ {HID_KEYBOARD_GRAVE_ACCENT, true},
};

typedef struct {
    const char* name;
    uint16_t keycode;
} KeyEntry;

static const KeyEntry special_keys[] = {
    {"ENTER", HID_KEYBOARD_RETURN},
    {"RETURN", HID_KEYBOARD_RETURN},
    {"TAB", HID_KEYBOARD_TAB},
    {"ESC", HID_KEYBOARD_ESCAPE},
    {"ESCAPE", HID_KEYBOARD_ESCAPE},
    {"SPACE", HID_KEYBOARD_SPACEBAR},
    {"BACKSPACE", HID_KEYBOARD_DELETE},
    {"DELETE", HID_KEYBOARD_DELETE_FORWARD},
    {"INSERT", HID_KEYBOARD_INSERT},
    {"HOME", HID_KEYBOARD_HOME},
    {"END", HID_KEYBOARD_END},
    {"PAGEUP", HID_KEYBOARD_PAGE_UP},
    {"PAGEDOWN", HID_KEYBOARD_PAGE_DOWN},
    {"UP", HID_KEYBOARD_UP_ARROW},
    {"DOWN", HID_KEYBOARD_DOWN_ARROW},
    {"LEFT", HID_KEYBOARD_LEFT_ARROW},
    {"RIGHT", HID_KEYBOARD_RIGHT_ARROW},
    {"F1", HID_KEYBOARD_F1}, {"F2", HID_KEYBOARD_F2}, {"F3", HID_KEYBOARD_F3},
    {"F4", HID_KEYBOARD_F4}, {"F5", HID_KEYBOARD_F5}, {"F6", HID_KEYBOARD_F6},
    {"F7", HID_KEYBOARD_F7}, {"F8", HID_KEYBOARD_F8}, {"F9", HID_KEYBOARD_F9},
    {"F10", HID_KEYBOARD_F10}, {"F11", HID_KEYBOARD_F11}, {"F12", HID_KEYBOARD_F12},
    {"PRINTSCREEN", HID_KEYBOARD_PRINT_SCREEN},
    {"CAPSLOCK", HID_KEYBOARD_CAPS_LOCK},
    {"SCROLLLOCK", HID_KEYBOARD_SCROLL_LOCK},
    {"NUMLOCK", HID_KEYPAD_NUMLOCK},
    {"PAUSE", HID_KEYBOARD_PAUSE},
};
#define SPECIAL_KEY_COUNT (sizeof(special_keys) / sizeof(special_keys[0]))

static const KeyEntry modifier_keys[] = {
    {"CTRL", HID_KEYBOARD_L_CTRL},
    {"CONTROL", HID_KEYBOARD_L_CTRL},
    {"SHIFT", HID_KEYBOARD_L_SHIFT},
    {"ALT", HID_KEYBOARD_L_ALT},
    {"GUI", HID_KEYBOARD_L_GUI},
    {"WIN", HID_KEYBOARD_L_GUI},
    {"WINDOWS", HID_KEYBOARD_L_GUI},
    {"META", HID_KEYBOARD_L_GUI},
};
#define MODIFIER_KEY_COUNT (sizeof(modifier_keys) / sizeof(modifier_keys[0]))

static uint16_t lookup_special_key(const char* name) {
    for(size_t i = 0; i < SPECIAL_KEY_COUNT; i++) {
        if(strcasecmp(special_keys[i].name, name) == 0) return special_keys[i].keycode;
    }
    return 0;
}

static uint16_t lookup_modifier(const char* name) {
    for(size_t i = 0; i < MODIFIER_KEY_COUNT; i++) {
        if(strcasecmp(modifier_keys[i].name, name) == 0) return modifier_keys[i].keycode;
    }
    return 0;
}

static bool cmd_ble(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    /* ---- ble info ---- */
    if(strcmp(subcmd, "info") == 0) {
        bool alive = furi_hal_bt_is_alive();
        bool active = furi_hal_bt_is_active();
        bool beacon_active = furi_hal_bt_extra_beacon_is_active();
        FuriHalBtStack stack = furi_hal_bt_get_radio_stack();
        const char* stack_str = "Unknown";
        if(stack == FuriHalBtStackLight) stack_str = "Light";
        else if(stack == FuriHalBtStackFull) stack_str = "Full";

        FuriString* dump = furi_string_alloc();
        furi_hal_bt_dump_state(dump);

        snprintf(
            result,
            result_size,
            "bt_alive: %s\nbt_active: %s\nradio_stack: %s\n"
            "extra_beacon: %s\nhid_active: %s\n%s",
            alive ? "yes" : "no",
            active ? "yes" : "no",
            stack_str,
            beacon_active ? "yes" : "no",
            app->ble_hid_profile ? "yes" : "no",
            furi_string_get_cstr(dump));
        furi_string_free(dump);
        return true;

    /* ---- ble beacon <hex_data> [--mac X] [--interval N] [--power N] ---- */
    } else if(strncmp(subcmd, "beacon ", 7) == 0) {
        const char* args_str = subcmd + 7;
        char hex_data[64] = {0};
        sscanf(args_str, "%63s", hex_data);

        uint8_t adv_data[EXTRA_BEACON_MAX_DATA_SIZE];
        int data_len = hex_to_bytes(hex_data, adv_data, EXTRA_BEACON_MAX_DATA_SIZE);
        if(data_len < 1) {
            snprintf(result, result_size, "Invalid hex data (1-31 bytes required)");
            return false;
        }

        uint16_t interval = 100;
        uint8_t mac[EXTRA_BEACON_MAC_ADDR_SIZE] = {0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01};
        bool custom_mac = false;

        const char* p;
        if((p = strstr(args_str, "--interval ")) != NULL) {
            interval = (uint16_t)atoi(p + 11);
            if(interval < 20) interval = 20;
            if(interval > 10240) interval = 10240;
        }
        if((p = strstr(args_str, "--mac ")) != NULL) {
            char mac_hex[13] = {0};
            sscanf(p + 6, "%12s", mac_hex);
            if(hex_to_bytes(mac_hex, mac, 6) == 6) custom_mac = true;
        }

        if(furi_hal_bt_extra_beacon_is_active()) {
            furi_hal_bt_extra_beacon_stop();
        }

        GapExtraBeaconConfig config = {
            .min_adv_interval_ms = interval,
            .max_adv_interval_ms = interval,
            .adv_channel_map = GapAdvChannelMapAll,
            .adv_power_level = GapAdvPowerLevel_0dBm,
            .address_type = custom_mac ? GapAddressTypePublic : GapAddressTypeRandom,
        };
        memcpy(config.address, mac, EXTRA_BEACON_MAC_ADDR_SIZE);

        if(!furi_hal_bt_extra_beacon_set_config(&config)) {
            snprintf(result, result_size, "Failed to set beacon config");
            return false;
        }
        if(!furi_hal_bt_extra_beacon_set_data(adv_data, (uint8_t)data_len)) {
            snprintf(result, result_size, "Failed to set beacon data");
            return false;
        }
        if(!furi_hal_bt_extra_beacon_start()) {
            snprintf(result, result_size, "Failed to start beacon");
            return false;
        }

        snprintf(
            result,
            result_size,
            "Beacon started\ndata: %d bytes\ninterval: %dms\n"
            "mac: %02X:%02X:%02X:%02X:%02X:%02X",
            data_len,
            interval,
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        return true;

    /* ---- ble beacon_stop ---- */
    } else if(strcmp(subcmd, "beacon_stop") == 0) {
        if(furi_hal_bt_extra_beacon_is_active()) {
            furi_hal_bt_extra_beacon_stop();
        }
        snprintf(result, result_size, "Beacon stopped");
        return true;

    /* ---- ble hid_start [--name X] ---- */
    } else if(strncmp(subcmd, "hid_start", 9) == 0) {
        if(app->ble_hid_profile) {
            snprintf(result, result_size, "HID profile already active");
            return false;
        }

        char name[9] = "FlpMCP";
        const char* name_arg = strstr(subcmd, "--name ");
        if(name_arg) sscanf(name_arg + 7, "%8s", name);

        BleProfileHidParams params = {
            .device_name_prefix = name,
            .mac_xor = 0,
        };

        Bt* bt = furi_record_open(RECORD_BT);
        app->bt_held = bt;

        app->ble_hid_profile =
            bt_profile_start(bt, ble_profile_hid, (FuriHalBleProfileParams)&params);
        if(!app->ble_hid_profile) {
            furi_record_close(RECORD_BT);
            app->bt_held = NULL;
            snprintf(result, result_size, "Failed to start HID profile");
            return false;
        }

        snprintf(
            result,
            result_size,
            "BLE HID started as '%s'\n"
            "WARNING: Mobile app disconnected.\n"
            "Target must pair to Flipper.\n"
            "Use ble_hid_stop to restore.",
            name);
        return true;

    /* ---- ble hid_type <text> [--delay N] ---- */
    } else if(strncmp(subcmd, "hid_type ", 9) == 0) {
        if(!app->ble_hid_profile) {
            snprintf(result, result_size, "HID not active. Call ble_hid_start first.");
            return false;
        }

        const char* text = subcmd + 9;
        int delay_ms = 30;
        const char* delay_arg = strstr(text, " --delay ");
        size_t text_len;
        if(delay_arg) {
            text_len = (size_t)(delay_arg - text);
            delay_ms = atoi(delay_arg + 9);
            if(delay_ms < 1) delay_ms = 1;
            if(delay_ms > 500) delay_ms = 500;
        } else {
            text_len = strlen(text);
        }

        size_t typed = 0;
        for(size_t i = 0; i < text_len; i++) {
            char c = text[i];
            /* Handle escaped \n as ENTER */
            if(c == '\\' && i + 1 < text_len && text[i + 1] == 'n') {
                ble_profile_hid_kb_press(app->ble_hid_profile, HID_KEYBOARD_RETURN);
                furi_delay_ms(delay_ms);
                ble_profile_hid_kb_release(app->ble_hid_profile, HID_KEYBOARD_RETURN);
                furi_delay_ms(delay_ms);
                i++; /* skip 'n' */
                typed++;
                continue;
            }
            if(c < 0x20 || c > 0x7E) continue; /* skip non-printable */

            const AsciiToHid* entry = &ascii_hid_map[c - 0x20];
            if(entry->shift) {
                ble_profile_hid_kb_press(app->ble_hid_profile, HID_KEYBOARD_L_SHIFT);
                furi_delay_ms(5);
            }
            ble_profile_hid_kb_press(app->ble_hid_profile, entry->keycode);
            furi_delay_ms(delay_ms);
            ble_profile_hid_kb_release(app->ble_hid_profile, entry->keycode);
            if(entry->shift) {
                ble_profile_hid_kb_release(app->ble_hid_profile, HID_KEYBOARD_L_SHIFT);
            }
            furi_delay_ms(delay_ms);
            typed++;
        }

        ble_profile_hid_kb_release_all(app->ble_hid_profile);
        snprintf(result, result_size, "Typed %zu characters (delay: %dms)", typed, delay_ms);
        return true;

    /* ---- ble hid_press <KEY_COMBO> ---- */
    } else if(strncmp(subcmd, "hid_press ", 10) == 0) {
        if(!app->ble_hid_profile) {
            snprintf(result, result_size, "HID not active. Call ble_hid_start first.");
            return false;
        }

        const char* combo = subcmd + 10;
        uint16_t modifiers[4] = {0};
        int mod_count = 0;
        uint16_t main_key = 0;

        /* Split on '+' manually (strtok_r not in Flipper SDK) */
        const char* p = combo;
        while(*p) {
            while(*p == ' ' || *p == '+') p++;
            if(!*p) break;
            const char* tok_start = p;
            while(*p && *p != '+') p++;
            /* trim trailing spaces */
            const char* tok_end = p;
            while(tok_end > tok_start && *(tok_end - 1) == ' ') tok_end--;
            size_t tok_len = tok_end - tok_start;
            if(tok_len == 0) continue;

            char token[32];
            if(tok_len >= sizeof(token)) tok_len = sizeof(token) - 1;
            memcpy(token, tok_start, tok_len);
            token[tok_len] = '\0';

            uint16_t mod = lookup_modifier(token);
            if(mod) {
                if(mod_count < 4) modifiers[mod_count++] = mod;
            } else {
                uint16_t special = lookup_special_key(token);
                if(special) {
                    main_key = special;
                } else if(tok_len == 1 && token[0] >= 0x20 && token[0] <= 0x7E) {
                    const AsciiToHid* entry = &ascii_hid_map[(uint8_t)token[0] - 0x20];
                    main_key = entry->keycode;
                    if(entry->shift && mod_count < 4) {
                        modifiers[mod_count++] = HID_KEYBOARD_L_SHIFT;
                    }
                } else {
                    snprintf(result, result_size, "Unknown key: %s", token);
                    return false;
                }
            }
        }

        for(int i = 0; i < mod_count; i++) {
            ble_profile_hid_kb_press(app->ble_hid_profile, modifiers[i]);
            furi_delay_ms(5);
        }
        if(main_key) {
            ble_profile_hid_kb_press(app->ble_hid_profile, main_key);
            furi_delay_ms(50);
            ble_profile_hid_kb_release(app->ble_hid_profile, main_key);
        }
        for(int i = mod_count - 1; i >= 0; i--) {
            ble_profile_hid_kb_release(app->ble_hid_profile, modifiers[i]);
        }

        snprintf(result, result_size, "Key pressed: %s", combo);
        return true;

    /* ---- ble hid_mouse [dx] [dy] [--button X] [--action X] [--scroll N] ---- */
    } else if(strncmp(subcmd, "hid_mouse", 9) == 0) {
        if(!app->ble_hid_profile) {
            snprintf(result, result_size, "HID not active. Call ble_hid_start first.");
            return false;
        }

        const char* args_str = subcmd + 9;
        int dx = 0, dy = 0, scroll = 0;
        char button[8] = {0};
        char action[8] = "click";

        sscanf(args_str, " %d %d", &dx, &dy);
        const char* p;
        if((p = strstr(args_str, "--button ")) != NULL) sscanf(p + 9, "%7s", button);
        if((p = strstr(args_str, "--action ")) != NULL) sscanf(p + 9, "%7s", action);
        if((p = strstr(args_str, "--scroll ")) != NULL) scroll = atoi(p + 9);

        if(dx > 127) dx = 127;
        if(dx < -128) dx = -128;
        if(dy > 127) dy = 127;
        if(dy < -128) dy = -128;
        if(scroll > 127) scroll = 127;
        if(scroll < -128) scroll = -128;

        if(dx != 0 || dy != 0) {
            ble_profile_hid_mouse_move(app->ble_hid_profile, (int8_t)dx, (int8_t)dy);
        }

        if(button[0]) {
            uint8_t btn = 0;
            if(strcasecmp(button, "LEFT") == 0) btn = 1;
            else if(strcasecmp(button, "RIGHT") == 0) btn = 2;
            else if(strcasecmp(button, "MIDDLE") == 0) btn = 4;

            if(btn) {
                if(strcmp(action, "click") == 0) {
                    ble_profile_hid_mouse_press(app->ble_hid_profile, btn);
                    furi_delay_ms(50);
                    ble_profile_hid_mouse_release(app->ble_hid_profile, btn);
                } else if(strcmp(action, "press") == 0) {
                    ble_profile_hid_mouse_press(app->ble_hid_profile, btn);
                } else if(strcmp(action, "release") == 0) {
                    ble_profile_hid_mouse_release(app->ble_hid_profile, btn);
                }
            }
        }

        if(scroll != 0) {
            ble_profile_hid_mouse_scroll(app->ble_hid_profile, (int8_t)scroll);
        }

        snprintf(
            result,
            result_size,
            "Mouse: dx=%d dy=%d btn=%s act=%s scroll=%d",
            dx, dy, button[0] ? button : "none", action, scroll);
        return true;

    /* ---- ble hid_stop ---- */
    } else if(strcmp(subcmd, "hid_stop") == 0) {
        if(!app->ble_hid_profile) {
            snprintf(result, result_size, "HID not active");
            return true; /* idempotent */
        }

        ble_profile_hid_kb_release_all(app->ble_hid_profile);
        ble_profile_hid_mouse_release_all(app->ble_hid_profile);

        if(app->bt_held) {
            bt_profile_restore_default(app->bt_held);
            furi_record_close(RECORD_BT);
            app->bt_held = NULL;
        }
        app->ble_hid_profile = NULL;

        snprintf(result, result_size, "BLE HID stopped. Default BT profile restored.");
        return true;

    } else {
        snprintf(
            result,
            result_size,
            "Unknown BLE command: %.40s\n"
            "Valid: info, beacon, beacon_stop, hid_start, hid_type, hid_press, hid_mouse, hid_stop",
            subcmd);
        return false;
    }
}

// -- Infrared handler ---------------------------------------------------------

static bool cmd_ir(const char* subcmd, char* result, size_t result_size) {
    if(strncmp(subcmd, "tx ", 3) == 0) {
        char protocol_name[32] = {0};
        uint32_t address = 0;
        uint32_t command = 0;
        int repeat = 1;

        int parsed = sscanf(subcmd + 3, "%31s %lx %lx %d", protocol_name, &address, &command, &repeat);
        if(parsed < 3) {
            snprintf(result, result_size, "Usage: ir tx <protocol> <address_hex> <command_hex> [repeat]");
            return false;
        }
        if(repeat < 1) repeat = 1;
        if(repeat > 20) repeat = 20;

        InfraredProtocol proto = infrared_get_protocol_by_name(protocol_name);
        if(proto == InfraredProtocolUnknown) {
            snprintf(result, result_size, "Unknown IR protocol: %s", protocol_name);
            return false;
        }

        InfraredMessage msg = {
            .protocol = proto,
            .address = address,
            .command = command,
            .repeat = false,
        };

        infrared_send(&msg, repeat);
        snprintf(
            result,
            result_size,
            "IR TX: %s addr=0x%lX cmd=0x%lX repeat=%d",
            protocol_name,
            address,
            command,
            repeat);
        return true;

    } else if(strncmp(subcmd, "tx_raw ", 7) == 0) {
        /* ir tx_raw <frequency> <duty_cycle> <mark> <space> <mark> ... */
        const char* p = subcmd + 7;
        uint32_t frequency = 0;
        float duty_cycle = 0.0f;
        int offset = 0;

        if(sscanf(p, "%lu %f%n", &frequency, &duty_cycle, &offset) < 2) {
            snprintf(result, result_size, "Usage: ir tx_raw <freq_hz> <duty_cycle> <timing1> <timing2> ...");
            return false;
        }
        p += offset;

        /* Parse timing values (max 512 entries) */
        uint32_t timings[512];
        size_t count = 0;
        while(count < 512) {
            uint32_t val = 0;
            int n = 0;
            if(sscanf(p, " %lu%n", &val, &n) < 1) break;
            timings[count++] = val;
            p += n;
        }
        if(count < 2) {
            snprintf(result, result_size, "Need at least 2 timing values");
            return false;
        }

        infrared_send_raw_ext(timings, count, true, frequency, duty_cycle);
        snprintf(result, result_size, "IR TX raw: %zu timings at %luHz", count, frequency);
        return true;

    } else {
        snprintf(result, result_size, "Unknown ir command. Valid: tx, tx_raw");
        return false;
    }
}

// -- iButton handler ----------------------------------------------------------

typedef struct {
    FuriSemaphore* sem;
    bool success;
} IButtonReadCtx;

static void ibutton_read_cb(void* context) {
    IButtonReadCtx* ctx = context;
    ctx->success = true;
    furi_semaphore_release(ctx->sem);
}

static bool cmd_ibutton(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    if(strncmp(subcmd, "read", 4) == 0) {
        iButtonProtocols* protocols = ibutton_protocols_alloc();
        size_t max_size = ibutton_protocols_get_max_data_size(protocols);
        iButtonKey* key = ibutton_key_alloc(max_size);
        iButtonWorker* worker = ibutton_worker_alloc(protocols);

        IButtonReadCtx ctx = {
            .sem = furi_semaphore_alloc(1, 0),
            .success = false,
        };

        ibutton_worker_read_set_callback(worker, ibutton_read_cb, &ctx);
        ibutton_worker_start_thread(worker);
        ibutton_worker_read_start(worker, key);

        FuriStatus status = furi_semaphore_acquire(ctx.sem, 10000);
        ibutton_worker_stop(worker);
        ibutton_worker_stop_thread(worker);

        if(status == FuriStatusOk && ctx.success) {
            FuriString* uid_str = furi_string_alloc();
            ibutton_protocols_render_uid(protocols, key, uid_str);
            iButtonProtocolId proto_id = ibutton_key_get_protocol_id(key);
            const char* proto_name = ibutton_protocols_get_name(protocols, proto_id);
            snprintf(
                result,
                result_size,
                "iButton read OK\nprotocol: %s\nuid: %s",
                proto_name ? proto_name : "unknown",
                furi_string_get_cstr(uid_str));
            furi_string_free(uid_str);

            /* If subcmd is "read_and_save <path>", also save the key */
            if(strncmp(subcmd, "read_and_save ", 14) == 0) {
                const char* path = subcmd + 14;
                if(ibutton_protocols_save(protocols, key, path)) {
                    size_t len = strlen(result);
                    snprintf(result + len, result_size - len, "\nsaved: %s", path);
                } else {
                    size_t len = strlen(result);
                    snprintf(result + len, result_size - len, "\nsave FAILED: %s", path);
                }
            }
        } else {
            snprintf(result, result_size, "iButton read timeout — no key detected within 10s");
        }

        furi_semaphore_free(ctx.sem);
        ibutton_worker_free(worker);
        ibutton_key_free(key);
        ibutton_protocols_free(protocols);
        return (status == FuriStatusOk && ctx.success);

    } else if(strncmp(subcmd, "emulate ", 8) == 0) {
        const char* path = subcmd + 8;
        iButtonProtocols* protocols = ibutton_protocols_alloc();
        size_t max_size = ibutton_protocols_get_max_data_size(protocols);
        iButtonKey* key = ibutton_key_alloc(max_size);

        if(!ibutton_protocols_load(protocols, key, path)) {
            snprintf(result, result_size, "Failed to load iButton file: %s", path);
            ibutton_key_free(key);
            ibutton_protocols_free(protocols);
            return false;
        }

        ibutton_protocols_emulate_start(protocols, key);
        furi_delay_ms(10000); /* Emulate for 10 seconds */
        ibutton_protocols_emulate_stop(protocols, key);

        iButtonProtocolId proto_id = ibutton_key_get_protocol_id(key);
        const char* proto_name = ibutton_protocols_get_name(protocols, proto_id);
        snprintf(
            result,
            result_size,
            "iButton emulate done (10s): %s from %s",
            proto_name ? proto_name : "unknown",
            path);

        ibutton_key_free(key);
        ibutton_protocols_free(protocols);
        return true;

    } else {
        snprintf(
            result,
            result_size,
            "Unknown ikey command: %.40s\nValid: read, read_and_save <path>, emulate <path>",
            subcmd);
        return false;
    }

    UNUSED(app);
}

// -- RFID handler -------------------------------------------------------------

typedef struct {
    FuriSemaphore* sem;
    LFRFIDWorkerReadResult read_result;
    ProtocolId protocol;
} RfidReadCtx;

static void rfid_read_cb(LFRFIDWorkerReadResult result, ProtocolId protocol, void* context) {
    RfidReadCtx* ctx = context;
    ctx->read_result = result;
    ctx->protocol = protocol;
    if(result == LFRFIDWorkerReadDone) {
        furi_semaphore_release(ctx->sem);
    }
}

static bool cmd_rfid(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    if(strncmp(subcmd, "read", 4) == 0) {
        ProtocolDict* dict = protocol_dict_alloc(lfrfid_protocols, LFRFIDProtocolMax);
        LFRFIDWorker* worker = lfrfid_worker_alloc(dict);

        RfidReadCtx ctx = {
            .sem = furi_semaphore_alloc(1, 0),
            .read_result = -1,
            .protocol = PROTOCOL_NO,
        };

        lfrfid_worker_start_thread(worker);
        lfrfid_worker_read_start(worker, LFRFIDWorkerReadTypeAuto, rfid_read_cb, &ctx);

        FuriStatus status = furi_semaphore_acquire(ctx.sem, 10000);
        lfrfid_worker_stop(worker);
        lfrfid_worker_stop_thread(worker);

        if(status == FuriStatusOk && ctx.protocol != PROTOCOL_NO) {
            FuriString* uid_str = furi_string_alloc();
            FuriString* data_str = furi_string_alloc();
            protocol_dict_render_uid(dict, uid_str, ctx.protocol);
            protocol_dict_render_data(dict, data_str, ctx.protocol);
            const char* name = protocol_dict_get_name(dict, ctx.protocol);
            snprintf(
                result,
                result_size,
                "RFID read OK\nprotocol: %s\nuid: %s\ndata: %s",
                name ? name : "unknown",
                furi_string_get_cstr(uid_str),
                furi_string_get_cstr(data_str));
            furi_string_free(uid_str);
            furi_string_free(data_str);

            /* If subcmd is "read_and_save <path>", also save */
            if(strncmp(subcmd, "read_and_save ", 14) == 0) {
                const char* path = subcmd + 14;
                if(lfrfid_dict_file_save(dict, ctx.protocol, path)) {
                    size_t len = strlen(result);
                    snprintf(result + len, result_size - len, "\nsaved: %s", path);
                } else {
                    size_t len = strlen(result);
                    snprintf(result + len, result_size - len, "\nsave FAILED: %s", path);
                }
            }
        } else {
            snprintf(result, result_size, "RFID read timeout — no tag detected within 10s");
        }

        furi_semaphore_free(ctx.sem);
        lfrfid_worker_free(worker);
        protocol_dict_free(dict);
        return (status == FuriStatusOk && ctx.protocol != PROTOCOL_NO);

    } else if(strncmp(subcmd, "emulate ", 8) == 0) {
        const char* path = subcmd + 8;
        ProtocolDict* dict = protocol_dict_alloc(lfrfid_protocols, LFRFIDProtocolMax);
        ProtocolId proto = lfrfid_dict_file_load(dict, path);
        if(proto == PROTOCOL_NO) {
            snprintf(result, result_size, "Failed to load RFID file: %s", path);
            protocol_dict_free(dict);
            return false;
        }

        LFRFIDWorker* worker = lfrfid_worker_alloc(dict);
        lfrfid_worker_start_thread(worker);
        lfrfid_worker_emulate_start(worker, (LFRFIDProtocol)proto);

        furi_delay_ms(10000); /* Emulate for 10 seconds */

        lfrfid_worker_stop(worker);
        lfrfid_worker_stop_thread(worker);

        const char* name = protocol_dict_get_name(dict, proto);
        snprintf(
            result,
            result_size,
            "RFID emulate done (10s): %s from %s",
            name ? name : "unknown",
            path);

        lfrfid_worker_free(worker);
        protocol_dict_free(dict);
        return true;

    } else {
        snprintf(
            result,
            result_size,
            "Unknown rfid command: %.40s\nValid: read, read_and_save <path>, emulate <path>",
            subcmd);
        return false;
    }

    UNUSED(app);
}

// -- NFC handler --------------------------------------------------------------

typedef struct {
    FuriSemaphore* sem;
    NfcProtocol detected_protocols[NfcProtocolNum];
    size_t detected_count;
} NfcScanCtx;

static void nfc_scan_cb(NfcScannerEvent event, void* context) {
    NfcScanCtx* ctx = context;
    if(event.type == NfcScannerEventTypeDetected) {
        ctx->detected_count =
            event.data.protocol_num > NfcProtocolNum ? NfcProtocolNum : event.data.protocol_num;
        for(size_t i = 0; i < ctx->detected_count; i++) {
            ctx->detected_protocols[i] = event.data.protocols[i];
        }
        furi_semaphore_release(ctx->sem);
    }
}

static bool cmd_nfc(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    if(strncmp(subcmd, "detect", 6) == 0) {
        Nfc* nfc = nfc_alloc();
        NfcScanner* scanner = nfc_scanner_alloc(nfc);

        NfcScanCtx ctx = {
            .sem = furi_semaphore_alloc(1, 0),
            .detected_count = 0,
        };

        nfc_scanner_start(scanner, nfc_scan_cb, &ctx);
        FuriStatus status = furi_semaphore_acquire(ctx.sem, 10000);
        nfc_scanner_stop(scanner);

        if(status == FuriStatusOk && ctx.detected_count > 0) {
            int off = snprintf(result, result_size, "NFC detected %zu protocol(s):", ctx.detected_count);
            for(size_t i = 0; i < ctx.detected_count && off < (int)result_size - 1; i++) {
                const char* name = nfc_device_get_protocol_name(ctx.detected_protocols[i]);
                off += snprintf(
                    result + off,
                    result_size - off,
                    "\n  - %s",
                    name ? name : "unknown");
            }
        } else {
            snprintf(result, result_size, "NFC detect timeout — no tag found within 10s");
        }

        furi_semaphore_free(ctx.sem);
        nfc_scanner_free(scanner);
        nfc_free(nfc);
        return (status == FuriStatusOk && ctx.detected_count > 0);

    } else if(strncmp(subcmd, "emulate ", 8) == 0) {
        const char* path = subcmd + 8;
        NfcDevice* device = nfc_device_alloc();

        if(!nfc_device_load(device, path)) {
            snprintf(result, result_size, "Failed to load NFC file: %s", path);
            nfc_device_free(device);
            return false;
        }

        NfcProtocol proto = nfc_device_get_protocol(device);
        Nfc* nfc = nfc_alloc();
        NfcListener* listener = nfc_listener_alloc(
            nfc, proto, nfc_device_get_data(device, proto));

        nfc_listener_start(listener, NULL, NULL);
        furi_delay_ms(30000); /* Emulate for 30 seconds */
        nfc_listener_stop(listener);

        const char* name = nfc_device_get_protocol_name(proto);
        snprintf(
            result,
            result_size,
            "NFC emulate done (30s): %s from %s",
            name ? name : "unknown",
            path);

        nfc_listener_free(listener);
        nfc_free(nfc);
        nfc_device_free(device);
        return true;

    } else {
        snprintf(
            result,
            result_size,
            "Unknown nfc command: %.40s\nValid: detect, emulate <path>",
            subcmd);
        return false;
    }

    UNUSED(app);
}

// -- SubGHz handler -----------------------------------------------------------

typedef struct {
    FuriSemaphore* sem;
    FuriString* decoded_text;
    bool got_signal;
} SubGhzRxCtx;

static void subghz_rx_callback(
    SubGhzReceiver* recv,
    SubGhzProtocolDecoderBase* decoder,
    void* ctx_ptr) {
    UNUSED(recv);
    SubGhzRxCtx* rctx = ctx_ptr;
    if(!rctx->got_signal) {
        rctx->got_signal = true;
        FuriString* text = furi_string_alloc();
        subghz_protocol_decoder_base_get_string(decoder, text);
        furi_string_set(rctx->decoded_text, text);
        furi_string_free(text);
        furi_semaphore_release(rctx->sem);
    }
}

static bool cmd_subghz(FlipperMcpApp* app, const char* subcmd, char* result, size_t result_size) {
    if(strncmp(subcmd, "tx_from_file ", 13) == 0) {
        /* Transmit from .sub file using file encoder worker */
        const char* path = subcmd + 13;

        SubGhzEnvironment* env = subghz_environment_alloc();
        subghz_environment_set_protocol_registry(env, &subghz_protocol_registry);

        subghz_devices_init();
        const SubGhzDevice* device = subghz_devices_get_by_name(SUBGHZ_DEVICE_CC1101_INT_NAME);
        if(!device || !subghz_devices_begin(device)) {
            snprintf(result, result_size, "Failed to init CC1101");
            subghz_devices_deinit();
            subghz_environment_free(env);
            return false;
        }

        SubGhzFileEncoderWorker* file_worker = subghz_file_encoder_worker_alloc();
        if(!subghz_file_encoder_worker_start(file_worker, path, SUBGHZ_DEVICE_CC1101_INT_NAME)) {
            snprintf(result, result_size, "Failed to load .sub file: %s", path);
            subghz_file_encoder_worker_free(file_worker);
            subghz_devices_end(device);
            subghz_devices_deinit();
            subghz_environment_free(env);
            return false;
        }

        /* The file encoder worker reads frequency/preset from the file and
         * handles async TX internally. Wait for completion or timeout. */
        uint32_t start = furi_get_tick();
        while(subghz_file_encoder_worker_is_running(file_worker)) {
            furi_delay_ms(50);
            if(furi_get_tick() - start > 10000) break; /* 10s safety timeout */
        }

        subghz_file_encoder_worker_stop(file_worker);
        subghz_file_encoder_worker_free(file_worker);
        subghz_devices_sleep(device);
        subghz_devices_end(device);
        subghz_devices_deinit();
        subghz_environment_free(env);

        snprintf(result, result_size, "SubGHz TX from file done: %s", path);
        return true;

    } else if(strncmp(subcmd, "tx ", 3) == 0) {
        /* tx <protocol> <key_hex> <frequency> */
        char protocol_name[32] = {0};
        char key_hex[32] = {0};
        uint32_t frequency = 0;

        int parsed = sscanf(subcmd + 3, "%31s %31s %lu", protocol_name, key_hex, &frequency);
        if(parsed < 3) {
            snprintf(result, result_size, "Usage: subghz tx <protocol> <key_hex> <frequency>");
            return false;
        }

        subghz_devices_init();
        const SubGhzDevice* device = subghz_devices_get_by_name(SUBGHZ_DEVICE_CC1101_INT_NAME);
        if(!device || !subghz_devices_begin(device)) {
            snprintf(result, result_size, "Failed to init CC1101");
            subghz_devices_deinit();
            return false;
        }

        if(!subghz_devices_is_frequency_valid(device, frequency)) {
            snprintf(result, result_size, "Invalid frequency: %lu", frequency);
            subghz_devices_end(device);
            subghz_devices_deinit();
            return false;
        }

        SubGhzEnvironment* env = subghz_environment_alloc();
        subghz_environment_set_protocol_registry(env, &subghz_protocol_registry);

        SubGhzTransmitter* transmitter =
            subghz_transmitter_alloc_init(env, protocol_name);
        if(!transmitter) {
            snprintf(result, result_size, "Unknown SubGHz protocol: %s", protocol_name);
            subghz_environment_free(env);
            subghz_devices_end(device);
            subghz_devices_deinit();
            return false;
        }

        /* Build a FlipperFormat in memory with the key data */
        FlipperFormat* ff = flipper_format_string_alloc();
        flipper_format_write_header_cstr(ff, "Flipper SubGhz Key File", 1);
        flipper_format_write_uint32(ff, "Frequency", &frequency, 1);
        const char* preset_name = "FuriHalSubGhzPresetOok650Async";
        flipper_format_write_string_cstr(ff, "Preset", preset_name);
        flipper_format_write_string_cstr(ff, "Protocol", protocol_name);

        /* Parse key hex string to uint64_t */
        uint64_t key_val = 0;
        for(const char* kp = key_hex; *kp; kp++) {
            char c = *kp;
            uint8_t nib = 0;
            if(c >= '0' && c <= '9') nib = c - '0';
            else if(c >= 'A' && c <= 'F') nib = 10 + c - 'A';
            else if(c >= 'a' && c <= 'f') nib = 10 + c - 'a';
            else continue;
            key_val = (key_val << 4) | nib;
        }

        /* Count bits from hex length (each hex char = 4 bits) */
        uint32_t bit_count = 0;
        for(const char* kp = key_hex; *kp; kp++) {
            char c = *kp;
            if((c >= '0' && c <= '9') || (c >= 'A' && c <= 'F') || (c >= 'a' && c <= 'f'))
                bit_count += 4;
        }
        if(bit_count == 0) bit_count = 32;

        flipper_format_write_uint32(ff, "Bit", &bit_count, 1);
        flipper_format_write_hex_uint64(ff, "Key", &key_val, 1);
        flipper_format_rewind(ff);

        SubGhzProtocolStatus status = subghz_transmitter_deserialize(transmitter, ff);
        flipper_format_free(ff);

        if(status != SubGhzProtocolStatusOk) {
            snprintf(result, result_size, "Failed to build TX signal (status=%d)", (int)status);
            subghz_transmitter_free(transmitter);
            subghz_environment_free(env);
            subghz_devices_end(device);
            subghz_devices_deinit();
            return false;
        }

        subghz_devices_set_frequency(device, frequency);
        subghz_devices_load_preset(device, FuriHalSubGhzPresetOok650Async, NULL);

        if(!subghz_devices_set_tx(device)) {
            snprintf(result, result_size, "CC1101 TX failed (frequency blocked or busy)");
            subghz_transmitter_free(transmitter);
            subghz_environment_free(env);
            subghz_devices_end(device);
            subghz_devices_deinit();
            return false;
        }

        subghz_devices_start_async_tx(device, subghz_transmitter_yield, transmitter);

        /* Wait for TX completion or timeout */
        uint32_t start = furi_get_tick();
        while(!subghz_devices_is_async_complete_tx(device)) {
            furi_delay_ms(10);
            if(furi_get_tick() - start > 5000) break;
        }

        subghz_devices_stop_async_tx(device);
        subghz_transmitter_free(transmitter);
        subghz_devices_idle(device);
        subghz_devices_end(device);
        subghz_devices_deinit();
        subghz_environment_free(env);

        snprintf(
            result,
            result_size,
            "SubGHz TX: %s key=%s freq=%lu bit=%lu",
            protocol_name,
            key_hex,
            frequency,
            bit_count);
        return true;

    } else if(strncmp(subcmd, "rx ", 3) == 0) {
        /* rx <frequency> [duration_ms] */
        uint32_t frequency = 0;
        uint32_t duration_ms = 5000;

        sscanf(subcmd + 3, "%lu %lu", &frequency, &duration_ms);
        if(frequency == 0) {
            snprintf(result, result_size, "Usage: subghz rx <frequency> [duration_ms]");
            return false;
        }
        if(duration_ms < 1000) duration_ms = 1000;
        if(duration_ms > 30000) duration_ms = 30000;

        subghz_devices_init();
        const SubGhzDevice* device = subghz_devices_get_by_name(SUBGHZ_DEVICE_CC1101_INT_NAME);
        if(!device || !subghz_devices_begin(device)) {
            snprintf(result, result_size, "Failed to init CC1101");
            subghz_devices_deinit();
            return false;
        }

        if(!subghz_devices_is_frequency_valid(device, frequency)) {
            snprintf(result, result_size, "Invalid frequency: %lu", frequency);
            subghz_devices_end(device);
            subghz_devices_deinit();
            return false;
        }

        SubGhzEnvironment* env = subghz_environment_alloc();
        subghz_environment_set_protocol_registry(env, &subghz_protocol_registry);

        SubGhzReceiver* receiver = subghz_receiver_alloc_init(env);
        subghz_receiver_set_filter(receiver, SubGhzProtocolFlag_Decodable);

        SubGhzRxCtx rx_ctx = {
            .sem = furi_semaphore_alloc(1, 0),
            .decoded_text = furi_string_alloc(),
            .got_signal = false,
        };

        subghz_receiver_set_rx_callback(receiver, subghz_rx_callback, &rx_ctx);

        subghz_devices_set_frequency(device, frequency);
        subghz_devices_load_preset(device, FuriHalSubGhzPresetOok650Async, NULL);
        subghz_devices_set_rx(device);
        subghz_devices_start_async_rx(device, subghz_receiver_decode, receiver);

        furi_semaphore_acquire(rx_ctx.sem, duration_ms);

        subghz_devices_stop_async_rx(device);
        subghz_devices_idle(device);

        if(rx_ctx.got_signal) {
            snprintf(
                result,
                result_size,
                "SubGHz RX at %luHz:\n%s",
                frequency,
                furi_string_get_cstr(rx_ctx.decoded_text));
        } else {
            snprintf(
                result,
                result_size,
                "SubGHz RX at %luHz: no signal decoded within %lums",
                frequency,
                duration_ms);
        }

        furi_string_free(rx_ctx.decoded_text);
        furi_semaphore_free(rx_ctx.sem);
        subghz_receiver_free(receiver);
        subghz_devices_end(device);
        subghz_devices_deinit();
        subghz_environment_free(env);
        return rx_ctx.got_signal;

    } else {
        snprintf(
            result,
            result_size,
            "Unknown subghz command: %.40s\nValid: tx, rx, tx_from_file",
            subcmd);
        return false;
    }

    UNUSED(app);
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
        /* Send response before reboot — use small inline buffer */
        uart_send(app, "CLI_OK|rebooting");
        furi_delay_ms(100);
        furi_hal_power_reset();
        return; /* won't reach here */
    } else if(strncmp(command, "gpio ", 5) == 0) {
        ok = cmd_gpio(command + 5, result, sizeof(result));
    } else if(strncmp(command, "storage ", 8) == 0) {
        ok = cmd_storage(app, command + 8, result, sizeof(result));
    } else if(strncmp(command, "ble ", 4) == 0) {
        ok = cmd_ble(app, command + 4, result, sizeof(result));
    } else if(strncmp(command, "ir ", 3) == 0) {
        ok = cmd_ir(command + 3, result, sizeof(result));
    } else if(strncmp(command, "ikey ", 5) == 0) {
        ok = cmd_ibutton(app, command + 5, result, sizeof(result));
    } else if(strncmp(command, "rfid ", 5) == 0) {
        ok = cmd_rfid(app, command + 5, result, sizeof(result));
    } else if(strncmp(command, "nfc ", 4) == 0) {
        ok = cmd_nfc(app, command + 4, result, sizeof(result));
    } else if(strncmp(command, "subghz ", 7) == 0) {
        ok = cmd_subghz(app, command + 7, result, sizeof(result));
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

    /* Escape newlines and send response — use heap to avoid stack overflow */
    size_t result_len = strlen(result);
    size_t escaped_size = result_len * 2 + 1; /* worst case: every char is \n */
    if(escaped_size < 128) escaped_size = 128;
    char* escaped = malloc(escaped_size);
    if(!escaped) {
        uart_send(app, "CLI_ERR|out of memory");
        return;
    }
    escape_newlines(result, escaped, escaped_size);
    size_t response_size = strlen(escaped) + 16;
    char* response = malloc(response_size);
    if(!response) {
        uart_send(app, "CLI_ERR|out of memory");
        free(escaped);
        return;
    }
    snprintf(response, response_size, "%s|%s", ok ? "CLI_OK" : "CLI_ERR", escaped);
    uart_send(app, response);
    free(response);
    free(escaped);
}

/* Forward declaration — defined further down in the file */
static bool write_file_str(FlipperMcpApp* app, const char* path, const char* content);

/** Handle WRITE_FILE|path|content from ESP32 */
static void handle_write_file(FlipperMcpApp* app, const char* payload) {
    /* payload = "path|escaped_content" — use heap to avoid stack overflow */
    size_t payload_len = strlen(payload);
    size_t alloc_size = payload_len + 1;
    if(alloc_size > 4096) alloc_size = 4096;

    char* payload_copy = malloc(alloc_size);
    if(!payload_copy) {
        uart_send(app, "CLI_ERR|out of memory");
        return;
    }
    strncpy(payload_copy, payload, alloc_size - 1);
    payload_copy[alloc_size - 1] = '\0';

    char* pipe = strchr(payload_copy, '|');
    if(!pipe) {
        uart_send(app, "CLI_ERR|Invalid WRITE_FILE format (no pipe)");
        free(payload_copy);
        return;
    }
    *pipe = '\0';
    const char* path = payload_copy;
    const char* escaped_content = pipe + 1;

    /* Unescape \\n -> \n — allocate on heap */
    char* content = malloc(alloc_size);
    if(!content) {
        uart_send(app, "CLI_ERR|out of memory");
        free(payload_copy);
        return;
    }
    size_t ci = 0;
    for(size_t i = 0; escaped_content[i] && ci + 1 < alloc_size; i++) {
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

    free(content);
    free(payload_copy);
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
        /* Release mutex before SD I/O, then append to SD log file */
        furi_mutex_release(app->data_mutex);
        sd_log_append(app, msg);
        return; /* mutex already released */

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
        app->esp_ready = true;
        FURI_LOG_I(TAG, "PONG received — ESP32 handshake complete");

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
    uint32_t last_ping_tick = 0;

    FURI_LOG_I(TAG, "UART worker started");

    while(app->worker_running) {
        /* Send PING every 2s until ESP32 responds with PONG */
        if(!app->esp_ready) {
            uint32_t now = furi_get_tick();
            if(now - last_ping_tick >= 2000) {
                uart_send(app, "PING");
                last_ping_tick = now;
                FURI_LOG_D(TAG, "PING sent (waiting for ESP32 handshake)");
            }
        }

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

// -- SD card log helpers ------------------------------------------------------

/** Append a single log line to the SD card log file (if SD logging enabled). */
static void sd_log_append(FlipperMcpApp* app, const char* msg) {
    if(!app->log_to_sd) return;

    /* Create parent directory of log file */
    char dir_path[256];
    strncpy(dir_path, app->log_file_path, sizeof(dir_path) - 1);
    dir_path[sizeof(dir_path) - 1] = '\0';
    /* Find last slash and null-terminate at it */
    char* last_slash = strrchr(dir_path, '/');
    if(last_slash) {
        *last_slash = '\0';
        storage_simply_mkdir(app->storage, dir_path);
    }

    File* f = storage_file_alloc(app->storage);
    if(storage_file_open(f, app->log_file_path, FSAM_WRITE, FSOM_OPEN_APPEND)) {
        uint64_t size = storage_file_size(f);
        if(size > LOG_MAX_SIZE) {
            storage_file_close(f);
            /* Trim the file (keep last half) */
            File* f_read = storage_file_alloc(app->storage);
            if(storage_file_open(f_read, app->log_file_path, FSAM_READ, FSOM_OPEN_EXISTING)) {
                uint64_t new_size = size / 2;
                uint8_t* buf = malloc(new_size);
                if(buf) {
                    storage_file_seek(f_read, new_size, true);
                    uint64_t read = storage_file_read(f_read, buf, new_size);
                    storage_file_close(f_read);
                    storage_file_free(f_read);
                    /* Rewrite file with second half */
                    if(storage_file_open(f, app->log_file_path, FSAM_WRITE, FSOM_CREATE_ALWAYS)) {
                        storage_file_write(f, buf, read);
                        storage_file_close(f);
                    }
                    free(buf);
                } else {
                    storage_file_close(f_read);
                    storage_file_free(f_read);
                }
            } else {
                storage_file_free(f_read);
            }
            /* Reopen for append after trim */
            if(!storage_file_open(f, app->log_file_path, FSAM_WRITE, FSOM_OPEN_APPEND)) {
                storage_file_free(f);
                return;
            }
        }
        storage_file_write(f, msg, strlen(msg));
        storage_file_write(f, "\n", 1);
        storage_file_close(f);
    }
    storage_file_free(f);
}

/** Get SD log file size in bytes, or -1 if file doesn't exist */
static int64_t sd_log_get_size(FlipperMcpApp* app) {
    if(!app->storage) return -1;
    FileInfo file_info;
    if(storage_common_stat(app->storage, app->log_file_path, &file_info) == FSE_OK) {
        return file_info.size;
    }
    return -1;
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
    char file_content[768];
    snprintf(
        file_content, sizeof(file_content),
        "wifi_ssid=%s\nwifi_password=%s\nrelay_url=%s\nlog_to_sd=%d\nlog_level=%d\nlog_file_path=%s\n",
        app->ssid_buf,
        app->pass_buf,
        app->relay_buf,
        app->log_to_sd ? 1 : 0,
        app->log_level,
        app->log_file_path);
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
        else if(strncmp(p, "log_to_sd=", 10) == 0)
            app->log_to_sd = (p[10] == '1');
        else if(strncmp(p, "log_level=", 10) == 0)
            app->log_level = atoi(p + 10);
        else if(strncmp(p, "log_file_path=", 14) == 0)
            strncpy(app->log_file_path, p + 14, sizeof(app->log_file_path) - 1);

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

/* Forward declaration — menu_cb defined below, build_menu needs it. */
static void menu_cb(void* context, uint32_t index);

/** (Re-)build the main submenu. Called once at startup and again when
 *  the SD log toggle changes so the label reflects current state. */
static void build_menu(FlipperMcpApp* app) {
    submenu_reset(app->menu);
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
    submenu_add_item(app->menu, "SD Logging Settings", MenuSettings,  menu_cb, app);
    submenu_add_item(app->menu,
        app->log_to_sd ? "SD Log: ON" : "SD Log: OFF",
        MenuToggleSdLog, menu_cb, app);
}

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

    case MenuSettings: {
        int64_t log_size = sd_log_get_size(app);
        const char* log_level_name[] = {"Errors", "Normal", "Verbose"};
        const char* level_str = (app->log_level >= 0 && app->log_level <= 2) ?
            log_level_name[app->log_level] : "Unknown";

        snprintf(app->text_buf, TEXT_BUF_LEN,
            "SD Logging Settings\n\n"
            "Status: %s\n"
            "Level: %s (0=Err, 1=Norm, 2=Verb)\n"
            "Path: %s\n"
            "Size: %s\n\n"
            "To change:\n"
            "- Edit /ext/apps_data/flipper_mcp/config.txt\n"
            "- log_to_sd=0|1\n"
            "- log_level=0|1|2\n"
            "- log_file_path=/path/to/log\n\n"
            "To clear logs, remove\n"
            "the log file manually\n"
            "on the SD card.",
            app->log_to_sd ? "ON" : "OFF",
            level_str,
            app->log_file_path,
            (log_size >= 0) ? (log_size > 1024*1024 ? "large (>1MB)" : "OK") : "not found");

        strncpy(app->scroll_title, "Logging Config", sizeof(app->scroll_title) - 1);
        app->scroll_offset = 0;
        app->current_view = ViewIdScrollText;
        view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdScrollText);
        break;
    }

    case MenuToggleSdLog:
        app->log_to_sd = !app->log_to_sd;
        build_menu(app);  /* rebuild to update label */
        snprintf(app->result, RESULT_BUF_LEN,
            "SD logging %s", app->log_to_sd ? "enabled" : "disabled");
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
    app->uart_worker = furi_thread_alloc_ex("McpUartWorker", 8192, uart_worker_thread, app);
    furi_thread_start(app->uart_worker);

    FURI_LOG_I(TAG, "UART initialized at %d baud", UART_BAUD_RATE);
    /* Worker thread will send periodic PINGs until ESP32 replies with PONG */
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

    /* Initialize logging defaults */
    app->log_to_sd = true;  /* enabled by default */
    app->log_level = 1;     /* 0=errors, 1=normal, 2=verbose */
    strncpy(
        app->log_file_path,
        "/ext/apps_data/flipper_mcp/logs.txt",
        sizeof(app->log_file_path) - 1);

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
    build_menu(app);
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

    /* Clean up BLE HID if still active */
    if(app->ble_hid_profile) {
        ble_profile_hid_kb_release_all(app->ble_hid_profile);
        ble_profile_hid_mouse_release_all(app->ble_hid_profile);
        if(app->bt_held) {
            bt_profile_restore_default(app->bt_held);
            furi_record_close(RECORD_BT);
        }
        app->ble_hid_profile = NULL;
        app->bt_held = NULL;
    }
    /* Stop extra beacon if active */
    if(furi_hal_bt_extra_beacon_is_active()) {
        furi_hal_bt_extra_beacon_stop();
    }

    uart_cleanup(app);

    furi_record_close(RECORD_GUI);
    furi_record_close(RECORD_STORAGE);
    furi_record_close(RECORD_NOTIFICATION);
    free(app);

    return 0;
}
