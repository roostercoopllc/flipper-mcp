# AGENTS.md — Flipper MCP Server

This file provides all context an AI agent (or human developer) needs to pick up this project and continue implementation from any machine.

---

## 1. Project Overview

**flipper-mcp** is a Rust-based MCP (Model Context Protocol) server that runs directly on the Flipper Zero's WiFi Dev Board v1 (ESP32-S2). It exposes all default Flipper Zero applications as MCP tools over HTTP, allowing AI agents (Claude, etc.) to programmatically control a Flipper Zero over the network.

This is distinct from existing projects (e.g., `busse/flipperzero-mcp`) which run on a host computer. This server runs **on the hardware itself**.

**Repo**: https://github.com/roostercoopllc/flipper-mcp

---

## 2. Architecture

```
LOCAL ACCESS:
┌─ Local Network ─────────────────────────────────────────┐
│                                                         │
│  Claude/MCP Client ──HTTP──► flipper-mcp.local:8080     │
│                              (ESP32-S2 WiFi Board)      │
│                                    │                    │
│                                  UART                   │
│                                    │                    │
│                              Flipper Zero               │
│                              (STM32WB55)                │
└─────────────────────────────────────────────────────────┘

REMOTE ACCESS:
Claude/MCP Client ──HTTP──► Relay Server ◄──WebSocket── ESP32-S2 ──UART──► Flipper Zero
                            (any machine)   (outbound)   (WiFi Board)     (main MCU)
```

**Data flow**: MCP JSON-RPC requests arrive over HTTP at the ESP32-S2. The firmware translates them into Flipper Zero CLI commands sent over UART at 115200 baud. Responses are parsed and returned as structured JSON.

**Remote access**: The ESP32-S2 initiates an outbound WebSocket connection to a relay server. The relay accepts MCP HTTP requests from clients and forwards them through the WebSocket tunnel. This solves NAT traversal and unknown-IP scenarios.

---

## 3. User Requirements (All Decisions Made)

These decisions were made during initial design and should be followed:

| Decision | Choice | Details |
|----------|--------|---------|
| **WiFi mode** | Dual (STA + AP) | STA connects to existing network; AP fallback with captive portal for config |
| **UART protocol** | CLI first, abstraction for RPC | `FlipperProtocol` trait; `CliProtocol` impl now, `RpcProtocol` (Protobuf) stub for future |
| **MCP transport** | Both | Streamable HTTP (modern, 2025-03-26 spec) + Legacy SSE (backward compat) |
| **Module discovery** | Both | Auto-scan FAP apps from SD card + TOML config-driven tool definitions |
| **Remote connectivity** | mDNS + WS tunnel | mDNS for LAN discovery (`flipper-mcp.local`); reverse WebSocket tunnel for cross-network |
| **Authentication** | None | No auth on MCP server (pentesting tool, local/controlled use) |
| **Language** | Rust | ESP32-S2 firmware in Rust via esp-idf-svc; relay server in Rust with tokio/axum |
| **Documentation** | Extensive | Full docs, helper scripts, troubleshooting, module dev guide |

---

## 4. Target Hardware

### ESP32-S2-WROVER (WiFi Dev Board v1)
- **CPU**: Xtensa LX7 single-core 32-bit @ 240 MHz
- **RAM**: 2 MB internal SRAM
- **Flash**: 4 MB SPI Flash
- **WiFi**: 802.11 b/g/n 2.4 GHz
- **No Bluetooth** on ESP32-S2

### Flipper Zero (Main MCU)
- **CPU**: STM32WB55 (ARM Cortex-M4 @ 64 MHz)
- **RAM**: 256 KB
- **Flash**: 1 MB
- **OS**: FreeRTOS-based custom firmware

### UART Connection (ESP32-S2 ↔ Flipper Zero)
- **Baud rate**: 115200
- **TX pin**: GPIO 1 (ESP32-S2) → RX on Flipper expansion header
- **RX pin**: GPIO 2 (ESP32-S2) ← TX on Flipper expansion header
- **Framing**: Send `command\r\n`, read until `>: ` prompt or 500ms timeout
- **Expansion Module Protocol**: Frame = header (1 byte) + contents (variable) + checksum (1 byte)

---

## 5. Flipper Zero CLI Commands

These are sent over UART and form the basis of all MCP tools:

### System
- `device_info` — hardware/firmware info
- `power info` / `power off` / `power reboot` — power management
- `ps` — running processes
- `free` — memory info
- `uptime` — system uptime

### SubGHz
- `subghz tx <protocol> <key> <frequency>` — transmit
- `subghz rx <frequency>` — receive
- `subghz rx_raw <frequency>` — raw receive
- `subghz decode_raw <file>` — decode raw capture
- `subghz chat <frequency>` — inter-Flipper chat
- `subghz tx_from_file <file>` — transmit from saved file

