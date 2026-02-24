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

### Understanding ESP32-S2 USB flashing

The WiFi Dev Board v1 uses an **ESP32-S2 with native USB-OTG** — there is no
USB-to-UART bridge chip (like CP2102 or CH340). This has two important
consequences:

1. **The running firmware creates `/dev/ttyACM0`** via USB CDC
   (`CONFIG_ESP_CONSOLE_USB_CDC=y`). This looks like a serial port but it's
   the firmware's console — **not** the bootloader. You cannot flash through
   this interface.

2. **You must physically enter the ROM bootloader** by holding GPIO0 (BOOT
   button) during reset. Unlike ESP32 boards with a UART bridge, `espflash`
   cannot auto-reset the S2 into download mode via DTR/RTS.

When you see `dmesg` output like:
```
Product: ESP32-S2
Manufacturer: Espressif
cdc_acm ... ttyACM0: USB ACM device
```
That's the **firmware running**, not the bootloader. The bootloader shows
different USB descriptors (e.g., `USB JTAG/serial debug unit`).

### Entering bootloader mode (required for every flash)

**Important:** Remove the WiFi Dev Board from the Flipper's GPIO header
before flashing. The Flipper can hold GPIO pins in states that prevent
bootloader entry.

**Method A — BOOT + USB plug (most reliable):**
1. Unplug the USB cable from the WiFi Dev Board
2. Locate the **BOOT** button on the board PCB (small tactile switch)
3. **Hold BOOT**, then plug the USB-C cable in
4. Wait 1 second, then release BOOT
5. Check `dmesg | tail -5` — you should NOT see `Product: ESP32-S2`

**Method B — BOOT + RESET (if board has both buttons):**
1. With USB already connected, hold **BOOT**
2. Tap **RESET** briefly
3. Release **BOOT**

**Method C — Software reboot to bootloader** (no physical buttons needed):

If the firmware is running and accessible via USB serial, you can trigger a
reboot into download mode. Install `esptool`:
```bash
pip install esptool  # or: pip install --break-system-packages esptool
```
Then:
```bash
esptool.py --chip esp32s2 --port /dev/ttyACM0 run
# This sometimes resets the chip; immediately re-run espflash
```

### Flashing the firmware

After entering bootloader mode, flash **immediately** (the bootloader can
time out):

```bash
espflash flash target/xtensa-esp32s2-espidf/release/flipper-mcp
```

If `espflash` detects the port automatically, it should flash without
specifying `--port`. If it picks the wrong port:
```bash
espflash flash --port /dev/ttyACM0 \
  target/xtensa-esp32s2-espidf/release/flipper-mcp
```

### `Communication error while flashing device`

This means `espflash` connected to the bootloader and uploaded the flash stub,
but communication broke during the actual data transfer. Common causes:

- **USB cable issue:** Use a short, high-quality data cable (not charge-only)
- **USB hub instability:** Connect directly to the computer, not through a hub
- **Flash stub incompatibility:** Bypass the stub with `--no-stub`:
  ```bash
  espflash flash --no-stub target/xtensa-esp32s2-espidf/release/flipper-mcp
  ```
- **Corrupted flash state:** Erase first, then flash:
  ```bash
  espflash erase-flash
  # Re-enter bootloader mode, then:
  espflash flash target/xtensa-esp32s2-espidf/release/flipper-mcp
  ```

### `Error while connecting to device`

The board is not in bootloader mode. The `/dev/ttyACM0` device you see is
from the running firmware's USB CDC console, not the ROM bootloader.

**Fix:** Follow the bootloader entry steps above. Verify with:
```bash
dmesg | tail -5
# Firmware running (WRONG for flashing): "Product: ESP32-S2"
# Bootloader mode (CORRECT): different descriptor or no "Product: ESP32-S2"
```

### `espflash` retries with `UsbJtagSerial reset strategy` and fails

