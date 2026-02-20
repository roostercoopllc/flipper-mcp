# Setup Guide

## Prerequisites

### Hardware
- Flipper Zero (any firmware with CLI support, e.g. Official or Unleashed)
- [WiFi Dev Board v1](https://shop.flipperzero.one/products/wifi-devboard) (ESP32-S2-WROVER)

### System packages (Debian/Ubuntu/Kali)
```bash
sudo apt install -y git curl gcc build-essential pkg-config libudev-dev \
    libssl-dev python3 python3-venv cmake ninja-build
```

### Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Xtensa toolchain (one-time)
```bash
cargo install espup
espup install         # Downloads the Xtensa Rust fork — takes ~5 min
source ~/export-esp.sh
```

Add `source ~/export-esp.sh` to your `~/.bashrc` or `~/.zshrc`.

### Flash and linker tools
```bash
cargo install espflash ldproxy
```

Or run the setup script which does all of the above:
```bash
./scripts/setup-toolchain.sh
```

---

## Building the Firmware

```bash
source ~/export-esp.sh      # every new terminal session
cd firmware
cargo build --release --target xtensa-esp32s2-espidf
```

The binary is at `firmware/target/xtensa-esp32s2-espidf/release/flipper-mcp`.

Or use the helper script:
```bash
./scripts/build.sh
```

---

## Flashing

**Connect the WiFi Dev Board's USB-C port** (not the Flipper's USB-C).

```bash
./scripts/flash.sh
# Opens the serial monitor automatically after flashing.
# Press Ctrl+C to exit.
```

Or manually:
```bash
source ~/export-esp.sh
cd firmware
cargo run --release --target xtensa-esp32s2-espidf
```

### Troubleshooting flash failures
- Try adding `ESPFLASH_PORT=/dev/ttyUSB0` (or `ttyACM0`) to the environment
- On Kali/Debian: `sudo usermod -a -G dialout $USER` then log out and back in
- Hold the BOOT button on the WiFi Dev Board while plugging in to enter download mode manually

---

## WiFi Configuration

### Option A: Captive portal (easiest, no tools needed)

1. Flash the firmware (no WiFi credentials needed yet)
2. On first boot, the device creates a `FlipperMCP-XXXX` open WiFi hotspot
3. Connect any device to that hotspot
4. Open `http://192.168.4.1` in a browser
5. Enter your WiFi SSID and password, click **Save & Connect**
6. The device reboots and connects to your WiFi as a client

### Option B: NVS script (before first boot, requires python3)

```bash
./scripts/wifi-config.sh --ssid YourSSID --password YourPassword
```

Options:
```
--ssid       WiFi network name (required)
--password   WiFi password
--name       Device hostname (default: flipper-mcp)
--relay      Relay URL for remote access (e.g. ws://relay.example.com:9090/tunnel)
--erase      Erase stored credentials (returns device to AP mode)
```

### Option C: SD card config file

Create `/ext/apps_data/flipper_mcp/config.txt` on the Flipper's SD card:
```
# Flipper MCP Configuration
wifi_ssid=YourNetwork
wifi_password=YourPassword
device_name=flipper-mcp
relay_url=ws://relay.example.com:9090/tunnel
```

The firmware reads this file on every boot, overriding NVS values.

---

## First Connection

After the device connects to WiFi, the serial monitor shows:
```
=== Flipper MCP Firmware v0.1.0 ===
WiFi connected. IP: 192.168.1.xxx
HTTP server ready — POST /mcp, GET /health, GET /sse, POST /messages
mDNS: advertising flipper-mcp.local:8080
Firmware ready. MCP server listening on :8080
```

Test the connection:
```bash
curl http://flipper-mcp.local:8080/health
# or by IP:
curl http://192.168.1.xxx:8080/health

# Test MCP:
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}'
```

---

## Claude Desktop / Claude Code Integration

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS)
or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "flipper": {
      "url": "http://flipper-mcp.local:8080/mcp"
    }
  }
}
```

Restart Claude Desktop. The Flipper tools should appear.

For Claude Code:
```bash
claude mcp add flipper http://flipper-mcp.local:8080/mcp
```

---

## Optional: Enable mDNS and WebSocket Tunnel

These features require additional ESP-IDF managed components.
Add them to `firmware/idf_component.yml`:

```yaml
dependencies:
  idf: ">=5.2.0"
  espressif/mdns: ">=1.3.0"                       # for flipper-mcp.local discovery
  espressif/esp_websocket_client: ">=1.1.0"       # for relay tunnel
```

Then rebuild (internet access required on first build to download the components):
```bash
cd firmware && cargo clean && cargo build --release --target xtensa-esp32s2-espidf
```

Without these components:
- mDNS: use the device's IP address directly instead of `flipper-mcp.local`
- Tunnel: remote access unavailable (local access via IP still works)

---

## Custom Modules

See [MODULE_DEVELOPMENT.md](MODULE_DEVELOPMENT.md) for adding TOML-driven tools
and [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for common issues.