### NFC
- `nfc detect` — detect NFC tags
- `nfc emulate` — emulate NFC tag
- `nfc field` — enable NFC field

### RFID
- `rfid read` — read RFID tag
- `rfid emulate <type> <data>` — emulate RFID tag
- `rfid write <type> <data>` — write RFID tag

### Infrared
- `ir tx <protocol> <address> <command>` — transmit IR

### GPIO
- `gpio set <pin> <value>` — set GPIO output
- `gpio read <pin>` — read GPIO input
- `gpio mode <pin> <mode>` — configure pin mode

### Storage
- `storage list <path>` — list directory
- `storage read <path>` — read file
- `storage write <path> <data>` — write file
- `storage remove <path>` — delete file
- `storage stat <path>` — file info

### iButton
- `ikey read` — read iButton
- `ikey emulate <type> <data>` — emulate iButton

### Application Management
- `loader list` — list available apps
- `loader open <app_name>` — launch app
- `loader close` — close current app
- `loader info` — current app info

---

## 6. MCP Protocol Details

MCP uses **JSON-RPC 2.0** as the wire format.

### Core Methods to Implement
```
initialize          → capability negotiation
tools/list          → return all available tools with JSON Schema params
tools/call          → execute a tool, return result
resources/list      → list available resources
resources/read      → read a resource
notifications/initialized → client confirms init
```

### Streamable HTTP Transport (Primary)
- **`POST /mcp`** — JSON-RPC request → JSON-RPC response
- **`GET /mcp`** — SSE stream for server notifications
- Content-Type: `application/json` for requests, `text/event-stream` for SSE

### Legacy SSE Transport (Compatibility)
- **`GET /sse`** — SSE connection, first event contains endpoint URI
- **`POST /messages?sessionId=xxx`** — JSON-RPC requests for that session

### Example Exchange
```json
// Request
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nfc_detect","arguments":{}}}

// Response
{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"NFC-A detected: UID=04:AB:CD:EF"}]}}
```

---

## 7. Crate Dependencies

### Firmware (`firmware/Cargo.toml`)
```toml
[dependencies]
esp-idf-svc = { version = "0.49", features = ["binstart"] }
esp-idf-hal = "0.44"
log = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
anyhow = "1"
```

### Relay Server (`relay/Cargo.toml`)
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.7"
tokio-tungstenite = "0.24"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
```

---

## 8. Project Structure

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
│       │   ├── rpc.rs                 # Protobuf RPC (future, stub)
│       │   └── protocol.rs            # FlipperProtocol trait + factory
│       ├── mcp/
│       │   ├── mod.rs
│       │   ├── server.rs              # MCP server core (capability negotiation)
│       │   ├── jsonrpc.rs             # JSON-RPC 2.0 request/response handling
│       │   ├── transport/
│       │   │   ├── mod.rs
│       │   │   ├── streamable.rs      # Streamable HTTP (POST+SSE single endpoint)
│       │   │   └── sse.rs             # Legacy SSE (dual endpoint)
│       │   ├── tools.rs               # Tool registry, dispatch, schema generation
│       │   └── types.rs               # MCP type definitions
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
│       │   └── mdns.rs               # mDNS advertisement
│       └── config/
│           ├── mod.rs
│           ├── settings.rs            # Runtime config struct
│           └── nvs.rs                 # NVS read/write for persisted settings
│
├── relay/                             # Companion relay server binary
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                    # CLI: listen for WS + serve HTTP
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
│   ├── monitor.sh                     # Serial monitor
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
├── .github/workflows/ci.yml
├── Cargo.toml                         # Workspace root
├── AGENTS.md                          # This file
├── README.md
├── LICENSE
└── .gitignore
```

---

## 9. Implementation Phases

### Phase 1: Project Scaffolding & UART Communication
**Goal**: ESP32-S2 boots, connects to WiFi (STA), sends CLI commands to Flipper over UART.

**Files**: Workspace Cargo.toml, firmware/Cargo.toml, .cargo/config.toml, sdkconfig.defaults, build.rs, cfg.toml, main.rs, wifi/station.rs, uart/transport.rs, uart/cli.rs, uart/protocol.rs, config/nvs.rs, config/settings.rs

**Verify**: Flash firmware → serial monitor shows WiFi connected → `device_info` response logged from Flipper.

### Phase 2: MCP Server Core + HTTP
**Goal**: HTTP server at port 8080 responds to MCP JSON-RPC 2.0 with Streamable HTTP transport.

**Files**: mcp/server.rs, mcp/jsonrpc.rs, mcp/types.rs, mcp/tools.rs, mcp/transport/streamable.rs

**Verify**: `curl -X POST http://<ip>:8080/mcp` with `initialize` method returns capabilities; `tools/list` returns tools; `tools/call` with `system_info` returns Flipper device info.

