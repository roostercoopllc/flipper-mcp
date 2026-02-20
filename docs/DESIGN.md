# Flipper Zero MCP Server — Implementation Plan

## Context

**Problem**: There's no way to control a Flipper Zero programmatically from an AI agent (Claude, etc.) when the Flipper is deployed on a target network. The existing `busse/flipperzero-mcp` project runs on a host computer via USB — we want the MCP server running **directly on the Flipper's WiFi Dev Board v1 (ESP32-S2)**, accessible over the network with no auth required.

**Solution**: A Rust MCP server on the ESP32-S2 that:
- Exposes all default Flipper apps as MCP tools over HTTP
- Supports dynamic module discovery (FAP apps + config-driven)
- Is reachable locally via mDNS and remotely via reverse WebSocket tunnel
- Includes a companion relay server binary for cross-network access

**Architecture**:
```
                    ┌─ Local Network ─────────────────────────────┐
                    │                                             │
Claude Client ──HTTP──► flipper-mcp.local:8080 (ESP32-S2) ──UART──► Flipper Zero
                    │                                             │
                    └─────────────────────────────────────────────┘

        OR (remote):

Claude Client ──HTTP──► Relay Server ◄──WebSocket── ESP32-S2 ──UART──► Flipper Zero
```

**Repo**: https://github.com/roostercoopllc/flipper-mcp

---

## Project Structure

```
flipper-mcp/
├── firmware/                          # ESP32-S2 firmware (main crate)
│   ├── Cargo.toml
│   ├── build.rs
│   ├── sdkconfig.defaults             # ESP-IDF configuration
│   ├── .cargo/
│   │   └── config.toml                # Xtensa target config
│   ├── cfg.toml                       # espflash board config
│   └── src/
│       ├── main.rs                    # Entry: init WiFi → UART → HTTP → MCP
│       ├── wifi/
│       │   ├── mod.rs
│       │   ├── station.rs             # STA mode (join existing network)
│       │   ├── ap.rs                  # AP mode (create hotspot + captive portal)
│       │   └── manager.rs             # Dual-mode: STA with AP fallback
│       ├── uart/
│       │   ├── mod.rs
│       │   ├── transport.rs           # Raw UART read/write with framing
│       │   ├── cli.rs                 # CLI text protocol implementation
│       │   ├── rpc.rs                 # Protobuf RPC (future, stub for now)
│       │   └── protocol.rs            # FlipperProtocol trait + factory
│       ├── mcp/
│       │   ├── mod.rs
│       │   ├── server.rs              # MCP server core (capability negotiation)
│       │   ├── jsonrpc.rs             # JSON-RPC 2.0 request/response handling
│       │   ├── transport/
│       │   │   ├── mod.rs
│       │   │   ├── streamable.rs      # Streamable HTTP (POST+SSE single endpoint)
│       │   │   └── sse.rs             # Legacy SSE (dual endpoint /sse + /messages)
│       │   ├── tools.rs               # Tool registry, dispatch, schema generation
│       │   └── types.rs               # MCP type definitions (Tool, Resource, etc.)
│       ├── modules/
│       │   ├── mod.rs
│       │   ├── registry.rs            # Central module registry
│       │   ├── discovery.rs           # FAP app scanner (via `storage list`)
│       │   ├── config.rs              # TOML config-driven module loader
│       │   ├── traits.rs              # FlipperModule trait definition
│       │   └── builtin/
│       │       ├── mod.rs             # Re-exports all built-in modules
│       │       ├── subghz.rs          # SubGHz: tx, rx, decode, chat
│       │       ├── nfc.rs             # NFC: detect, read, emulate
│       │       ├── rfid.rs            # RFID: read, emulate, write
│       │       ├── infrared.rs        # IR: tx, rx, learn
│       │       ├── gpio.rs            # GPIO: read, write, set_mode
│       │       ├── badusb.rs          # BadUSB: run script, generate
│       │       ├── ibutton.rs         # iButton: read, emulate
│       │       ├── storage.rs         # Storage: list, read, write, remove
│       │       └── system.rs          # System: info, reboot, power, ps, free
│       ├── tunnel/
│       │   ├── mod.rs
│       │   ├── client.rs             # WebSocket client → relay server
│       │   └── mdns.rs               # mDNS advertisement (flipper-mcp.local)
│       └── config/
│           ├── mod.rs
│           ├── settings.rs            # Runtime config struct
│           └── nvs.rs                 # NVS read/write for persisted settings
│
├── relay/                             # Companion relay server binary
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                    # CLI entry: listen for WS + serve HTTP
│       ├── tunnel.rs                  # WebSocket server, manage Flipper connections
│       └── proxy.rs                   # HTTP→WS proxy (MCP requests → Flipper)
│
├── config/
│   └── modules.example.toml           # Example module config template
│
├── scripts/
│   ├── setup-toolchain.sh             # Install espup, Rust Xtensa toolchain
│   ├── build.sh                       # Build firmware
│   ├── flash.sh                       # Flash to ESP32-S2 via espflash
│   ├── monitor.sh                     # Serial monitor (espflash monitor)
│   ├── build-relay.sh                 # Build relay server for host
│   └── wifi-config.sh                 # Set WiFi creds via espflash NVS
│
├── docs/
│   ├── ARCHITECTURE.md
│   ├── SETUP.md
│   ├── TROUBLESHOOTING.md
│   ├── MODULE_DEVELOPMENT.md
│   ├── API.md
│   ├── RELAY.md
│   └── HARDWARE.md
│
├── .github/
│   └── workflows/
│       └── ci.yml                     # Build check (firmware + relay)
│
├── Cargo.toml                         # Workspace root
├── AGENTS.md                          # AI agent context for project continuation
├── README.md
├── LICENSE
└── .gitignore
```

