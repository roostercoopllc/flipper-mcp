/**
 * c2_protocol.h — Shared C2 binary protocol for SubGHz Flipper-to-Flipper communication.
 *
 * Frame format (raw bytes over SubGhzTxRxWorker):
 *   MAGIC(1) | FLAGS(1) | SEQ(1) | CMD(1) | LEN(1) | PAYLOAD(0-250) | CHECKSUM(1)
 *
 * This header is shared between:
 *   - flipper-app/flipper_mcp.c  (C2 controller side)
 *   - flipper-c2-client/c2_client.c  (C2 client side)
 *
 * For authorized security research only.
 */

#pragma once

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <string.h>

/* ---------- Constants ---------------------------------------------------- */

#define C2_MAGIC          0xC2
#define C2_HEADER_SIZE    5    /* MAGIC + FLAGS + SEQ + CMD + LEN */
#define C2_CHECKSUM_SIZE  1
#define C2_MAX_PAYLOAD    250
#define C2_MAX_FRAME_SIZE (C2_HEADER_SIZE + C2_MAX_PAYLOAD + C2_CHECKSUM_SIZE)
#define C2_MIN_FRAME_SIZE (C2_HEADER_SIZE + C2_CHECKSUM_SIZE) /* empty payload */

/* Default SubGHz frequency: 433.92 MHz */
#define C2_FREQ_DEFAULT   433920000UL

/* ACK/retry parameters */
#define C2_ACK_TIMEOUT_MS   500
#define C2_MAX_RETRIES      3

/* ---------- Flag bits ---------------------------------------------------- */

#define C2_FLAG_ACK_REQ   0x01  /* Sender wants an ACK back */
#define C2_FLAG_MULTI     0x02  /* Multi-frame message (not the last fragment) */
#define C2_FLAG_LAST      0x04  /* Last fragment of a multi-frame message */

/* ---------- Command types ------------------------------------------------ */

/* Protocol management */
#define C2_CMD_PING             0x01  /* C2 → Client: are you there? */
#define C2_CMD_PONG             0x02  /* Client → C2: yes, I'm here */
#define C2_CMD_ACK              0x03  /* Both: acknowledge receipt of SEQ */
#define C2_CMD_NACK             0x04  /* Both: negative ack (SEQ + error code) */

/* BLE HID commands */
#define C2_CMD_BLE_HID_START    0x10  /* C2 → Client: start HID profile (payload: device name) */
#define C2_CMD_BLE_HID_TYPE     0x11  /* C2 → Client: type text (payload: ASCII bytes) */
#define C2_CMD_BLE_HID_PRESS    0x12  /* C2 → Client: press key combo (payload: combo string) */
#define C2_CMD_BLE_HID_MOUSE    0x13  /* C2 → Client: mouse action (payload: dx,dy,btn,act,scroll) */
#define C2_CMD_BLE_HID_STOP     0x14  /* C2 → Client: stop HID profile */

/* BLE Beacon commands */
#define C2_CMD_BLE_BEACON_START 0x20  /* C2 → Client: start beacon (payload: config) */
#define C2_CMD_BLE_BEACON_STOP  0x21  /* C2 → Client: stop beacon */

/* NFC commands */
#define C2_CMD_NFC_READ         0x50  /* C2 → Client: read ISO 14443-3A tag UID */

/* Response commands */
#define C2_CMD_RESULT           0x30  /* Client → C2: success result text */
#define C2_CMD_ERROR            0x31  /* Client → C2: error text */
#define C2_CMD_STATUS           0x40  /* Client → C2: status info */

/* ---------- Mouse action payload format ---------------------------------- */

/* BLE_HID_MOUSE payload: 5 bytes total */
#define C2_MOUSE_OFFSET_DX      0
#define C2_MOUSE_OFFSET_DY      1
#define C2_MOUSE_OFFSET_BUTTON  2  /* 0=none, 1=left, 2=right, 3=middle */
#define C2_MOUSE_OFFSET_ACTION  3  /* 0=click, 1=press, 2=release */
#define C2_MOUSE_OFFSET_SCROLL  4
#define C2_MOUSE_PAYLOAD_SIZE   5

/* ---------- Beacon start payload format ---------------------------------- */

/* BLE_BEACON_START payload layout:
 *   [0]         adv_data_len (1-31)
 *   [1..N]      adv_data bytes
 *   [N+1..N+6]  MAC address (6 bytes, 00:00:00:00:00:00 = random)
 *   [N+7..N+8]  interval_ms (uint16_t big-endian)
 */

/* ---------- NACK error codes --------------------------------------------- */

#define C2_ERR_UNKNOWN_CMD    0x01
#define C2_ERR_INVALID_PARAM  0x02
#define C2_ERR_BLE_FAILED     0x03
#define C2_ERR_RADIO_BUSY     0x04
#define C2_ERR_TIMEOUT        0x05

/* ---------- Inline helper functions -------------------------------------- */

/**
 * Compute XOR checksum over a buffer.
 */
