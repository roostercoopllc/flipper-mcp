# Operations Guide

Complete reference for operating the Flipper MCP server. Covers every tool with copy-paste curl commands, FAP management, SD card logging, and common operational workflows.

> Replace `<IP>` with your device IP (e.g., `192.168.0.58`) or `flipper-mcp.local` in all examples.

---

## Table of Contents

- [Quick Health Check](#quick-health-check)
- [Server Lifecycle](#server-lifecycle)
- [System Tools](#system-tools)
- [BLE Tools](#ble-tools)
- [SubGHz Tools](#subghz-tools)
- [NFC Tools](#nfc-tools)
- [RFID Tools](#rfid-tools)
- [Infrared Tools](#infrared-tools)
- [GPIO Tools](#gpio-tools)
- [Storage Tools](#storage-tools)
- [iButton Tools](#ibutton-tools)
- [Dynamic Tools](#dynamic-tools)
- [FAP Management](#fap-management)
- [SD Card Logging](#sd-card-logging)
- [Operational Workflows](#operational-workflows)
- [Monitoring & Diagnostics](#monitoring--diagnostics)

---

## Quick Health Check

```bash
# Health endpoint (no JSON-RPC, plain GET)
curl http://<IP>:8080/health
# {"status":"ok","version":"0.1.0"}

# Initialize MCP session
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}'

# List all available tools
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | python3 -m json.tool

# OpenAPI spec (tool schemas for integration)
curl -s http://<IP>:8080/openapi.json | python3 -m json.tool
```

---

## Server Lifecycle

Server start/stop/restart is controlled from the **Flipper FAP** (Apps > Tools > Flipper MCP):

| FAP Menu Item | Effect |
|---------------|--------|
| **Start Server** | Starts the HTTP server on port 8080 |
| **Stop Server** | Stops the HTTP server (MCP requests will fail) |
| **Restart Server** | Stops then starts the HTTP server |
| **Reboot Board** | Full ESP32 reboot (WiFi reconnects, server restarts) |
| **Refresh Modules** | Reloads FAP discovery + TOML config without restart |

Refresh modules can also be triggered via MCP:
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}'
```

---

## System Tools

### Get device info
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_device_info","arguments":{}}}'
```
Returns hardware revision, firmware version, radio stack version, etc.

### Battery and power status
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_power_info","arguments":{}}}'
```
Returns battery voltage, current, temperature, and charge percentage.

### Memory usage
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_free","arguments":{}}}'
```

### Device uptime
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_uptime","arguments":{}}}'
```

### List running threads
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_ps","arguments":{}}}'
```

### Power off the Flipper
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_power_off","arguments":{}}}'
```
**Warning:** This powers off the Flipper Zero. The ESP32 WiFi board will lose its UART connection.

### Reboot the Flipper
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_power_reboot","arguments":{}}}'
```
**Warning:** This reboots the Flipper. You'll need to reopen the FAP after reboot.

---

## BLE Tools

### Scan for BLE devices
```bash
# Default 5-second scan
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_scan","arguments":{}}}'

# Custom duration (1-30 seconds)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_scan","arguments":{"duration":15}}}'
```

**Important:** BLE scanning temporarily disconnects the Flipper mobile app. The connection is restored after the scan.

**Current status:** The scan tool toggles the BT service but actual GAP device enumeration requires STM32WB BLE stack integration (pending). See [Troubleshooting > BLE Issues](TROUBLESHOOTING.md#ble-issues).

### Connect to a BLE device
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_connect","arguments":{"mac":"AA:BB:CC:DD:EE:FF"}}}'
```
**Status:** Pending implementation (requires GAP central role).

### Disconnect from BLE device
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_disconnect","arguments":{}}}'
```

### Discover GATT services
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_discover","arguments":{}}}'
```
Must be connected to a device first. Returns service UUIDs and characteristic handles.

### Read GATT characteristic
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_read","arguments":{"handle":42}}}'
```
The `handle` value comes from `ble_gatt_discover` output.

### Write GATT characteristic
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_write","arguments":{"handle":42,"data":"0102FF"}}}'
```
Data is hex-encoded (e.g., `"0102FF"` = bytes `[0x01, 0x02, 0xFF]`).

---

## SubGHz Tools

### Receive signals
```bash
# Listen on 433.92 MHz for 5 seconds
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx","arguments":{"frequency":433920000,"duration":5000}}}'

# Listen on 315 MHz for 10 seconds
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx","arguments":{"frequency":315000000,"duration":10000}}}'
```

### Transmit a signal
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_tx","arguments":{"frequency":433920000,"protocol":"Princeton","key":"000001"}}}'
```

### Raw capture to file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx_raw","arguments":{"frequency":433920000,"output_path":"/ext/subghz/captures/raw_capture.sub"}}}'
```

### Decode a raw capture
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_decode_raw","arguments":{"file_path":"/ext/subghz/captures/raw_capture.sub"}}}'
```

### Transmit from saved .sub file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_tx_from_file","arguments":{"file_path":"/ext/subghz/captures/signal.sub"}}}'
```

### SubGHz chat
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_chat","arguments":{"message":"Hello","frequency":433920000}}}'
```

---

## NFC Tools

### Detect NFC tag
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_detect","arguments":{}}}'
```
Hold an NFC tag near the Flipper's NFC antenna before calling.

### Emulate NFC tag
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_emulate","arguments":{"file_path":"/ext/nfc/tag.nfc"}}}'
```

### Toggle NFC field
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_field","arguments":{"enable":true}}}'
```

---

## RFID Tools

### Read RFID card
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rfid_read","arguments":{}}}'
```
Hold a 125kHz RFID card near the Flipper's RFID antenna.

### Emulate RFID tag
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rfid_emulate","arguments":{"type":"EM4100","data":"0102030405"}}}'
```

### Write RFID tag
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rfid_write","arguments":{"type":"EM4100","data":"0102030405"}}}'
```

---

## Infrared Tools

### Transmit IR signal
```bash
# NEC protocol example (common for TVs)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ir_tx","arguments":{"protocol":"NEC","address":"04","command":"08"}}}'

# Samsung protocol
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ir_tx","arguments":{"protocol":"Samsung","address":"07","command":"02"}}}'
```

---

## GPIO Tools

Available pins: `PA4`, `PA6`, `PA7`, `PB2`, `PB3`, `PC0`, `PC1`, `PC3`

### Read pin value
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_read","arguments":{"pin":"PA7"}}}'
```

### Set pin high/low
```bash
# Set PA7 high
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_set","arguments":{"pin":"PA7","value":1}}}'

# Set PA7 low
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_set","arguments":{"pin":"PA7","value":0}}}'
```

### Set pin mode
```bash
# Set as output
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_mode","arguments":{"pin":"PA7","mode":"1"}}}'

# Set as input
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_mode","arguments":{"pin":"PA7","mode":"0"}}}'
```

---

## Storage Tools

### List directory
```bash
# SD card root
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_list","arguments":{"path":"/ext"}}}'

# SubGHz captures
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_list","arguments":{"path":"/ext/subghz"}}}'

# Installed apps
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_list","arguments":{"path":"/ext/apps"}}}'
```

### Read file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_read","arguments":{"path":"/ext/apps_data/flipper_mcp/config.txt"}}}'
```

### Write file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_write","arguments":{"path":"/ext/test.txt","data":"Hello from MCP!"}}}'
```

### File info
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_stat","arguments":{"path":"/ext/apps_data/flipper_mcp/config.txt"}}}'
```

### Delete file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_remove","arguments":{"path":"/ext/test.txt"}}}'
```

---

## iButton Tools

### Read iButton
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ibutton_read","arguments":{}}}'
```
Touch an iButton key to the Flipper's 1-Wire contact.

### Emulate iButton
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ibutton_emulate","arguments":{"type":"DS1990","data":"0102030405060708"}}}'
```

---

## Dynamic Tools

### Refresh modules (discover new FAPs / reload TOML)
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}'
```

### Launch an installed FAP app
FAP apps on the SD card are auto-discovered as `app_launch_{name}` tools:
```bash
# Example: launch an app called "badapple"
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"app_launch_badapple","arguments":{}}}'
```

---

## FAP Management

The Flipper MCP FAP (Apps > Tools > Flipper MCP) provides on-device control:

| Menu Item | Description |
|-----------|-------------|
| **Status** | Shows IP, SSID, server state, version, heap free |
| **Start Server** | Starts the MCP HTTP server |
| **Stop Server** | Stops the MCP HTTP server |
| **Restart Server** | Stops then starts the server |
| **Reboot Board** | Full ESP32 reboot |
| **Configure WiFi** | On-screen keyboard for SSID + password + relay URL |
| **View Logs** | Scrollable log messages from ESP32 |
| **Tools List** | Shows all registered MCP tools |
| **Refresh Modules** | Reloads dynamic tool discovery |
| **Load SD Config** | Reads config.txt from SD and sends to ESP32 |
| **SD Log: ON/OFF** | Toggle persistent SD card logging |

---

## SD Card Logging

When enabled via the "SD Log" menu toggle, all `LOG|` messages from the ESP32 are appended to:

```
/ext/apps_data/flipper_mcp/mcp.log
```

### Configuration
- **Toggle:** FAP menu > "SD Log: ON/OFF"
- **Persisted:** The setting is saved in `config.txt` as `log_to_sd=1` when you use "Configure WiFi" or "Save Config"
- **Load on startup:** Use "Load SD Config" to restore the setting from `config.txt`

### Log file management
- **Max size:** 64 KB — when exceeded, the file is trimmed to the last 32 KB
- **Format:** One log line per line, same content as the in-memory log buffer
- **Location:** `/ext/apps_data/flipper_mcp/mcp.log`

### Reading logs via MCP
```bash
# Read the persistent log file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_read","arguments":{"path":"/ext/apps_data/flipper_mcp/mcp.log"}}}'

# Check log file size
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_stat","arguments":{"path":"/ext/apps_data/flipper_mcp/mcp.log"}}}'

# Clear the log file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_remove","arguments":{"path":"/ext/apps_data/flipper_mcp/mcp.log"}}}'
```

---

## Operational Workflows

### First-time setup
1. Flash ESP32 firmware (see [SETUP.md](SETUP.md))
2. Install FAP on Flipper (copy `flipper_mcp.fap` to `SD:/apps/Tools/`)
3. Open FAP: Apps > Tools > Flipper MCP
4. Select "Configure WiFi" — enter SSID, password, and optional relay URL
5. Select "Reboot Board"
6. Wait for Status to show an IP address
7. Test: `curl http://<IP>:8080/health`

### Capture and replay SubGHz signal
```bash
# 1. Listen for signal
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx_raw","arguments":{"frequency":433920000,"output_path":"/ext/subghz/captures/captured.sub"}}}'

# 2. Decode what was captured
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_decode_raw","arguments":{"file_path":"/ext/subghz/captures/captured.sub"}}}'

# 3. Replay the captured signal
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_tx_from_file","arguments":{"file_path":"/ext/subghz/captures/captured.sub"}}}'
```

### NFC tag clone workflow
```bash
# 1. Read the tag
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_detect","arguments":{}}}'

# 2. Emulate the tag
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_emulate","arguments":{"file_path":"/ext/nfc/captured_tag.nfc"}}}'
```

### GPIO blink example
```bash
# Set pin as output, then toggle
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_mode","arguments":{"pin":"PA7","mode":"1"}}}'

# Turn on
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_set","arguments":{"pin":"PA7","value":1}}}'

# Turn off
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_set","arguments":{"pin":"PA7","value":0}}}'
```

---

## Monitoring & Diagnostics

### Check server status
```bash
curl -s http://<IP>:8080/health | python3 -m json.tool
```

### View ESP32 free heap
Check the FAP Status screen — the `heap_free` field shows available RAM on the ESP32.

### Serial monitor (ESP32 console logs)
```bash
picocom -b 115200 /dev/ttyACM0
```
Shows ESP-IDF internal logs, WiFi driver output, MCP request processing, and tool execution results.

### Read persistent SD logs remotely
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_read","arguments":{"path":"/ext/apps_data/flipper_mcp/mcp.log"}}}' \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['content'][0]['text'])"
```

### Check config file
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_read","arguments":{"path":"/ext/apps_data/flipper_mcp/config.txt"}}}'
```
