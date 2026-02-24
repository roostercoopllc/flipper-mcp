# MCP API Reference

## Protocol

flipper-mcp implements [MCP 2025-03-26](https://spec.modelcontextprotocol.io/) over HTTP.

**Base URL**: `http://<device-ip>:8080` or `http://flipper-mcp.local:8080`

## Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | `POST` | JSON-RPC 2.0 request (Streamable HTTP) |
| `/mcp` | `OPTIONS` | CORS preflight |
| `/health` | `GET` | Health check |
| `/openapi.json` | `GET` | OpenAPI 3.1 spec with tool definitions |
| `/sse` | `GET` | Legacy SSE stream (pre-2025 clients) |
| `/messages` | `POST` | Legacy SSE messages |

---

## MCP Methods

### `initialize`
Capability negotiation. Always call this first.

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}'
```

Response:
```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26",
  "capabilities":{"tools":{},"resources":{}},
  "serverInfo":{"name":"flipper-mcp","version":"0.1.0"}}}
```

### `tools/list`
Returns all available tools with JSON schemas.

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

### `tools/call`
Execute a tool.

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"TOOL_NAME","arguments":{}}}'
```

### `resources/list`
List available resources (currently returns empty list).

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}'
```

### `modules/refresh`
Reload dynamic modules (FAP discovery + TOML config) without rebooting.

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}'
```

---

## Tools Reference

All tools are called via `tools/call` with JSON-RPC. Replace `<IP>` with your device IP or `flipper-mcp.local`.

### System

| Tool | Arguments | Description |
|------|-----------|-------------|
| `system_device_info` | — | Get hardware/firmware info |
| `system_power_info` | — | Battery and power supply status |
| `system_power_off` | — | Power off the Flipper Zero |
| `system_power_reboot` | — | Reboot the Flipper Zero |
| `system_ps` | — | List running processes/threads |
| `system_free` | — | Show memory usage (heap free/total) |
| `system_uptime` | — | Show device uptime |

```bash
# Get device info
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_device_info","arguments":{}}}'

# Battery status
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_power_info","arguments":{}}}'

# Memory usage
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_free","arguments":{}}}'

# Uptime
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"system_uptime","arguments":{}}}'
```

### SubGHz

| Tool | Arguments | Description |
|------|-----------|-------------|
| `subghz_tx` | `frequency` (int), `protocol` (str), `key` (str) | Transmit a signal |
| `subghz_rx` | `frequency` (int), `duration` (int, optional) | Receive and display signals |
| `subghz_rx_raw` | `frequency` (int), `output_path` (str) | Record raw capture to file |
| `subghz_decode_raw` | `file_path` (str) | Decode a raw capture file |
| `subghz_chat` | `message` (str), `frequency` (int, optional) | Send SubGHz chat message |
| `subghz_tx_from_file` | `file_path` (str) | Transmit from saved .sub file |

```bash
# Receive on 433.92 MHz for 5 seconds
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx","arguments":{"frequency":433920000,"duration":5000}}}'

# Transmit a signal
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_tx","arguments":{"frequency":433920000,"protocol":"Princeton","key":"000001"}}}'

# Transmit from .sub file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_tx_from_file","arguments":{"file_path":"/ext/subghz/captures/signal.sub"}}}'
```

### NFC

| Tool | Arguments | Description |
|------|-----------|-------------|
| `nfc_detect` | — | Detect NFC tag in field |
| `nfc_emulate` | `file_path` (str) | Emulate from saved file |
| `nfc_field` | `enable` (bool) | Toggle NFC field |

```bash
# Detect NFC tag
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_detect","arguments":{}}}'

# Emulate NFC tag from file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_emulate","arguments":{"file_path":"/ext/nfc/tag.nfc"}}}'
```

### RFID

| Tool | Arguments | Description |
|------|-----------|-------------|
| `rfid_read` | — | Read LF RFID card |
| `rfid_emulate` | `type` (str), `data` (str) | Emulate RFID tag |
| `rfid_write` | `type` (str), `data` (str) | Write RFID tag |

```bash
# Read RFID card
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rfid_read","arguments":{}}}'
```

### Infrared

| Tool | Arguments | Description |
|------|-----------|-------------|
| `ir_tx` | `protocol` (str), `address` (str), `command` (str) | Transmit IR signal |

```bash
# Send IR command (NEC protocol)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ir_tx","arguments":{"protocol":"NEC","address":"04","command":"08"}}}'
```

