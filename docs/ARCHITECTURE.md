# Architecture

## System Overview

```
┌────────────────────────────────────────────────────────────────┐
│  Local Network                                                 │
│                                                                │
│  MCP Client ──HTTP:8080──► ESP32-S2 ──UART 115200──► Flipper │
│                             (firmware)                        │
└────────────────────────────────────────────────────────────────┘

Remote access:

MCP Client ──HTTP──► Relay Server ◄──WebSocket── ESP32-S2 ──UART──► Flipper
```

The ESP32-S2 is the core: it runs an HTTP server that speaks MCP, translates tool calls into Flipper Zero CLI commands over UART, and returns structured results.

---

## Component Map

```
firmware/src/
├── main.rs               Entry point: init → WiFi → UART → MCP server → poll loop
│
├── wifi/
│   ├── station.rs        Connect to an existing WiFi network (STA mode)
│   ├── ap.rs             Create FlipperMCP-XXXX hotspot + captive portal (AP mode)
│   └── manager.rs        Boot logic: try STA → fall back to AP if no credentials
│
├── uart/
│   ├── transport.rs      Raw UART driver (GPIO 1/2, 115200 baud, framing)
│   ├── cli.rs            CLI text protocol: send "command\r\n", read until ">: "
│   └── protocol.rs       FlipperProtocol trait + factory
│
├── mcp/
│   ├── server.rs         JSON-RPC 2.0 dispatcher (initialize, tools/list, tools/call…)
│   ├── jsonrpc.rs        Request/response types, error codes
│   ├── types.rs          ToolDefinition, ToolResult, MCP schema types
│   ├── tools.rs          ToolRegistry → delegates to ModuleRegistry
│   └── transport/
│       ├── streamable.rs HTTP server: POST /mcp, GET /health
│       ├── sse.rs        Legacy SSE: GET /sse, POST /messages
│       └── manager.rs    HttpServerManager (start/stop/restart lifecycle)
│
├── modules/
│   ├── traits.rs         FlipperModule trait
│   ├── registry.rs       ModuleRegistry: static built-ins + dynamic (FAP/config)
│   ├── discovery.rs      FAP scanner: reads /ext/apps via UART → DynamicModule
│   ├── config.rs         TOML loader: reads modules.toml from Flipper SD card
│   └── builtin/          8 built-in modules (~32 tools)
│       ├── subghz.rs, nfc.rs, rfid.rs, infrared.rs
│       ├── gpio.rs, ibutton.rs, storage.rs, system.rs
│
├── tunnel/
│   ├── mdns.rs           mDNS advertisement (requires espressif/mdns component)
│   ├── client.rs         WebSocket tunnel client (requires espressif/esp_websocket_client)
│   └── mod.rs            Feature-gated wrappers: start_mdns_if_available,
│                         start_tunnel_if_available
│
└── config/
    ├── settings.rs       Settings struct + SD card config parser
    └── nvs.rs            NVS read/write (wifi_ssid, wifi_password, relay_url, …)

relay/src/
├── main.rs               CLI entry: axum server on --listen addr
├── tunnel.rs             GET /tunnel WebSocket: register device, route responses
└── proxy.rs              POST /mcp, GET /sse, POST /messages → forward to device
```

---

## Data Flow: MCP Tool Call

```
1. Claude sends HTTP POST /mcp to ESP32-S2:8080
   Body: {"jsonrpc":"2.0","id":1,"method":"tools/call",
          "params":{"name":"nfc_detect","arguments":{}}}

2. EspHttpServer handler in streamable.rs reads the body

3. McpServer::handle_request() parses JSON-RPC, dispatches to tools/call handler

4. ToolRegistry::call_tool("nfc_detect", {}) → finds NfcModule

5. NfcModule::execute() builds CLI command: "nfc detect"

6. CliProtocol::execute_command("nfc detect") sends via UART:
   ESP32-S2 GPIO1 (TX) → Flipper Zero RX → CLI executes "nfc detect"

7. Flipper responds with detection output, terminated by ">: " prompt

8. CliProtocol returns the parsed response string

9. NfcModule wraps output in ToolResult::success(json!)

10. success_response() wraps in JSON-RPC: {"jsonrpc":"2.0","id":1,"result":{...}}

11. HTTP handler writes response back to Claude
```

---

## WiFi Boot State Machine

```
Boot
 │
 ├─ wifi_ssid empty? ──Yes──► AP mode: FlipperMCP-XXXX hotspot
 │                              Captive portal at 192.168.4.1
 │                              User submits SSID/pass → save to NVS → esp_restart()
 │                              (loops back to Boot with credentials)
 │
 └─ wifi_ssid set ────────────► Try STA connection
                                  │
                            OK ───┴─── Err
                            │           │
                  Normal operation     Error propagates →
                  HTTP server on :8080   device restarts
                  mDNS advertisement    Use AP portal or
                  Tunnel (optional)     wifi-config.sh --erase
```

---

## Module System

### Static modules (built-in, always present)

Built at compile time in `modules/builtin/`. Each implements `FlipperModule`:
- `name()`, `description()` — metadata
- `tools() → Vec<ToolDefinition>` — JSON Schema for each tool
- `execute(tool, args, protocol) → ToolResult` — sends CLI command, parses output

### Dynamic modules (runtime, refreshable)

Two sources:
1. **FAP discovery** — `storage list /ext/apps` via UART finds `.fap` files → `app_launch_{name}` tools
2. **TOML config** — reads `/ext/apps_data/flipper_mcp/modules.toml` → parametric tools with `{param}` substitution

Refreshed on startup and via `modules/refresh` MCP method (no reflash needed).

---

## MCP Transport

### Streamable HTTP (MCP 2025-03-26 spec)
- `POST /mcp` — one JSON-RPC request per HTTP request, one JSON response (or 202 for notifications)
- `GET /mcp` — returns 405 (server-initiated notifications not implemented)

### Legacy SSE (pre-2025 MCP spec)
- `GET /sse` — SSE stream; first event is `event: endpoint\ndata: /messages?sessionId=xxx`
- `POST /messages?sessionId=xxx` — JSON-RPC request; response delivered on SSE stream

---

## Server Lifecycle Control

The ESP32 polls the Flipper's SD card every 5 seconds for a control file:
- Path: `/ext/apps_data/flipper_mcp/server.cmd`
- Content: `stop`, `start`, or `restart`
- After processing, the file is deleted

Create the file from the Flipper's file manager or via UART:
```
storage write /ext/apps_data/flipper_mcp/server.cmd
restart
```

---

## Remote Tunnel (Relay)

```
ESP32-S2           Relay Server              Claude
    │                    │                      │
    │──WS CONNECT───────►│                      │
    │  X-Device-Id: xxx  │                      │
    │                    │◄──POST /mcp──────────│
    │◄──WS Text──────────│  {"jsonrpc":"2.0"…}  │
    │  {request json}    │                      │
    │                    │                      │
    │──WS Text──────────►│                      │
    │  {response json}   │──200 {response}─────►│
    │                    │                      │
```

The relay matches responses to waiting HTTP handlers by JSON-RPC `id`.
Only one device per relay instance in the current implementation.
Timeout: 30 seconds per request.

---

## Binary Size (ESP32-S2, 4MB flash)

| Phase | Size |
|-------|------|
| Phase 1 (WiFi + UART) | ~1.4 MB |
| Phase 2 (+ MCP/HTTP) | ~1.6 MB |
| Phase 5 (current, all features) | ~1.7 MB estimated |

Flash budget: 4 MB. The firmware fits comfortably with significant room to grow.