### Phase 3: Built-in Modules + Module Framework
**Goal**: All default Flipper apps (~30 tools across 9 modules) exposed as MCP tools.

**Files**: modules/traits.rs, modules/registry.rs, all modules/builtin/*.rs

**Verify**: `tools/list` returns ~30 tools with JSON Schema; `tools/call` for each category works.

### Phase 4: Dynamic Module Discovery
**Goal**: FAP apps from SD card and TOML-defined tools auto-discovered.

**Files**: modules/discovery.rs, modules/config.rs, config/modules.example.toml

**Verify**: Install FAP → appears in `tools/list`; edit TOML config → `modules/refresh` → new tools appear.

### Phase 5: Legacy SSE Transport + WiFi AP Mode
**Goal**: Full MCP transport compatibility; zero-config WiFi setup via captive portal.

**Files**: mcp/transport/sse.rs, wifi/ap.rs, wifi/manager.rs

**Verify**: Boot with no creds → AP mode → configure via phone → reboot into STA; SSE client connects.

### Phase 6: Reverse WebSocket Tunnel + mDNS
**Goal**: Remote access via relay; local discovery via mDNS.

**Files**: tunnel/client.rs, tunnel/mdns.rs, relay/src/main.rs, relay/src/tunnel.rs, relay/src/proxy.rs

**Verify**: Start relay → Flipper connects outbound → `curl http://relay:9090/mcp` reaches Flipper; `ping flipper-mcp.local` resolves.

### Phase 7: Documentation, Scripts & Polish
**Goal**: Complete docs, helper scripts, CI pipeline.

**Files**: All docs/*.md, scripts/*.sh, README.md, .github/workflows/ci.yml

**Verify**: Fresh clone → follow SETUP.md → working; CI passes.

---

## 10. Coding Conventions

- **Error handling**: Use `anyhow::Result` for application code. Define specific error types only where recovery logic differs.
- **Logging**: Use the `log` crate (`info!`, `warn!`, `error!`, `debug!`). Initialize via `esp_idf_svc::log::EspLogger`.
- **Memory awareness**: ESP32-S2 has 2MB SRAM. Avoid large heap allocations. Limit HTTP request body to 16KB. Use fixed-size buffers where possible. Stream large responses.
- **Binary size**: 4MB flash is tight. Use `opt-level = "s"`, strip debug symbols in release, feature-gate optional modules.
- **Naming**: Rust standard — snake_case for functions/variables, PascalCase for types, SCREAMING_SNAKE for constants.
- **Modules**: Each built-in module is a struct implementing `FlipperModule` trait. Keep tool implementations thin — translate params to CLI command, send, parse response.
- **No unsafe**: Avoid `unsafe` code. The esp-idf-svc/hal crates handle the FFI boundary.

---

## 11. Build & Flash Commands

### Prerequisites
```bash
# Install Rust + Xtensa toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install espup
espup install  # Installs Xtensa Rust toolchain + LLVM
source ~/export-esp.sh  # Set environment variables

# Install flash tool
cargo install espflash
cargo install ldproxy
```

### Build Firmware
```bash
cd firmware
cargo build --release --target xtensa-esp32s2-espidf
```

### Flash & Monitor
```bash
espflash flash --monitor target/xtensa-esp32s2-espidf/release/flipper-mcp
```

### Build Relay Server
```bash
cd relay
cargo build --release
# Binary at target/release/flipper-mcp-relay
```

### Run Relay
```bash
./target/release/flipper-mcp-relay --listen 0.0.0.0:9090
```

---

## 12. Key Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| 4MB flash too small | `opt-level = "s"`, strip debug info, feature-gate optional modules |
| 2MB RAM pressure from HTTP + JSON | Stream responses, limit request size (16KB), reuse buffers |
| UART timing/framing issues | Configurable timeouts, prompt detection with fallback, retry logic |
| ESP-IDF WebSocket client maturity | Fall back to raw TCP + manual WS framing if needed |
| Cross-compilation complexity | Detailed setup docs, Docker build option, CI validates builds |
| CLI output parsing fragility | Robust parsers with fallback to raw text; integration tests with captured output |

---

## 13. Reference Links

- [esp-rs/esp-idf-svc](https://github.com/esp-rs/esp-idf-svc) — ESP-IDF services for Rust
- [esp-rs/esp-idf-hal](https://github.com/esp-rs/esp-idf-hal) — HAL for ESP chips
- [MCP Specification](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) — Transport spec
- [Flipper Zero CLI docs](https://docs.flipper.net/zero/development/cli) — CLI reference
- [Flipper Expansion Protocol](https://developer.flipper.net/flipperzero/doxygen/expansion_protocol.html) — UART protocol
- [ESP-IDF HTTP Server Training](https://docs.esp-rs.org/std-training/03_4_http_server.html) — HTTP on ESP32
- [awesome-esp-rust](https://github.com/esp-rs/awesome-esp-rust) — Curated resources
