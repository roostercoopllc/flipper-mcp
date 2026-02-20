# flipper-mcp

A Rust MCP (Model Context Protocol) server that runs directly on the Flipper Zero's WiFi Dev Board v1 (ESP32-S2), enabling AI agents to control a Flipper Zero over the network.

## What is this?

This project puts an MCP server **on the Flipper itself**. Any MCP-compatible AI client (Claude Desktop, Claude Code, etc.) can connect and use the Flipper's capabilities as tools — SubGHz, NFC, RFID, IR, GPIO, BadUSB, iButton, file storage, and more.

Unlike other projects that require a USB-connected host computer, flipper-mcp runs on the ESP32-S2 WiFi module attached to the Flipper. The Flipper becomes a standalone, network-accessible tool.

## Architecture

```
LOCAL (same network):
  MCP Client ──HTTP──► flipper-mcp.local:8080 (ESP32-S2) ──UART──► Flipper Zero

REMOTE (cross-network):
  MCP Client ──HTTP──► Relay Server ◄──WebSocket── ESP32-S2 ──UART──► Flipper Zero
```

The ESP32-S2 runs an HTTP server implementing the MCP protocol. It translates MCP tool calls into Flipper Zero CLI commands over UART at 115200 baud. A companion relay server enables remote access from any network.

## Features

- **~30 built-in tools** covering all default Flipper Zero applications
- **Dynamic module discovery** — auto-detect FAP apps from SD card + TOML config-driven tools
- **Dual WiFi mode** — connects to your network (STA) or creates its own hotspot (AP) with captive portal
- **Dual MCP transport** — Streamable HTTP (modern) + Legacy SSE (backward compatible)
- **Local discovery** — mDNS advertisement as `flipper-mcp.local`
- **Remote access** — reverse WebSocket tunnel through a relay server (no port forwarding needed)
- **No authentication** — designed for pentesting and security research scenarios
- **Companion relay server** — small Rust binary for cross-network access, supports multiple Flippers

## Hardware Required

- Flipper Zero (any firmware version with CLI support)
- WiFi Dev Board v1 (ESP32-S2-WROVER module)

**Which device do I connect to?**

| Stage | Connect to | Notes |
|-------|-----------|-------|
| Flashing firmware | **WiFi Dev Board** USB-C | Board has its own USB port, separate from Flipper |
| Serial monitoring | **WiFi Dev Board** USB-C | Same USB connection as flashing |
| SD card config files | **Flipper Zero** SD card | Insert SD into Flipper, or remove and mount on PC |
| Server control commands | **Flipper Zero** SD card | Create `server.cmd` file in `apps_data/flipper_mcp/` |
| UART communication | Automatic | WiFi Dev Board and Flipper connect via GPIO header |
| MCP HTTP requests | **WiFi Dev Board** IP:8080 | Connect over your WiFi network |

## Prerequisites