```
Using UsbJtagSerial reset strategy
Failed to reset, error Connection(Error { kind: Unknown, description: "Protocol error" })
```

This happens when `espflash` misidentifies the ESP32-S2 as an ESP32-S3/C3
(which have USB-JTAG-Serial, a different USB peripheral). The S2 has USB-OTG
instead. Workaround:
```bash
espflash flash --before default-reset \
  target/xtensa-esp32s2-espidf/release/flipper-mcp
```
Or enter bootloader mode manually and use `--before no-reset`:
```bash
# After entering bootloader with BOOT button:
espflash flash --before no-reset \
  target/xtensa-esp32s2-espidf/release/flipper-mcp
```

### Interactive prompts cause bootloader to time out

`espflash` asks "Use serial port?" and "Remember?" on the first run. By the
time you answer, the bootloader has timed out. Enter bootloader mode again
immediately before running the flash command. Subsequent runs skip the prompts
(port is remembered).

### `espflash: command not found`
```bash
cargo +stable install espflash
```
After install, `espflash` is in `~/.cargo/bin`. Either use the full path:
```bash
~/.cargo/bin/espflash flash ...
```
Or add `~/.cargo/bin` to your PATH permanently:
```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### `No such file or directory` when specifying the binary path

The workspace puts build artifacts under the **workspace root** `target/`, not
`firmware/target/`. Always use:
```bash
# From workspace root:
target/xtensa-esp32s2-espidf/release/flipper-mcp

# Or the absolute path:
/home/you/Code/flipper-mcp/target/xtensa-esp32s2-espidf/release/flipper-mcp
```
`cargo run --release` (from `firmware/`) handles this automatically via the
`.cargo/config.toml` runner.

### `Permission denied` on `/dev/ttyACM0`
```bash
sudo usermod -a -G dialout $USER
# Log out and back in for group change to take effect
```

### ModemManager interference

On many Linux distros, **ModemManager** probes new USB CDC devices by sending
AT commands. This confuses the ESP32 and can cause flash failures or serial
monitor disconnections.

```bash
sudo systemctl stop ModemManager
sudo systemctl disable ModemManager  # prevent it from starting on reboot
```
Then unplug and re-plug the USB cable.

### Quick reference: flash cheat sheet

```bash
# 1. Remove board from Flipper
# 2. Hold BOOT, plug USB, release BOOT
# 3. Flash:
espflash flash target/xtensa-esp32s2-espidf/release/flipper-mcp

# If "Communication error":
espflash flash --no-stub target/xtensa-esp32s2-espidf/release/flipper-mcp

# If "Error while connecting":
#   → Board is NOT in bootloader mode. Redo step 2.

# If "UsbJtagSerial" errors:
espflash flash --before no-reset target/xtensa-esp32s2-espidf/release/flipper-mcp

