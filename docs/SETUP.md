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

The binary is at `target/xtensa-esp32s2-espidf/release/flipper-mcp` (under the **workspace root**, not `firmware/target/`).

Or use the helper script:
```bash
./scripts/build.sh
```

---

## Flashing

The ESP32-S2 on the WiFi Dev Board v1 uses **native USB** (USB-OTG), not a
USB-to-UART bridge. This means flashing requires manually entering the ROM
bootloader — the chip cannot be auto-reset into download mode via serial
DTR/RTS lines like ESP32 boards with a CP2102/CH340 bridge.

### Step 1: Enter bootloader mode

**Remove the WiFi Dev Board from the Flipper** before flashing. The Flipper's
GPIO header can hold pins in states that interfere with the bootloader.

Then put the board into download mode:

1. **Unplug** the USB cable from the WiFi Dev Board
2. **Hold the BOOT button** (small tactile button on the board PCB)
3. **While holding BOOT**, plug the USB-C cable into the board
4. **Release BOOT** after ~1 second

Verify the board is in bootloader mode:
```bash
dmesg | tail -5
# Should show: "Product: USB JTAG/serial debug unit" or similar
# (NOT "Product: ESP32-S2" — that means the firmware booted instead)
ls /dev/ttyACM0   # Should exist
```

> **Tip:** If you see `Product: ESP32-S2` in dmesg, the firmware booted
> instead of the bootloader. Try again — hold BOOT *before* plugging in,
> and don't release it until the USB cable is firmly connected.

If the board has both BOOT and RESET buttons: hold BOOT, tap RESET briefly,
then release BOOT.

### Step 2: Flash

Flash **immediately** after entering bootloader mode (the bootloader can
time out):

```bash
espflash flash target/xtensa-esp32s2-espidf/release/flipper-mcp
```

