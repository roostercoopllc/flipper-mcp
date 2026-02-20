# MCP API Reference

## Protocol

flipper-mcp implements [MCP 2025-03-26](https://spec.modelcontextprotocol.io/) over HTTP.

**Base URL**: `http://<device-ip>:8080` or `http://flipper-mcp.local:8080`

## Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | `POST` | JSON-RPC 2.0 request |
| `/health` | `GET` | Health check |
| `/sse` | `GET` | Legacy SSE stream (pre-2025 clients) |
| `/messages` | `POST` | Legacy SSE messages |

---

## MCP Methods

### `initialize`
Capability negotiation. Always call this first.

```json
{"jsonrpc":"2.0","id":1,"method":"initialize",
 "params":{"protocolVersion":"2025-03-26","capabilities":{}}}
```

Response:
```json
{"result":{"protocolVersion":"2025-03-26",
  "capabilities":{"tools":{},"resources":{}},
  "serverInfo":{"name":"flipper-mcp","version":"0.1.0"}}}
```

### `tools/list`
Returns all available tools with schemas.

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

### `tools/call`
Execute a tool.

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"nfc_detect","arguments":{}}}
```

### `modules/refresh`
Reload dynamic modules (FAP discovery + TOML config) without rebooting.

```json
{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}
```

---

## Tools Reference

### SubGHz

| Tool | Arguments | Description |
|------|-----------|-------------|
| `subghz_tx` | `frequency` (int), `protocol` (str), `key` (str) | Transmit a signal |
| `subghz_rx` | `frequency` (int), `duration` (int, optional) | Receive and display signals |
| `subghz_rx_raw` | `frequency` (int), `output_path` (str) | Record raw capture to file |
| `subghz_decode_raw` | `file_path` (str) | Decode a raw capture file |
| `subghz_chat` | `message` (str), `frequency` (int, optional) | Send SubGHz chat message |
| `subghz_tx_from_file` | `file_path` (str) | Transmit from saved .sub file |

Example:
```json
{"name":"subghz_rx","arguments":{"frequency":433920000,"duration":5000}}
```

### NFC

| Tool | Arguments | Description |
|------|-----------|-------------|
| `nfc_detect` | — | Detect NFC tag in field |
| `nfc_read` | — | Read and dump NFC tag data |
| `nfc_emulate` | `file_path` (str) | Emulate from saved file |
| `nfc_field` | `enable` (bool) | Toggle NFC field |

### RFID

| Tool | Arguments | Description |
|------|-----------|-------------|
| `rfid_read` | — | Read LF RFID card |
| `rfid_emulate` | `file_path` (str) | Emulate from saved file |
| `rfid_write` | `file_path` (str) | Write card from saved file |

### Infrared

| Tool | Arguments | Description |
|------|-----------|-------------|
| `ir_tx` | `signal_name` (str), `file_path` (str, optional) | Transmit IR signal |

### GPIO

| Tool | Arguments | Description |
|------|-----------|-------------|
| `gpio_read` | `pin` (str) | Read pin voltage |
| `gpio_write` | `pin` (str), `value` (int: 0 or 1) | Set pin high/low |
| `gpio_set_mode` | `pin` (str), `mode` (str: input/output) | Set pin direction |

### BadUSB

| Tool | Arguments | Description |
|------|-----------|-------------|
| `badusb_run` | `script_path` (str) | Run a Ducky Script |
| `badusb_list` | `dir` (str, optional) | List available scripts |

### iButton

| Tool | Arguments | Description |
|------|-----------|-------------|
| `ibutton_read` | — | Read iButton key fob |
| `ibutton_emulate` | `file_path` (str) | Emulate from saved file |

### Storage

| Tool | Arguments | Description |
|------|-----------|-------------|
| `storage_list` | `path` (str) | List directory contents |
| `storage_read` | `path` (str) | Read file contents |
| `storage_write` | `path` (str), `data` (str) | Write file |
| `storage_remove` | `path` (str) | Delete file |
| `storage_stat` | `path` (str) | Get file info |

### System

| Tool | Arguments | Description |
|------|-----------|-------------|
| `system_info` | — | Get device info (firmware, hardware) |
| `system_power_info` | — | Battery status, charge level |
| `system_power_off` | — | Power off the Flipper |
| `system_reboot` | — | Reboot the Flipper |
| `system_ps` | — | List running processes |
| `system_free` | — | Memory usage |
| `system_uptime` | — | System uptime |

---

## Dynamic Tools

### FAP Launcher Tools

Format: `app_launch_{name}` where `{name}` is the `.fap` filename (lowercase, normalized).

Example: `/ext/apps/Games/BadApple.fap` → tool `app_launch_badapple`
```json
{"name":"app_launch_badapple","arguments":{}}
```

### TOML Config Tools

Tools defined in `/ext/apps_data/flipper_mcp/modules.toml` appear with their configured names and accept the defined parameters.

---

## Error Codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error — invalid JSON |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid params — missing or wrong type |
| -32603 | Internal error — UART failure, command timeout, etc. |