# Nuclear option — erase everything and start fresh:
espflash erase-flash
# Re-enter bootloader, then flash. WiFi config in NVS will be lost.
```

---

## WiFi Issues

### Debugging WiFi with the serial monitor

The USB serial monitor is the best tool for diagnosing WiFi problems. It shows
ESP-IDF's internal WiFi driver logs with details not available through the FAP.

```bash
# Connect USB to the WiFi Dev Board (can be on the Flipper at the same time)
picocom -b 115200 /dev/ttyACM0
```

See [SETUP.md — Verifying WiFi with Serial Monitor](SETUP.md#verifying-wifi-with-serial-monitor)
for full setup instructions. Key things to look for:

| Serial output | Meaning |
|----------------|---------|
| `WiFi started` then nothing | Radio started but can't find the AP — wrong SSID or out of range |
| `WiFi connect failed: ESP_ERR_TIMEOUT` | AP found but handshake timed out — wrong password, auth mismatch, or weak signal |
| `WiFi connect failed: ESP_ERR_WIFI_SSID` | SSID not found in scan results |
| `WiFi connected — IP: x.x.x.x` | Success — proceed to test the MCP server |

### ESP32 stuck in "needs_config" loop
No WiFi credentials found in NVS. Create `config.txt` on the Flipper's SD card:
1. Use the FAP: **Apps → Tools → Flipper MCP → Load SD Config**
2. Or: **Configure WiFi** to enter credentials via on-screen keyboard
3. Select **Reboot Board** to apply

### WiFi connection times out (`ESP_ERR_TIMEOUT`)

The ESP32 can see the network but can't complete the WPA handshake. Try these
in order:

1. **Check the password** — SSIDs and passwords are case-sensitive
2. **Verify 2.4 GHz** — the ESP32-S2 does NOT support 5 GHz. If your router
   uses a combined SSID, set up a separate 2.4 GHz-only network
3. **Try a different `wifi_auth` value** — add `wifi_auth=wpa2wpa3` to
   config.txt, then Load SD Config + Reboot Board
4. **Test with a phone hotspot** — create a 2.4 GHz hotspot with a simple
   SSID (no spaces), WPA2, and a short password. This isolates router issues
5. **Erase flash and reflash** — clears stale NVS data from previous firmware:
   ```bash
   espflash erase-flash
   # Re-enter bootloader, then:
   espflash flash --no-stub target/xtensa-esp32s2-espidf/release/flipper-mcp
   ```
   You'll need to re-send WiFi config via Load SD Config after erasing

### Device connected to WiFi but can't be reached

```bash
# Ping the ESP32's IP (shown on FAP Status or serial monitor)
ping 192.168.x.xxx

