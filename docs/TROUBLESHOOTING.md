# Troubleshooting

## Build Issues

### `error: toolchain 'esp' is not installed`
The Xtensa toolchain isn't installed or the environment isn't loaded.
```bash
# Install
cargo install espup && espup install
# Load for this session
source ~/export-esp.sh
# Load permanently (add to ~/.bashrc or ~/.zshrc)
echo 'source ~/export-esp.sh' >> ~/.bashrc
```

### `cannot find -lxml2` or `libxml2.so.2: No such file`
ESP-IDF's clang needs `libxml2.so.2`. On Kali Linux, only `libxml2.so.16` exists.
```bash
sudo apt install libxml2-dev
# If still broken on Kali:
sudo ln -s /usr/lib/x86_64-linux-gnu/libxml2.so.16 /usr/lib/x86_64-linux-gnu/libxml2.so.2
```

### Build succeeds but `firmware/` build doesn't pick up `sdkconfig.defaults` changes
The ESP-IDF cmake build is cached. Force a full rebuild:
```bash
cd firmware && cargo clean && cargo build --release --target xtensa-esp32s2-espidf
```

### `heapless String try_into()` errors with `.context()`
`try_into()` on heapless::String returns `Result<_, ()>` — `()` doesn't implement `std::error::Error`.
Use `ensure!()` + `unwrap()` instead of `.context("…")?`.

---

## Flash Issues

### Device not detected / `No serial ports found`
```bash
# Check if device is visible
lsusb | grep -i silicon   # Should show Silicon Labs CP2102 or similar
ls /dev/ttyUSB* /dev/ttyACM*

# Add yourself to dialout group (requires logout)
sudo usermod -a -G dialout $USER

# Or flash with explicit port
ESPFLASH_PORT=/dev/ttyUSB0 cargo run --release --target xtensa-esp32s2-espidf
```

Make sure you're connected to the **WiFi Dev Board's USB-C port**, not the Flipper's.

### Flash fails with timeout
Hold the **BOOT** button on the WiFi Dev Board while clicking **Reset**, or while plugging in USB.

### `espflash: command not found`
```bash
cargo install espflash
# Make sure ~/.cargo/bin is in PATH:
export PATH="$HOME/.cargo/bin:$PATH"
```

---

## WiFi Issues

### Device creates FlipperMCP-XXXX hotspot instead of connecting
No WiFi credentials are stored. Use the captive portal:
1. Connect to `FlipperMCP-XXXX` (open network)
2. Open `http://192.168.4.1`
3. Enter your WiFi SSID and password
4. Click Save & Connect

Or pre-configure via script:
```bash
./scripts/wifi-config.sh --ssid YourSSID --password YourPassword
```

### Device connected to WiFi but can't be reached
```bash
# Find the IP from serial monitor
./scripts/monitor.sh
# Look for: "WiFi connected. IP: 192.168.x.xxx"

# Or check your router's DHCP client list

# Test health endpoint
curl http://192.168.x.xxx:8080/health
```

### `flipper-mcp.local` not resolving
mDNS either isn't built in or isn't working on your OS.
- macOS/iOS: should work out of the box
- Linux: install `avahi-daemon` if not running
- Windows: install Apple Bonjour or use the IP address directly
- If mDNS component isn't enabled: use the IP address from the serial monitor

To enable mDNS, add to `firmware/idf_component.yml`:
```yaml
dependencies:
  espressif/mdns: ">=1.3.0"
```
Then: `cd firmware && cargo clean && cargo build --release --target xtensa-esp32s2-espidf`

---

## UART / Flipper Communication Issues

### `UART smoke tests failed` or tools returning empty output
Check the physical connection between the WiFi Dev Board and Flipper Zero:
- The GPIO header must be fully seated
- Flipper must be powered on
- Flipper CLI must be accessible (not in a full-screen app)

Test manually with `espflash monitor`:
```
storage list /ext
```
Should return a directory listing. If it returns empty or errors, the UART connection has an issue.

### Commands timeout (`execute_command: timeout`)
- Default timeout is 500ms — some commands (like `subghz rx`) take longer
- The Flipper might be running a full-screen app that blocks the CLI
- Try restarting the Flipper

### `Storage error: File not found` for SD card config
The SD card isn't inserted, or the path doesn't exist. This is non-fatal — the firmware continues with NVS/default settings.

---

## MCP / Tool Issues

### `Method not found` for a tool
The tool name might have changed. Check current tool names:
```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

### FAP tools not appearing in `tools/list`
Dynamic discovery runs at startup. Trigger a refresh:
```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}'
```
Ensure the FAP is in `/ext/apps/` (nested one level, e.g. `/ext/apps/Games/MyApp.fap`).

### MCP client connects but tools fail
Check the serial monitor for error details. Common causes:
- Flipper is disconnected from the WiFi Dev Board
- The tool's CLI command failed on the Flipper side
- A required tool argument is missing or has the wrong type

---

## Relay Issues

### Relay shows `SERVICE_UNAVAILABLE` (503)
No device is connected to the relay. Check:
- The device's `relay_url` is set correctly (starts with `ws://`)
- The relay is reachable from the device's network
- Check relay logs: device should log "Tunnel: WebSocket connected"

To enable the tunnel, the esp_websocket_client component is required:
```yaml
# firmware/idf_component.yml
dependencies:
  espressif/esp_websocket_client: ">=1.1.0"
```
Then rebuild after `cargo clean`.

### Gateway Timeout (504) on relay requests
The Flipper tool took more than 30 seconds to respond. This is unusual — check for UART connectivity issues.

---

## Memory / Stack Issues

### Panic: `stack overflow`
The ESP32-S2 has limited stack space per thread. If a handler panics with a stack overflow:
- The HTTP server handler stack is set to 10240 bytes — increase in `streamable.rs` `Configuration::stack_size`
- The tunnel thread uses 10240 bytes — increase `TUNNEL_STACK_SIZE` in `tunnel/client.rs`

The main task uses 20000 bytes (set in `sdkconfig.defaults` as `CONFIG_ESP_MAIN_TASK_STACK_SIZE`).