> **Note:** The build places the binary under the **workspace root** `target/`
> directory, not `firmware/target/`. If running from the workspace root, use
> `target/xtensa-esp32s2-espidf/release/flipper-mcp`. See
> [TROUBLESHOOTING.md](TROUBLESHOOTING.md#no-such-file-or-directory-when-specifying-the-binary-path)
> if you get path errors.

Or use the helper script (handles the path automatically):
```bash
./scripts/flash.sh
```

### Step 3: Reset and re-attach

After flashing:
1. Unplug USB from the WiFi Dev Board
2. Seat the board back onto the Flipper's expansion header
3. Power on the Flipper — the ESP32 boots automatically

### Common flash errors

| Error | Cause | Fix |
|-------|-------|-----|
| `Communication error while flashing device` | Flash stub incompatibility | Add `--no-stub` flag |
| `Error while connecting to device` | Board not in bootloader mode | Redo BOOT + plug sequence |
| `No serial ports found` | USB not detected | Check cable, try different port |
| `No such file or directory` | Wrong binary path | Use workspace root `target/` path |
| `Permission denied` on `/dev/ttyACM0` | Not in dialout group | `sudo usermod -a -G dialout $USER` then re-login |

For detailed troubleshooting, see [TROUBLESHOOTING.md — Flash Issues](TROUBLESHOOTING.md#flash-issues).

---

## Flipper Settings (Important!)

Before using the WiFi Dev Board with this firmware, you **must** disable the
Flipper's expansion module protocol handler. If left enabled, it intercepts all
UART data and the ESP32 cannot communicate with the FAP.

**On the Flipper Zero:**
1. Go to **Settings → System → Expansion Modules**
2. Set to **None**

> **Symptom if skipped:** The firmware flashes and boots fine, but the FAP
> shows "No status yet" with `rx_bytes: 0` because the ESP32's UART messages
> are silently swallowed by the expansion protocol handler instead of
> reaching the FAP.

---

## WiFi Configuration

Create `/ext/apps_data/flipper_mcp/config.txt` on the Flipper's SD card:
```
wifi_ssid=YourNetwork
wifi_password=YourPassword
wifi_auth=wpa2
device_name=flipper-mcp
relay_url=wss://relay.example.com/tunnel
```

**`wifi_auth` values:**

| Value | Auth Method |
|-------|-------------|
| `wpa2` | WPA2-Personal (recommended default) |
| `wpa3` | WPA3-Personal |
| `wpa2wpa3` | WPA2/WPA3 transition mode |
| `open` | No authentication (open network) |
| *(empty/omitted)* | Auto — WPA2 if password set, open otherwise |

> **Tip:** Most home routers use WPA2. If you're unsure, start with `wifi_auth=wpa2`.
> If connection times out, try `wpa2wpa3`. The ESP32-S2 has limited WPA3 support.

You can create this file and load it in several ways:

### Option A: Edit SD card, then Load SD Config (recommended)

1. Mount the Flipper's SD card on your PC (or use qFlipper / USB mass storage)
2. Create/edit the file at `apps_data/flipper_mcp/config.txt` with a text editor
3. Eject the SD card and put it back in the Flipper
4. Open the Flipper MCP app: **Apps → Tools → Flipper MCP**
5. Select **Load SD Config** — this reads config.txt and sends it to the ESP32
6. Select **Reboot Board** to apply

This is the easiest method because you can type on a real keyboard with full
uppercase/special character support.

### Option B: Flipper FAP Configure WiFi screen

1. Open the Flipper MCP app: **Apps → Tools → Flipper MCP**
2. Select **Configure WiFi**
3. Enter your SSID (SSIDs are case-sensitive; the on-screen keyboard is
   lowercase-only on some firmware versions)
4. Enter your password
5. Optionally enter a relay URL
6. Select **Reboot Board** to apply

### Option C: Edit the SD card directly (no FAP needed)

Mount the Flipper's SD card on your PC and create/edit the file at
`apps_data/flipper_mcp/config.txt`. The ESP32 does not read this file
directly — you must use **Load SD Config** in the FAP to send it over UART.

If `wifi_ssid` is empty in the ESP32's NVS on boot, it enters a
**waiting-for-config** loop and sends `status=needs_config` to the FAP.
Use **Load SD Config** or **Configure WiFi** to set credentials, then
reboot the board.

---

## Verifying WiFi with Serial Monitor

Before testing the MCP server, verify that the ESP32 connects to WiFi
successfully. The USB serial monitor shows ESP-IDF's internal WiFi logs —
this is the best way to diagnose connection issues.

### Setup

The ESP32-S2's console output goes to USB CDC (`/dev/ttyACM0`). You can
monitor it while the board is attached to the Flipper:

1. Seat the WiFi Dev Board on the Flipper's GPIO header
2. **Also** connect a USB-C cable from the board to your PC
3. Power on the Flipper
4. Open a serial terminal:
   ```bash
   picocom -b 115200 /dev/ttyACM0
   # Or: screen /dev/ttyACM0 115200
   # Or: minicom -D /dev/ttyACM0 -b 115200
   ```

> **Note:** `espflash monitor` does not work reliably with ESP32-S2 USB-OTG.
> Use `picocom`, `screen`, or `minicom` instead.

> **Tip:** If you see no output, the board may have booted before the terminal
> was opened. Press the RESET button on the board (or power-cycle the Flipper)
> while the terminal is open. The firmware waits 2 seconds at startup so early
> messages aren't lost.

### What to look for

**Successful connection:**
```
=== Flipper MCP Firmware v0.1.0 ===
NVS: wifi_ssid loaded
NVS: wifi_password loaded
WiFi auth: WPA2Personal (config='wpa2')
WiFi started
WiFi connected
WiFi connected — IP: 192.168.1.xxx
HTTP server started
Firmware ready. MCP server listening on :8080
```

**Failed connection (timeout):**
```
WiFi auth: WPA2Personal (config='wpa2')
WiFi started
WiFi connect failed: ESP_ERR_TIMEOUT. Retrying in 10s.
```

If WiFi times out repeatedly, check:
- SSID and password are correct (case-sensitive)
- Network is **2.4 GHz** (ESP32-S2 does not support 5 GHz)
- Try `wifi_auth=wpa2wpa3` in config.txt if your router uses WPA3
- Try a phone hotspot with a simple SSID/password to isolate router issues

### Exit the serial monitor

| Tool | Exit shortcut |
|------|---------------|
| picocom | `Ctrl-A` then `Ctrl-X` |
| screen | `Ctrl-A` then `K` |
| minicom | `Ctrl-A` then `X` |

---

## First Connection

After confirming WiFi is connected (via serial monitor or FAP Status screen),
test the MCP server:

```bash
# Health check (use the IP from serial output or FAP Status):
curl http://192.168.1.xxx:8080/health

# If mDNS is available:
curl http://flipper-mcp.local:8080/health

# Test MCP initialize:
curl -X POST http://192.168.1.xxx:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}'

# List available tools:
curl -X POST http://192.168.1.xxx:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

> **If health check fails:** Verify your PC can reach the ESP32 with
> `ping 192.168.1.xxx`. If ping fails, check your router for AP/client
> isolation settings that block device-to-device traffic.

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