# If "Destination Host Unreachable":
# → AP/client isolation is enabled on your router, OR
# → The WiFi connection dropped after the initial connect
```

**Fix:** Check your router's settings for "AP isolation", "client isolation",
or "wireless isolation" and disable it. Some guest networks have this enabled
by default.

If your PC is on Ethernet and the ESP32 is on WiFi, some routers don't bridge
between wired and wireless. Try accessing from another WiFi device instead.

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

### FAP Status shows "No status yet" with rx_bytes: 0

No bytes are being received from the ESP32 over UART. Check in order:

1. **WiFi Dev Board is seated on the Flipper's GPIO header.** The UART pins
   (GPIO43 TX → Flipper Pin 14 RX, GPIO44 RX ← Flipper Pin 13 TX) are
   only connected when the board is physically attached.

2. **The Flipper MCP FAP is running.** The FAP takes exclusive control of the
   UART when it starts (`expansion_disable()` + `furi_hal_serial_control_acquire`).
   The ESP32 can only communicate when the FAP is open.

3. **ESP32 firmware is flashed and running.** If you just flashed, make sure
   the board is powered (either from Flipper GPIO or USB). Check the
   Flipper's battery — low battery may not supply enough power.

4. **Expansion Modules is set to None.** This is the **#1 gotcha**. On the
   Flipper, go to **Settings → System → Expansion Modules → None**. If
   enabled, the expansion protocol handler intercepts all UART data before
   the FAP can read it.

### FAP Status shows rx_bytes > 0 but "No status yet"

Bytes are arriving but no STATUS messages are being parsed. This means the
ESP32 is sending data but it's not in the expected protocol format. Possible
causes:

- **Wrong firmware flashed.** The ESP32 must be running the Flipper MCP
  firmware (not BlackMagic, Marauder, or stock firmware).
- **Baud rate mismatch.** Both sides must use 115200 baud.
- **Garbage data.** Check the `last:` line on the Status screen for the last
  raw line received. If it's garbled, it's likely a baud rate or electrical
  issue.

### FAP Status shows "needs_config"

The ESP32 booted but has no WiFi credentials in NVS. Use one of:
- **Load SD Config** — reads `config.txt` from SD and sends it via UART
- **Configure WiFi** — enter credentials via the on-screen keyboard
- Then select **Reboot Board** to apply

### FAP Status shows "connecting_wifi" or "wifi_error"

The ESP32 is trying to connect to WiFi but failing. Common causes:
- Wrong SSID or password (SSIDs are case-sensitive)
- Network is 5 GHz only (ESP32-S2 only supports 2.4 GHz)
- Router is out of range
- Too many clients on the network

Use **View Logs** for more detail on the WiFi error. You can send new
credentials via **Load SD Config** or **Configure WiFi** while the ESP32
is in the retry loop — it accepts CONFIG messages during WiFi retry.

### "No ACK" after sending a command

The FAP sends a CMD over UART and waits up to 6 seconds for an ACK response
from the ESP32. If no ACK arrives:

- **Reboot command:** A brief "No ACK" is normal — the ESP32 restarts
  immediately after sending the ACK, and the UART bytes may not reach the FAP
  in time. Wait 10–30 seconds, then check Status again.
- **Other commands:** The ESP32 may not be in the main loop yet (still
  connecting to WiFi). Commands sent during WiFi retry get responses like
  `err:wifi_not_connected`. Check View Logs for details.

### Expansion Modules setting (detailed)

Flipper firmware 0.97.0+ has an Expansion Modules feature that listens on the
UART expansion port for the expansion protocol handshake. When enabled, it
intercepts **all** UART data, preventing both the ESP32 and the FAP from
communicating.

The FAP disables the expansion handler on startup (`expansion_disable()`),
but this only works if the setting is **None**. If set to "Listen UART USART"
or "LPUART", the handler may re-engage.

| Setting | Effect |
|---------|--------|
| **None** | UART free for FAP to use — **required** |
| Listen UART USART | Expansion protocol intercepts UART — breaks this project |
| LPUART | Expansion protocol on low-power UART — also breaks this project |

### ESP32 reboots but FAP shows stale data

When you close and reopen the FAP, all UART buffers are reset (rx_bytes
returns to 0). This is expected — the FAP only accumulates data while it's
running. Wait 5–30 seconds after opening the FAP for the ESP32's periodic
STATUS push to arrive.

---

## Serial Monitor Issues (Linux)

### Recommended serial monitor tools

The ESP32-S2 uses **USB CDC** for console output (not a UART bridge). Use a
plain serial terminal — `espflash monitor` does not work reliably:

```bash
# Recommended:
picocom -b 115200 /dev/ttyACM0