---

## Phase 1: Project Scaffolding & UART Communication

**Goal**: ESP32-S2 boots, connects to WiFi, and can send CLI commands to the Flipper Zero over UART and read responses.

### Files to create:
- Workspace `Cargo.toml`, `firmware/Cargo.toml`, `.cargo/config.toml`, `sdkconfig.defaults`, `build.rs`, `cfg.toml`
- `src/main.rs` — init logging, WiFi (STA only), UART, send `device_info` as smoke test
- `src/wifi/mod.rs`, `src/wifi/station.rs` — connect to hardcoded/NVS WiFi
- `src/uart/mod.rs`, `src/uart/transport.rs` — UART driver setup (115200 baud, GPIO pins)
- `src/uart/cli.rs` — send command string, read until prompt, parse response
- `src/uart/protocol.rs` — `FlipperProtocol` trait definition
- `src/config/mod.rs`, `src/config/nvs.rs`, `src/config/settings.rs`

### Key dependencies (firmware):
```toml
[dependencies]
esp-idf-svc = { version = "0.49", features = ["binstart"] }
esp-idf-hal = "0.44"
log = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
```

### Key details:
- UART TX/RX pins: GPIO 1 (TX) and GPIO 2 (RX) — match Flipper expansion header pinout
- CLI framing: send `command\r\n`, read until `>: ` prompt or timeout (500ms)
- `FlipperProtocol` trait with `execute_command`, `list_apps`, `launch_app`, `get_device_info`
- `CliProtocol` implements the trait by formatting text commands and parsing text output
- `RpcProtocol` is a stub returning `Err("not yet implemented")`

### Verification:
- Flash firmware, open serial monitor
- Observe: WiFi connects, UART sends `device_info`, response logged
- Run `power info` and `ps` to confirm bidirectional communication

---

## Phase 2: MCP Server Core + HTTP

**Goal**: HTTP server responds to MCP JSON-RPC 2.0 requests with tool listing and basic execution.

