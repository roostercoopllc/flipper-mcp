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
- **Flipper-first setup** — configure WiFi from the Flipper FAP (no phone, browser, or PC scripts needed)
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

Create `config.txt` on the Flipper SD card at `SD:/apps_data/flipper_mcp/config.txt`:

```ini
wifi_ssid=YourNetworkName
wifi_password=YourPassword
device_name=flipper-mcp
```

**Or configure directly from the Flipper** using the companion FAP (see Step 5):
`Apps → Tools → Flipper MCP → Configure WiFi`

On first boot without a config file, the ESP32 waits and writes `status=needs_config` to the status file. The Flipper FAP will display this on the Status screen.

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

#### Test MCP tools directly with curl

Get the Flipper's IP from the **Flipper MCP → Status** menu, then test:

**BLE beacon broadcast** (transmit spoofed BLE advertisement):
```bash
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_beacon","arguments":{"data":"020106"}}}'
```

**BLE HID keyboard** (emulate a wireless keyboard and type):
```bash
# Start HID emulation
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ble_hid_start","arguments":{}}}'

# Type a message
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ble_hid_type","arguments":{"text":"Hello from Flipper!"}}}'

# Stop HID emulation
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ble_hid_stop","arguments":{}}}'
```

**IR transmit** (send IR remote control codes):
```bash
# NEC protocol IR code (generic TV power button)
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ir_tx","arguments":{"protocol":"NEC","address":"00","command":"01","repeat":0}}}'
```

**SubGHz receive** (listen for wireless signals on 433.92 MHz):
```bash
curl -X POST http://192.168.0.58:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"subghz_rx","arguments":{"frequency":433920000,"duration":5000}}}'
```

Replace `192.168.0.58` with your Flipper's actual IP address.

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
| **Status** | Requests a fresh status update from the ESP32, shows IP, SSID, server state, version |
| **Start / Stop / Restart** | Controls the MCP HTTP server lifecycle |
| **Reboot Board** | Restarts the ESP32 WiFi Dev Board |
| **Configure WiFi** | On-screen keyboard to enter SSID + password; writes `config.txt` to SD card |
| **View Logs** | Scrollable diagnostic log written by the ESP32 every 30 s |
| **Tools List** | Scrollable list of all MCP tools currently registered on the ESP32 |
| **Refresh Modules** | Triggers FAP discovery rescan + `modules.toml` reload on the ESP32 |

The app communicates via SD card files (no extra wiring beyond the GPIO header). **Configure WiFi** is the first-boot wizard — no phone, browser, or PC scripts required.

### 6. (Optional) Remote access via relay

**Self-hosted (run the binary anywhere):**
```bash
./scripts/build-relay.sh
./target/release/flipper-mcp-relay --listen 0.0.0.0:9090
# Then add relay_url to config.txt on the Flipper SD card:
# relay_url=ws://your-server:9090/tunnel
```