- **Rust** — install via [rustup](https://rustup.rs/):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Xtensa toolchain** — ESP32-S2 requires a custom Rust toolchain:
  ```bash
  cargo install espup
  espup install
  source ~/export-esp.sh  # Run this in every new terminal
  ```
- **Flash tool & linker proxy**:
  ```bash
  cargo install espflash
  cargo install ldproxy
  ```
- **System packages** (Debian/Ubuntu/Kali):
  ```bash
  sudo apt install -y git curl gcc build-essential pkg-config libudev-dev libssl-dev python3 python3-venv cmake ninja-build
  ```

## Quick Start

> Full setup instructions in [docs/SETUP.md](docs/SETUP.md)

### 1. Build & flash firmware
```bash
source ~/export-esp.sh
cd firmware
cargo build --release --target xtensa-esp32s2-espidf
espflash flash --monitor target/xtensa-esp32s2-espidf/release/flipper-mcp
```

### 3. Configure WiFi
On first boot, the Flipper creates a `FlipperMCP-XXXX` hotspot. Connect to it, enter your WiFi credentials in the captive portal, and the device reboots into station mode.

Or pre-configure via script (writes to NVS before first boot):
```bash
./scripts/wifi-config.sh --ssid YourSSID --password YourPassword
```

### 4. Verify the server is working

Before configuring an AI client, confirm the server is reachable with curl:

```bash
# Quick health check
curl http://flipper-mcp.local:8080/health

# Full verification — initialize + list all available tools
./scripts/test-connection.sh

# If mDNS isn't resolving on your OS, pass the IP directly:
./scripts/test-connection.sh 192.168.x.xxx
```

Then add to your Claude Desktop config (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "flipper": {
      "url": "http://flipper-mcp.local:8080/mcp"
    }
  }
}
```

### 5. Manage the WiFi board from the Flipper

Install the companion **Flipper MCP** app from `flipper-app/` onto the Flipper Zero. Build with [ufbt](https://github.com/flipperdevices/flipperzero-ufbt) and copy the `.fap` to `SD:/apps/Tools/`:

```bash
cd flipper-app && ufbt   # produces flipper_mcp.fap
# Copy flipper_mcp.fap to your Flipper SD card under apps/Tools/
```

The app appears in **Apps → Tools → Flipper MCP** and provides:

| Screen | What it does |
|--------|-------------|
| **Status** | Shows WiFi IP, SSID, server state, firmware version |
| **Start Server** | Brings the MCP HTTP server online |
| **Stop Server** | Takes the MCP HTTP server offline |
| **Restart Server** | Stops then starts — pick up config changes |

The app communicates via SD card files (no extra wiring beyond the standard GPIO header). The ESP32 writes a status file every 30 seconds; the Flipper app reads it on the Status screen.

### 6. (Optional) Remote access via relay
```bash
./scripts/build-relay.sh
./target/release/flipper-mcp-relay --listen 0.0.0.0:9090
# Then configure the relay URL on the device:
./scripts/wifi-config.sh --ssid MySSID --password MyPass --relay ws://your-server:9090/tunnel
```

## Available Tools

| Category | Tools | Description |
|----------|-------|-------------|
| SubGHz | `subghz_tx`, `subghz_rx`, `subghz_decode_raw`, `subghz_chat`, `subghz_tx_from_file` | Radio frequency operations |
| NFC | `nfc_detect`, `nfc_read`, `nfc_emulate`, `nfc_field` | NFC tag interaction |
| RFID | `rfid_read`, `rfid_emulate`, `rfid_write` | Low-frequency RFID |
| Infrared | `ir_tx`, `ir_rx` | IR remote control |
| GPIO | `gpio_read`, `gpio_write`, `gpio_set_mode` | Pin I/O control |
| BadUSB | `badusb_run`, `badusb_list` | USB HID attacks |
| iButton | `ibutton_read`, `ibutton_emulate` | 1-Wire key fobs |
| Storage | `storage_list`, `storage_read`, `storage_write`, `storage_remove` | SD card file management |
| System | `system_info`, `system_reboot`, `system_power`, `system_ps`, `system_free` | Device management |
| Apps | `app_list`, `app_launch`, `app_close`, `app_info` | Application management |

Custom tools can be added via TOML config files or by installing FAP apps on the SD card.

## Project Structure

```
flipper-mcp/
├── firmware/          # ESP32-S2 firmware (Rust, esp-idf-svc)
├── relay/             # Companion relay server (Rust, tokio/axum)
├── flipper-app/       # Flipper Zero FAP — in-device management UI (C, ufbt)
├── config/            # Example module configurations
├── scripts/           # Build, flash, and setup helper scripts
└── docs/              # Architecture, setup, API, troubleshooting
```

## Documentation

- [SETUP.md](docs/SETUP.md) — Full setup from scratch
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design deep dive
- [API.md](docs/API.md) — Complete MCP tool reference
- [MODULE_DEVELOPMENT.md](docs/MODULE_DEVELOPMENT.md) — Create custom modules
- [RELAY.md](docs/RELAY.md) — Remote access setup
- [HARDWARE.md](docs/HARDWARE.md) — Wiring and hardware details
- [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) — Common issues and fixes
- [DESIGN.md](docs/DESIGN.md) — Implementation plan and phases

## For AI Agents

See [AGENTS.md](AGENTS.md) for complete project context, technical specifications, implementation phases, and everything needed to continue development on this project.

## License

MIT