static inline uint8_t c2_checksum(const uint8_t* buf, size_t len) {
    uint8_t cs = 0;
    for(size_t i = 0; i < len; i++) {
        cs ^= buf[i];
    }
    return cs;
}

/**
 * Build a C2 frame into buf.
 *
 * @param buf       Output buffer (must be at least C2_HEADER_SIZE + payload_len + 1)
 * @param cmd       Command type (C2_CMD_*)
 * @param seq       Sequence number
 * @param flags     Flag bits (C2_FLAG_*)
 * @param payload   Payload bytes (may be NULL if payload_len == 0)
 * @param payload_len  Payload length (0-250)
 * @return          Total frame length, or 0 on error
 */
static inline size_t c2_build_frame(
    uint8_t* buf,
    uint8_t cmd,
    uint8_t seq,
    uint8_t flags,
    const uint8_t* payload,
    uint8_t payload_len) {
    if(payload_len > C2_MAX_PAYLOAD) return 0;

    buf[0] = C2_MAGIC;
    buf[1] = flags;
    buf[2] = seq;
    buf[3] = cmd;
    buf[4] = payload_len;

    if(payload_len > 0 && payload != NULL) {
        memcpy(buf + C2_HEADER_SIZE, payload, payload_len);
    }

    size_t frame_len = C2_HEADER_SIZE + payload_len;
    buf[frame_len] = c2_checksum(buf, frame_len);

    return frame_len + C2_CHECKSUM_SIZE;
}

/**
 * Parse and validate a C2 frame.
 *
 * @param buf           Input buffer
 * @param buf_len       Input buffer length
 * @param out_cmd       Output: command type
 * @param out_seq       Output: sequence number
 * @param out_flags     Output: flag bits
 * @param out_payload   Output: pointer to payload within buf (not a copy)
 * @param out_payload_len Output: payload length
 * @return              true if valid frame, false otherwise
 */
static inline bool c2_parse_frame(
    const uint8_t* buf,
    size_t buf_len,
    uint8_t* out_cmd,
    uint8_t* out_seq,
    uint8_t* out_flags,
    const uint8_t** out_payload,
    uint8_t* out_payload_len) {
    /* Minimum frame check */
    if(buf_len < C2_MIN_FRAME_SIZE) return false;

    /* Magic byte check */
    if(buf[0] != C2_MAGIC) return false;

    uint8_t payload_len = buf[4];
    size_t expected_len = C2_HEADER_SIZE + payload_len + C2_CHECKSUM_SIZE;

    /* Length check */
    if(buf_len < expected_len) return false;
    if(payload_len > C2_MAX_PAYLOAD) return false;

    /* Checksum check */
    uint8_t expected_cs = c2_checksum(buf, C2_HEADER_SIZE + payload_len);
    if(buf[C2_HEADER_SIZE + payload_len] != expected_cs) return false;

    /* Extract fields */
    *out_flags = buf[1];
    *out_seq = buf[2];
    *out_cmd = buf[3];
    *out_payload_len = payload_len;
    *out_payload = (payload_len > 0) ? (buf + C2_HEADER_SIZE) : NULL;

    return true;
}

/**
 * Build an ACK frame for a given sequence number.
 */
static inline size_t c2_build_ack(uint8_t* buf, uint8_t acked_seq) {
    uint8_t payload = acked_seq;
    return c2_build_frame(buf, C2_CMD_ACK, 0, 0, &payload, 1);
}

/**
 * Build a NACK frame for a given sequence number with error code.
 */
static inline size_t c2_build_nack(uint8_t* buf, uint8_t nacked_seq, uint8_t error_code) {
    uint8_t payload[2] = {nacked_seq, error_code};
    return c2_build_frame(buf, C2_CMD_NACK, 0, 0, payload, 2);
}

/**
 * Get a human-readable name for a command type (for logging).
 */
static inline const char* c2_cmd_name(uint8_t cmd) {
    switch(cmd) {
    case C2_CMD_PING:             return "PING";
    case C2_CMD_PONG:             return "PONG";
    case C2_CMD_ACK:              return "ACK";
    case C2_CMD_NACK:             return "NACK";
    case C2_CMD_BLE_HID_START:    return "BLE_HID_START";
    case C2_CMD_BLE_HID_TYPE:     return "BLE_HID_TYPE";
    case C2_CMD_BLE_HID_PRESS:    return "BLE_HID_PRESS";
    case C2_CMD_BLE_HID_MOUSE:    return "BLE_HID_MOUSE";
    case C2_CMD_BLE_HID_STOP:     return "BLE_HID_STOP";
    case C2_CMD_BLE_BEACON_START: return "BLE_BEACON_START";
    case C2_CMD_BLE_BEACON_STOP:  return "BLE_BEACON_STOP";
    case C2_CMD_NFC_READ:         return "NFC_READ";
    case C2_CMD_RESULT:           return "RESULT";
    case C2_CMD_ERROR:            return "ERROR";
    case C2_CMD_STATUS:           return "STATUS";
    default:                      return "UNKNOWN";
    }
}