**Cloud deployment (AWS or GCP, with TLS + DNS):**
```bash
# Bootstrap state storage, then deploy
./infra/bootstrap/aws.sh   # or ./infra/bootstrap/gcp.sh
cd infra/aws && cp terraform.tfvars.example terraform.tfvars
tofu init && tofu apply
# Outputs the relay URL and a ready-to-paste wifi-config.sh command
```
See [RELAY.md](docs/RELAY.md#cloud-deployment-opentofu) for full instructions.

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
| Storage | `storage_list`, `storage_read`, `storage_write`, `storage_remove`, `storage_stat` | SD card file management |
| System | `system_device_info`, `system_power_info`, `system_power_reboot`, `system_ps`, `system_free`, `system_uptime` | Device management |
| BLE | `ble_info`, `ble_beacon`, `ble_beacon_stop`, `ble_hid_start`, `ble_hid_type`, `ble_hid_press`, `ble_hid_mouse`, `ble_hid_stop` | Bluetooth Low Energy (beacon broadcast + HID emulation) |
| Apps | `app_launch_{name}` (auto-discovered from SD card) | Application management |

Custom tools can be added via TOML config files or by installing FAP apps on the SD card. See [OPERATIONS.md](docs/OPERATIONS.md) for copy-paste curl commands for every tool.

## Free Alternative: Open WebUI + Ollama

You don't need a Claude subscription to use flipper-mcp. [Open WebUI](https://github.com/open-webui/open-webui) is a free, self-hosted ChatGPT-style interface that natively supports MCP Streamable HTTP (v0.6.31+). Pair it with [Ollama](https://ollama.com/) for fully local, offline AI-driven Flipper control.

### 1. Install Ollama and pull a tool-capable model

```bash
# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# Pull a model with tool-calling support
ollama pull llama3.1        # 8B — good balance of speed and capability
# or: ollama pull qwen2.5   # strong tool-calling, good at structured output
# or: ollama pull mistral    # lightweight, fast tool use
```

### 2. Start Open WebUI

```bash
# Docker (recommended) — connects to Ollama on localhost automatically
docker run -d -p 3000:8080 \
  --add-host=host.docker.internal:host-gateway \
  -e OLLAMA_BASE_URL=http://host.docker.internal:11434 \
  -e WEBUI_AUTH=False \
  -v open-webui:/app/backend/data \
  --name open-webui \
  ghcr.io/open-webui/open-webui:main

# Or without Docker:
pip install open-webui
open-webui serve
```

Open **http://localhost:3000** in your browser.

### 3. Connect the Flipper MCP server

1. Go to **Admin Settings** (gear icon) → **External Tools**
2. Click **+ Add Server**
3. Set **Type** to **MCP (Streamable HTTP)**
4. Enter the server URL:
   - Local: `http://flipper-mcp.local:8080/mcp` (or `http://192.168.x.x:8080/mcp`)
   - Via relay: `https://relay.example.com/mcp`
   - From Docker: use `http://host.docker.internal:8080/mcp` if the Flipper is on the Docker host's network
5. Set **Authentication** to **None** (flipper-mcp has no auth)
6. **Save**

> **Tip:** Under **Workspace → Models → (your model) → Advanced Parameters**, set **Function Calling** to **Default** for smaller models. Only switch to **Native** for models with strong built-in tool support (Llama 3.1 8B+, Qwen 2.5, Mistral).

### 4. Example prompts

Once connected, try these in the Open WebUI chat:

| Prompt | What it does |
|--------|-------------|
| "Scan for NFC tags near the Flipper" | Calls `nfc_detect` to read nearby tags |
| "List all files on the Flipper's SD card" | Calls `storage_list` on `/ext` |
| "Transmit this SubGHz signal on 433.92 MHz: ..." | Calls `subghz_tx` with the given frequency |
| "Read any RFID card that's presented to the Flipper" | Calls `rfid_read` and returns tag data |
| "What apps are installed on the Flipper?" | Calls `app_list` to enumerate installed FAPs |
| "Send this IR signal to turn off the TV" | Calls `ir_tx` with the specified protocol and data |
| "Show me the Flipper's system info and free memory" | Calls `system_info` + `system_free` |
| "Read the NFC tag, then save its data to /ext/nfc/captured.nfc" | Multi-step: `nfc_read` → `storage_write` |
| "Monitor 315 MHz for 10 seconds and decode anything you hear" | Calls `subghz_rx` with frequency and duration |

**Multi-step agentic tasks** work best with larger models (Llama 3.1 70B, Qwen 2.5 72B, or cloud models via OpenAI-compatible APIs). Smaller models handle single-tool calls reliably.

## Project Structure

```
flipper-mcp/
├── firmware/          # ESP32-S2 firmware (Rust, esp-idf-svc)
├── relay/             # Companion relay server (Rust, tokio/axum)
├── flipper-app/       # Flipper Zero FAP — in-device management UI (C, ufbt)
├── infra/             # OpenTofu IaC — cloud relay deployment (AWS + GCP)
├── config/            # Example module configurations
├── scripts/           # Build, flash, and setup helper scripts
└── docs/              # Architecture, setup, API, troubleshooting
```

## Documentation

- [SETUP.md](docs/SETUP.md) — Full setup from scratch
- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — System design deep dive
- [API.md](docs/API.md) — Complete MCP tool reference
- [OPERATIONS.md](docs/OPERATIONS.md) — Operations guide with curl commands for every tool
- [MODULE_DEVELOPMENT.md](docs/MODULE_DEVELOPMENT.md) — Create custom modules
- [RELAY.md](docs/RELAY.md) — Remote access setup
- [HARDWARE.md](docs/HARDWARE.md) — Wiring and hardware details
- [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) — Common issues and fixes
- [DESIGN.md](docs/DESIGN.md) — Implementation plan and phases

## For AI Agents

See [AGENTS.md](AGENTS.md) for complete project context, technical specifications, implementation phases, and everything needed to continue development on this project.

## License

MIT