### Files to create:
- `src/mcp/mod.rs`, `src/mcp/server.rs`, `src/mcp/jsonrpc.rs`, `src/mcp/types.rs`
- `src/mcp/tools.rs` — tool registry
- `src/mcp/transport/mod.rs`, `src/mcp/transport/streamable.rs`

### Key details:
- **JSON-RPC 2.0** implementation (hand-rolled, ~200 lines):
  - Parse `{"jsonrpc":"2.0","id":1,"method":"...","params":{...}}`
  - Route to handlers: `initialize`, `tools/list`, `tools/call`, `resources/list`
  - Return `{"jsonrpc":"2.0","id":1,"result":{...}}` or `error`
- **Streamable HTTP transport**:
  - Single endpoint `POST /mcp` — accepts JSON-RPC, returns JSON response
  - `GET /mcp` — SSE stream for server-initiated notifications (optional)
  - Content-Type negotiation per MCP spec
- **EspHttpServer** setup:
  - Register handlers for `POST /mcp`, `GET /mcp`, `GET /health`
  - Parse request body, dispatch to MCP server, serialize response
- **Capability negotiation** (`initialize`):
  - Server declares: `tools`, `resources` capabilities
  - Returns server name, version, protocol version
- **Tool registry**: `HashMap<String, Box<dyn Fn(Value) -> Result<Value>>>`
  - Register a single test tool `system_info` to validate the pipeline

### Verification:
- Flash, connect to ESP32's IP
- `curl -X POST http://<ip>:8080/mcp -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}'`
- Get back capability response
- Call `tools/list`, see `system_info` tool
- Call `tools/call` with `system_info`, get Flipper device info back

---

## Phase 3: Built-in Modules + Module Framework

**Goal**: All default Flipper apps are exposed as MCP tools via the module system.

### Files to create:
- `src/modules/mod.rs`, `src/modules/traits.rs`, `src/modules/registry.rs`
- All `src/modules/builtin/*.rs` files (subghz, nfc, rfid, infrared, gpio, badusb, ibutton, storage, system)

### Key details:
- **`FlipperModule` trait**:
  ```rust
  pub trait FlipperModule: Send + Sync {
      fn name(&self) -> &str;
      fn description(&self) -> &str;
      fn tools(&self) -> Vec<ToolDefinition>;
      fn execute(&self, tool: &str, params: &Value, proto: &mut dyn FlipperProtocol) -> Result<Value>;
  }
  ```
- **`ModuleRegistry`**: holds `Vec<Box<dyn FlipperModule>>`, provides:
  - `list_all_tools()` — aggregates tools from all modules
  - `dispatch(tool_name, params)` — finds owning module, calls execute
- **Built-in module example** (`SubGhzModule`):
  - Tools: `subghz_tx` (frequency, protocol, key), `subghz_rx` (frequency, duration), `subghz_decode_raw` (file_path), `subghz_chat` (message)
  - Each tool maps to CLI command: e.g., `subghz tx <protocol> <key> <freq>`
  - Returns parsed output as structured JSON
- **Tool definitions** include JSON Schema for parameters (MCP spec requirement)
- **All built-in modules**: ~30 tools total across 9 modules

### Verification:
- `tools/list` returns all ~30 tools with schemas
- `tools/call` with `nfc_detect` sends `nfc detect` over UART, returns parsed result
- `tools/call` with `storage_list` path="/ext/apps" returns directory listing
- Test each module category with at least one tool call

---

## Phase 4: Dynamic Module Discovery

**Goal**: New FAP apps and config-defined tools are auto-discovered and exposed as MCP tools.

### Files to create:
- `src/modules/discovery.rs` — FAP scanner
- `src/modules/config.rs` — TOML config loader
- `config/modules.example.toml`

### Key details:
- **FAP Discovery** (`discovery.rs`):
  - Runs `storage list /ext/apps` and subfolders via UART
  - Parses output to find `.fap` files
  - Creates generic tool for each: `launch_app_{name}` with args parameter
  - Re-scans on `modules/refresh` MCP method or periodic timer (every 60s)