### GPIO

| Tool | Arguments | Description |
|------|-----------|-------------|
| `gpio_set` | `pin` (str), `value` (int: 0 or 1) | Set pin high/low |
| `gpio_read` | `pin` (str) | Read pin value |
| `gpio_mode` | `pin` (str), `mode` (str) | Set pin direction |

Available pins: `PA4`, `PA6`, `PA7`, `PB2`, `PB3`, `PC0`, `PC1`, `PC3`

```bash
# Read pin value
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_read","arguments":{"pin":"PA7"}}}'

# Set pin high
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_set","arguments":{"pin":"PA7","value":1}}}'

# Set pin as output
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"gpio_mode","arguments":{"pin":"PA7","mode":"1"}}}'
```

### Storage

| Tool | Arguments | Description |
|------|-----------|-------------|
| `storage_list` | `path` (str) | List directory contents |
| `storage_read` | `path` (str) | Read file contents |
| `storage_write` | `path` (str), `data` (str) | Write file |
| `storage_remove` | `path` (str) | Delete file or directory |
| `storage_stat` | `path` (str) | Get file info (size, type) |

```bash
# List SD card root
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_list","arguments":{"path":"/ext"}}}'

# Read a file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_read","arguments":{"path":"/ext/apps_data/flipper_mcp/config.txt"}}}'

# Write a file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_write","arguments":{"path":"/ext/test.txt","data":"Hello from MCP!"}}}'

# Get file info
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_stat","arguments":{"path":"/ext/apps_data/flipper_mcp/config.txt"}}}'

# Delete a file
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"storage_remove","arguments":{"path":"/ext/test.txt"}}}'
```

### iButton

| Tool | Arguments | Description |
|------|-----------|-------------|
| `ibutton_read` | — | Read iButton key fob |
| `ibutton_emulate` | `type` (str), `data` (str) | Emulate iButton key |

```bash
# Read iButton
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ibutton_read","arguments":{}}}'
```

### BLE (Bluetooth Low Energy)

| Tool | Arguments | Description |
|------|-----------|-------------|
| `ble_scan` | `duration` (int, optional, 1-30, default 5) | Scan for nearby BLE devices |
| `ble_connect` | `mac` (str) | Connect to BLE device by MAC address |
| `ble_disconnect` | — | Disconnect current BLE connection |
| `ble_gatt_discover` | — | Discover GATT services/characteristics |
| `ble_gatt_read` | `handle` (int) | Read a GATT characteristic value |
| `ble_gatt_write` | `handle` (int), `data` (str, hex) | Write to a GATT characteristic |

> **Note:** BLE scanning temporarily disconnects the Flipper mobile app. The connection is restored after the scan completes. BLE connect/GATT operations require STM32WB BLE central role (pending full implementation).

```bash
# Scan for BLE devices (5 seconds default)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_scan","arguments":{"duration":5}}}'

# Scan for 15 seconds
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_scan","arguments":{"duration":15}}}'

# Connect to a BLE device
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_connect","arguments":{"mac":"AA:BB:CC:DD:EE:FF"}}}'

# Disconnect from BLE device
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_disconnect","arguments":{}}}'

# Discover GATT services (must be connected first)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_discover","arguments":{}}}'

# Read a GATT characteristic
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_read","arguments":{"handle":42}}}'

# Write to a GATT characteristic (hex data)
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_gatt_write","arguments":{"handle":42,"data":"0102FF"}}}'
```

---

## Dynamic Tools

### FAP Launcher Tools

Format: `app_launch_{name}` where `{name}` is the `.fap` filename (lowercase, normalized).

Example: `/ext/apps/Games/BadApple.fap` -> tool `app_launch_badapple`
```bash
curl -s -X POST http://<IP>:8080/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"app_launch_badapple","arguments":{}}}'
```

### TOML Config Tools

Tools defined in `/ext/apps_data/flipper_mcp/modules.toml` appear with their configured names and accept the defined parameters.

---

## Error Codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error — invalid JSON |
| -32600 | Invalid request — body too large |
| -32601 | Method not found |
| -32602 | Invalid params — missing or wrong type |
| -32603 | Internal error — UART failure, command timeout, etc. |

---

## CORS

All endpoints support CORS with `Access-Control-Allow-Origin: *`. Preflight `OPTIONS` requests are handled for `/mcp` and `/openapi.json`.