# Alternatives:
screen /dev/ttyACM0 115200
minicom -D /dev/ttyACM0 -b 115200
```

> **Note:** Baud rate doesn't technically matter for USB CDC (it's native USB),
> but the tools require a value. Use 115200 for convention.

### `FATAL: read zero bytes from port` / `term_exitfunc: reset failed`

This happens when the USB CDC device disappears — typically because the ESP32
reset (RESET button pressed, power cycle, or crash). During reset, the USB
device is momentarily disconnected.

**Fix:** This is expected behavior. After the reset, wait 2–3 seconds for the
board to re-enumerate, then reconnect:
```bash
picocom -b 115200 /dev/ttyACM0
```

### `screen` or `espflash monitor` immediately terminates on `/dev/ttyACM0`

**ModemManager** (installed by default on many Linux distros) probes new USB CDC
devices by sending AT commands. This confuses the ESP32 and can kill the
connection.

**Fix:**
```bash
sudo systemctl stop ModemManager
# To prevent it from starting on reboot:
sudo systemctl disable ModemManager
```

Then unplug and re-plug the USB cable (or reset the board) and try again.

### `espflash monitor` shows `Communication error` or `Protocol error`

`espflash monitor` tries to use the flash stub protocol, which doesn't work
with ESP32-S2 USB-OTG. Use `picocom`, `screen`, or `minicom` instead (see
above).

### No output on serial monitor

If the serial terminal connects but shows no output:

1. **Board may have already booted.** Press the RESET button while the terminal
   is open, or power-cycle the Flipper. The firmware waits 2 seconds at startup
   to allow USB CDC to enumerate.

2. **USB CDC not enabled.** Verify `CONFIG_ESP_CONSOLE_USB_CDC=y` is in
   `sdkconfig.defaults` and do a clean rebuild:
   ```bash
   cd firmware && cargo clean && cargo build --release --target xtensa-esp32s2-espidf
   ```

3. **Board is in bootloader mode.** If you held BOOT while plugging in,
   the firmware isn't running. Unplug and replug without holding BOOT.

### Serial monitor vs FAP UART

The USB serial monitor and the FAP communicate on **different channels**:

| Channel | Purpose | What you see |
|---------|---------|--------------|
| USB CDC (`/dev/ttyACM0`) | ESP-IDF console logs | `info!()`, `error!()`, WiFi driver output |
| UART0 (GPIO43/44) | FAP ↔ ESP32 protocol | STATUS, LOG, TOOLS, ACK, CMD messages |

FAP commands (Load SD Config, Reboot Board, etc.) go over UART and won't
appear on the USB serial monitor. However, the **effects** are logged — for
example, "FAP config: wifi_ssid set" appears on USB CDC when Load SD Config
is received over UART.

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

## Cloud Deployment (OpenTofu) Issues

### `tofu init` fails: `Failed to get existing workspaces: S3 bucket does not exist`
The state backend bucket hasn't been created yet. Run the bootstrap script first:
```bash
# AWS
./infra/bootstrap/aws.sh flipper-mcp-tfstate flipper-mcp-tflock us-east-1
# GCP
./infra/bootstrap/gcp.sh my-gcp-project my-project-tfstate us-central1
```
Then re-run `tofu init` with the `-backend-config` flags shown in `infra/*/terraform.tfvars.example`.

### `tofu apply` fails with `AccessDenied` / permission errors
The AWS IAM user or GCP service account running Tofu lacks required permissions.

**AWS minimum permissions:**
- `ec2:*`, `iam:*`, `route53:*`, `s3:*`, `dynamodb:*` (or use `AdministratorAccess` for initial setup, then lock down)

**GCP minimum roles:**
- `roles/compute.admin`, `roles/dns.admin`, `roles/storage.admin`, `roles/iam.serviceAccountAdmin`, `roles/iam.serviceAccountUser`

### Relay binary not found on first boot (`flipper-mcp-relay: not found`)
The cloud-init user-data runs at first boot and downloads the binary from S3/GCS. If the binary hasn't been uploaded yet, the download silently fails and the service won't start.

**Cause 1 — CI hasn't run yet.** Push a commit to `main` after setting up the GitHub secrets; the `publish-relay` job uploads the binary.

**Cause 2 — GitHub secrets not configured.** Check **Settings → Secrets and variables → Actions** and ensure `AWS_ARTIFACTS_BUCKET` / `GCP_ARTIFACTS_BUCKET` (and credentials) are set.

**Cause 3 — Manual upload needed.** You can upload directly:
```bash
# AWS
cargo build --release -p flipper-mcp-relay
aws s3 cp target/release/flipper-mcp-relay \
  s3://<artifacts-bucket>/relay/flipper-mcp-relay

# GCP
gcloud storage cp target/release/flipper-mcp-relay \
  gs://<artifacts-bucket>/relay/flipper-mcp-relay
```

After uploading, SSH into the server and run cloud-init again, or simply:
```bash
sudo aws s3 cp s3://<bucket>/relay/flipper-mcp-relay /usr/local/bin/flipper-mcp-relay
sudo chmod +x /usr/local/bin/flipper-mcp-relay
sudo systemctl start flipper-mcp-relay
```

### Caddy TLS certificate not provisioning

Caddy uses Let's Encrypt HTTP-01 challenge (port 80). If the cert isn't issuing:

1. **DNS not propagated yet.** After setting nameservers at your registrar, propagation can take up to an hour. Test with:
   ```bash
   dig +short relay.example.com     # should return your static IP
   curl http://relay.example.com/health   # should respond before TLS works
   ```

2. **Port 80 not reachable.** The AWS security group and GCP firewall both allow port 80. If you modified them, verify:
   ```bash
   # AWS
   aws ec2 describe-security-groups --filters Name=group-name,Values=flipper-mcp-relay
   # GCP
   gcloud compute firewall-rules describe flipper-mcp-relay-allow
   ```

3. **Caddy not running.** SSH in and check:
   ```bash
   sudo systemctl status caddy
   sudo journalctl -u caddy -n 50
   ```
   If Caddy failed to start because the relay service wasn't bound yet, restart it after the relay is up:
   ```bash
   sudo systemctl restart caddy
   ```

### Health check returns 502 Bad Gateway

Caddy is running but the relay process isn't bound on `localhost:9090`.
```bash
# SSH into server and check relay service
sudo systemctl status flipper-mcp-relay
sudo journalctl -u flipper-mcp-relay -n 50
```

Common causes: binary not downloaded (see above), or `ExecStart` path is wrong. Verify:
```bash
ls -la /usr/local/bin/flipper-mcp-relay
/usr/local/bin/flipper-mcp-relay --help
```

### `tofu apply` creates new instance instead of updating

`user_data_replace_on_change = true` (AWS) and `replace_triggered_by` (GCP) are set so that changing the cloud-init template forces instance replacement — cloud-init only runs on first boot. This is intentional. The new instance gets a new public IP, but the Elastic IP (AWS) or static address (GCP) is automatically reassociated.

### EC2 instance is `t3.micro` but my account only has `t2.micro` free tier

Older AWS accounts may only have `t2.micro` in the free tier. Change in `infra/aws/terraform.tfvars`:
```hcl
# Override instance type (set in compute.tf default)
```
Or directly edit `infra/aws/compute.tf` and change `instance_type = "t3.micro"` to `"t2.micro"`.

### GCP `e2-micro` free tier availability

The free tier `e2-micro` is available only in `us-central1`, `us-east1`, and `us-west1`. If you choose a different region, you'll be billed for the instance. Change `gcp_region` and `gcp_zone` in `terraform.tfvars` accordingly.

### SSH: `Permission denied (publickey)`

Verify the key in `terraform.tfvars` matches your local private key:
```bash
# AWS — check what key is registered
ssh-keygen -y -f ~/.ssh/id_ed25519   # should match ssh_public_key in tfvars

# GCP — check metadata
gcloud compute instances describe flipper-mcp-relay \
  --zone us-central1-a \
  --format='value(metadata.items[ssh-keys])'
```
If the key is wrong, update `terraform.tfvars` and run `tofu apply` (the instance will be replaced).

### CI `publish-relay` job: `Upload to S3` / `Upload to GCS` steps are skipped

The steps are skipped when the corresponding bucket secret is empty. Check:
1. The secret name is exactly `AWS_ARTIFACTS_BUCKET` (or `GCP_ARTIFACTS_BUCKET`)
2. The secret is set at the **repository** level (not environment level), unless you've configured environment-scoped secrets
3. The job only runs on push to `main` — PRs won't trigger it

---

## Memory / Stack Issues

### Panic: `stack overflow`
The ESP32-S2 has limited stack space per thread. If a handler panics with a stack overflow:
- The HTTP server handler stack is set to 10240 bytes — increase in `streamable.rs` `Configuration::stack_size`
- The tunnel thread uses 10240 bytes — increase `TUNNEL_STACK_SIZE` in `tunnel/client.rs`

The main task uses 20000 bytes (set in `sdkconfig.defaults` as `CONFIG_ESP_MAIN_TASK_STACK_SIZE`).