- **Config-driven modules** (`config.rs`):
  - TOML file stored in ESP32 NVS or read from Flipper SD card
  - Format:
    ```toml
    [[module]]
    name = "custom_scanner"
    description = "Custom frequency scanner"
    [[module.tool]]
    name = "scan_range"
    description = "Scan frequency range"
    command_template = "subghz rx {frequency}"
    params = [
      { name = "frequency", type = "number", required = true, description = "Frequency in Hz" }
    ]
    ```
  - Config modules parsed at startup and on refresh
- **Unified registry**: built-in + FAP-discovered + config-driven all live in same registry

### Verification:
- Install a FAP on Flipper SD card
- Call `tools/list` — new FAP appears as launchable tool
- Edit config TOML, call `modules/refresh` — new tools appear
- Call a config-defined tool, verify correct CLI command is sent

---

## Phase 5: Legacy SSE Transport + WiFi AP Mode

**Goal**: Full MCP transport compatibility and zero-config WiFi setup.

### Files to create:
- `src/mcp/transport/sse.rs` — Legacy SSE dual-endpoint transport
- `src/wifi/ap.rs` — AP mode with captive portal
- `src/wifi/manager.rs` — dual-mode manager

### Key details:
- **Legacy SSE transport**:
  - `GET /sse` — SSE connection, sends endpoint URI in first event
  - `POST /messages?sessionId=xxx` — JSON-RPC requests
  - Session management with timeout
- **AP mode**:
  - SSID: `FlipperMCP-XXXX` (last 4 of MAC)
  - No password (open network for easy config)
  - DNS redirect all domains to captive portal
  - Simple HTML page: enter SSID + password, save to NVS, reboot into STA
- **WiFi manager**:
  - Boot: try STA with NVS credentials (10s timeout)
  - If STA fails: start AP mode
  - If STA connects: start mDNS + HTTP server
  - Periodic connectivity check, re-enter AP if disconnected for >60s

### Verification:
- Boot with no WiFi creds — AP mode activates, connect phone, configure WiFi
- Reboot — STA mode connects
- Legacy SSE client connects and exchanges MCP messages
- Streamable HTTP still works simultaneously

---

## Phase 6: Reverse WebSocket Tunnel + mDNS

**Goal**: Flipper is accessible remotely via relay, and locally via `flipper-mcp.local`.

### Files to create:
- `src/tunnel/mod.rs`, `src/tunnel/client.rs`, `src/tunnel/mdns.rs`
- `relay/Cargo.toml`, `relay/src/main.rs`, `relay/src/tunnel.rs`, `relay/src/proxy.rs`

### Key details:
- **mDNS** (`mdns.rs`):
  - Advertise `_mcp._tcp` service via `EspMdns` from esp-idf-svc
  - Hostname: configurable, default `flipper-mcp`
  - Clients discover via `flipper-mcp.local:8080`
- **WebSocket tunnel client** (`client.rs`):
  - Connects to relay URL from config (e.g., `ws://relay.example.com:9090/tunnel`)
  - Sends device ID in handshake headers
  - Receives MCP JSON-RPC requests over WS, forwards to local MCP server
  - Sends responses back over WS
  - Auto-reconnect with exponential backoff (5s, 10s, 30s, 60s max)
- **Relay server** (`relay/`):
  - Dependencies: `tokio`, `axum`, `tokio-tungstenite`, `clap`
  - `GET /tunnel` — WebSocket endpoint, Flipper connects here
  - `POST /mcp` and `GET /mcp` — MCP endpoints, proxied to connected Flipper
  - `GET /sse` + `POST /messages` — Legacy SSE, proxied
  - `GET /health` — relay status + connected device info
  - Supports multiple Flippers: route by device ID header or path prefix
  - CLI: `flipper-mcp-relay --listen 0.0.0.0:9090`

