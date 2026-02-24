# Flipper Zero MCP Server — Technical Design

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

## Core Components

### 1. WiFi & Network Connectivity

**Files**: `src/wifi/station.rs`, `src/wifi/ap.rs`, `src/wifi/manager.rs`, `src/config/nvs.rs`, `src/config/settings.rs`

**Responsibility**: Initialize WiFi in STA mode (connect to existing network), provide AP mode fallback with captive portal for initial configuration, manage NVS storage for persistent WiFi credentials.

**Key Features**:
- **STA Mode**: Connect to configured WiFi SSID using credentials from NVS or config file
- **AP Mode**: Fallback hotspot (`FlipperMCP-XXXX`) with captive portal for setting WiFi without external tools
- **Dual-mode Manager**: Boot with STA, fall back to AP if connection fails, periodic connectivity monitoring
- **NVS Persistence**: Store WiFi credentials, device name, relay URL across reboots

**Architecture**:
```rust
// Config stored in NVS
pub struct Settings {
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub device_name: String,
    pub relay_url: Option<String>,
}
```

---

### 2. UART & Device Communication

**Files**: `src/uart/transport.rs`, `src/uart/cli.rs`, `src/uart/protocol.rs`, `src/uart/rpc.rs`

**Responsibility**: Establish reliable serial communication with Flipper Zero at 115200 baud, handle CLI command transmission, parse responses, provide abstraction layer for protocol variants.

**Key Features**:
- **UART Framing**: Send `command\r\n`, receive until `>: ` prompt or 500ms timeout
- **CLI Protocol** (active): Text-based command execution with response parsing
- **Protobuf RPC** (stub): Placeholder for future binary protocol
- **Error Handling**: Timeout recovery, retry logic, fallback response modes

**Protocol Trait**:
```rust
pub trait FlipperProtocol: Send + Sync {
    fn execute_command(&mut self, cmd: &str) -> Result<String>;
    fn list_apps(&mut self) -> Result<Vec<String>>;
    fn launch_app(&mut self, name: &str) -> Result<()>;
    fn get_device_info(&mut self) -> Result<DeviceInfo>;
}
```

---

### 3. HTTP Server & MCP Protocol

**Files**: `src/mcp/server.rs`, `src/mcp/jsonrpc.rs`, `src/mcp/types.rs`, `src/mcp/tools.rs`, `src/mcp/transport/streamable.rs`, `src/mcp/transport/sse.rs`

**Responsibility**: Implement HTTP server (port 8080) with MCP JSON-RPC 2.0 protocol, handle client capability negotiation, dispatch tool execution requests, support multiple MCP transport variants.

**Key Features**:
- **JSON-RPC 2.0**: Full request/response handling with error codes, batch support
- **Streamable HTTP** (primary): Single endpoint `/mcp` for both requests and SSE notifications
- **Legacy SSE** (backward compatible): Dual endpoints `/sse` and `/messages?sessionId=xxx`
- **Tool Registry**: Dynamic tool registration from modules with JSON Schema parameter definitions
- **Server Capabilities**: Declare supported features (tools, resources) to clients

**Protocol Flow**:
1. Client → `initialize` → Server returns capabilities
2. Client → `tools/list` → Server returns all available tools with schemas
3. Client → `tools/call` → Server executes tool, returns structured result
4. Server → Server notifications (if client supports SSE)

---

### 4. Module System & Tool Registry

**Files**: `src/modules/traits.rs`, `src/modules/registry.rs`, `src/modules/builtin/*.rs`, `src/modules/discovery.rs`, `src/modules/config.rs`

**Responsibility**: Provide extensible interface for exposing Flipper capabilities as tools, implement all built-in modules (~30 tools across 9 categories), support dynamic FAP app discovery and config-driven tool definition.

**Key Features**:
- **FlipperModule Trait**: Standard interface for module implementation
- **Module Registry**: Central dispatcher for tool lookup and execution
- **Built-in Modules**:
  - SubGHz: TX, RX, decode, chat (wireless transmission)
  - NFC: Detect, read, emulate, field operations
  - RFID: Read, emulate, write operations
  - Infrared: TX, RX, learn remote codes
  - GPIO: Read, write, set mode
  - BadUSB: Run scripts, generate HID sequences
  - iButton: Read, emulate 1-Wire keys
  - Storage: List, read, write, delete SD card files
  - System: Device info, power management, process listing, memory info
  - BLE: Beacon broadcast, HID emulation, keyboard, mouse
- **FAP Discovery**: Auto-scan SD card for `.fap` apps and create launch tools
- **TOML Configuration**: Define custom tools via config files

---

### 5. Remote Access & Relay Tunnel

**Firmware Files**: `src/tunnel/client.rs`, `src/tunnel/mdns.rs`

**Relay Files**: `relay/src/main.rs`, `relay/src/tunnel.rs`, `relay/src/proxy.rs`

**Responsibility**: Enable cross-network access through reverse WebSocket tunnel, manage device identity and routing, support multiple simultaneous Flippers on relay server.

**Firmware Tunnel Client**:
- Outbound WebSocket connection to relay server
- Device ID in handshake for identification
- Automatic reconnection with exponential backoff (5s → 10s → 30s → 60s max)
- Transparent request/response forwarding

**Relay Server**:
- Accept WebSocket connections from multiple Flippers (by device ID)
- HTTP endpoints (`/mcp`, `/sse`, `/messages`) that proxy to connected Flipper
- Health endpoint for monitoring
- Support for path-based or header-based routing

**mDNS Advertisement**:
- Service type: `_mcp._tcp`
- Hostname: `flipper-mcp` (configurable)
- Clients discover as `flipper-mcp.local:8080`

---

### 6. Runtime Configuration & Settings Management

**Files**: `src/config/settings.rs`, `src/config/nvs.rs`, `config/modules.example.toml`

**Responsibility**: Manage device configuration (WiFi, relay URL, device name), provide persistent storage via NVS, load module definitions from TOML files.

**Configuration Sources**:
1. **Defaults**: Built-in reasonable defaults (WiFi AP, no relay)
2. **NVS Storage**: Persist across reboots (WiFi credentials, device name)
3. **SD Card**: TOML files for module configuration and custom tools
4. **Runtime**: Updates via server control commands

**Configuration Structure**:
```toml
[server]
device_name = "flipper-mcp"
wifi_ssid = "YourNetwork"
wifi_password = "YourPassword"
relay_url = "ws://relay.example.com:9090/tunnel"

[[modules]]
name = "custom_scanner"
[[modules.tools]]
name = "scan_frequency"
command_template = "subghz rx {frequency}"
```

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