### Verification:
- Start relay on laptop: `flipper-mcp-relay --listen 0.0.0.0:9090`
- Flash Flipper with relay URL configured
- From different machine: `curl http://laptop-ip:9090/mcp` reaches Flipper
- mDNS: `ping flipper-mcp.local` resolves on same LAN
- Disconnect WiFi, reconnect — tunnel auto-reconnects

---

## Phase 7: Documentation, Scripts & Polish

**Goal**: Complete docs, helper scripts, CI, and production hardening.

### Files to create:
- All `docs/*.md` files
- All `scripts/*.sh` files
- `README.md`, `.gitignore`, `LICENSE`, `.github/workflows/ci.yml`

### Documentation outline:
- **README.md**: Project overview, architecture diagram, 5-minute quickstart, feature list
- **SETUP.md**: Prerequisites, toolchain install, building, flashing, first connection
- **ARCHITECTURE.md**: Component diagram, data flow, module system, protocol details
- **API.md**: Every MCP tool with name, description, parameters, example request/response
- **MODULE_DEVELOPMENT.md**: How to write config modules, how to add built-in modules
- **RELAY.md**: Relay setup (local, VPS, Docker), multi-Flipper routing, security
- **TROUBLESHOOTING.md**: WiFi issues, UART issues, flash issues, MCP client config, LED codes
- **HARDWARE.md**: Pin connections, board variants, power considerations

### Scripts:
- `setup-toolchain.sh` — install `espup`, Rust toolchain, `espflash`, `ldproxy`
- `build.sh` — `cargo build --release --target xtensa-esp32s2-espidf`
- `flash.sh` — `espflash flash --monitor target/.../flipper-mcp`
- `monitor.sh` — `espflash monitor`
- `build-relay.sh` — `cargo build --release -p flipper-mcp-relay`
- `wifi-config.sh` — write WiFi SSID/password to NVS partition

### CI:
- GitHub Actions: check firmware compiles (xtensa target), check relay compiles (x86_64)
- Lint with clippy, format check with rustfmt

### Verification:
- Fresh clone on new machine, follow SETUP.md, working system
- All scripts run without errors
- CI passes on push
- Claude Desktop can connect to relay and list/call tools

---

## Implementation Order Summary

| Phase | Deliverable | Key Risk |
|-------|------------|----------|
| 1 | UART + WiFi STA boot | Pin mapping, UART framing timing |
| 2 | MCP HTTP server + JSON-RPC | Memory usage of HTTP + JSON parsing |
| 3 | All built-in modules (~30 tools) | CLI output parsing edge cases |
| 4 | FAP discovery + config modules | Storage list parsing, TOML in NVS |
| 5 | SSE transport + AP captive portal | DNS redirect on ESP32, session mgmt |
| 6 | WebSocket tunnel + mDNS + relay | WS client stability, relay routing |
| 7 | Docs, scripts, CI | Keeping docs in sync with code |

---

## Key Dependencies

**Firmware (`firmware/Cargo.toml`)**:
- `esp-idf-svc` ~0.49 (WiFi, HTTP, mDNS, NVS)
- `esp-idf-hal` ~0.44 (UART, GPIO)
- `serde` + `serde_json` (JSON handling)
- `toml` (config parsing)
- `anyhow` (error handling)
- `log` (logging via esp-idf-svc logger)

**Relay (`relay/Cargo.toml`)**:
- `tokio` (async runtime)
- `axum` (HTTP framework)
- `tokio-tungstenite` (WebSocket)
- `clap` (CLI args)
- `tracing` (logging)

## Key Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| 4MB flash too small for firmware | Use `opt-level = "s"`, strip debug info, feature-gate optional modules |
| 2MB RAM pressure from HTTP + JSON | Stream responses, limit max request size (16KB), reuse buffers |
| UART timing/framing issues | Configurable timeouts, prompt detection with fallback, retry logic |
| ESP-IDF WebSocket client maturity | Fall back to raw TCP + manual WS framing if needed |
| Cross-compilation complexity | Detailed setup docs, Docker build option, CI validates |
