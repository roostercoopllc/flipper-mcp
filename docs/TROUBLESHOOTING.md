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
The WiFi Module v1 (ESP32-S2) uses **native USB CDC** and only enumerates when in bootloader
mode. The running firmware uses UART (not USB), so the board won't appear as a serial port
during normal operation.

**Step 1 — Enter bootloader mode:**
1. Connect the WiFi Module's USB-C to your PC (not the Flipper's port)
2. Hold the **BOOT** button on the module
3. Briefly tap **RESET**, then release **BOOT**
4. The board should now appear as `/dev/ttyACM0`

Verify:
```bash
lsusb | grep -i espressif   # Should show: 303a:0002 Espressif
ls /dev/ttyACM*             # Should show: /dev/ttyACM0
```

**Step 2 — Flash immediately** (the bootloader has a short idle timeout):
```bash
~/.cargo/bin/espflash flash --no-stub --port /dev/ttyACM0 \
  /home/atilla/Code/flipper-mcp/target/xtensa-esp32s2-espidf/release/flipper-mcp
```

If you need to add yourself to the `dialout` group (required on some distros):
```bash
sudo usermod -a -G dialout $USER
# Log out and back in for group to take effect
```

### `Communication error while flashing device` (flash stub failure)
The default flash stub is sometimes incompatible. Use `--no-stub` to bypass it:
```bash
~/.cargo/bin/espflash flash --no-stub --port /dev/ttyACM0 \
  /home/atilla/Code/flipper-mcp/target/xtensa-esp32s2-espidf/release/flipper-mcp
```
If that still fails, try erasing flash first:
```bash
~/.cargo/bin/espflash erase-flash --no-stub --port /dev/ttyACM0
# then flash again
```

### Interactive prompts cause bootloader to time out
`espflash` asks "Use serial port?" and "Remember?" on the first run. By the time you answer,
the bootloader has timed out. Do BOOT+RESET again immediately before running the flash command.
Subsequent runs skip the prompts (port is remembered), so only the first flash is affected.

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
The workspace puts build artifacts under the **workspace root** `target/`, not `firmware/target/`.
Always use the full path:
```bash
/home/atilla/Code/flipper-mcp/target/xtensa-esp32s2-espidf/release/flipper-mcp
# or from the workspace root:
target/xtensa-esp32s2-espidf/release/flipper-mcp
```
`cargo run --release` (from `firmware/`) handles this automatically via the `.cargo/config.toml` runner.

---

## WiFi Issues

### ESP32 stuck in "needs_config" loop
No WiFi credentials found. Create `config.txt` on the Flipper's SD card:
1. Use the FAP: **Apps → Tools → Flipper MCP → Configure WiFi**
2. Or manually create `/ext/apps_data/flipper_mcp/config.txt` with `wifi_ssid=...` and `wifi_password=...`
3. Reboot the ESP32 after saving

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

### "No status file" despite ESP32 running — Expansion Modules setting

**This is the #1 gotcha.** Flipper firmware 0.97.0+ has an Expansion Modules
feature that listens on the UART expansion port for the expansion protocol
handshake. When enabled, it intercepts **all** UART data before the CLI shell
sees it, so the ESP32's commands are silently dropped.

**Fix:** On the Flipper, go to **Settings → System → Expansion Modules** and
set it to **None**.

Options you may see (varies by firmware version):
| Setting | Effect |
|---------|--------|
| **None** | UART passed straight to CLI — **use this** |
| Listen UART USART | Expansion protocol intercepts UART — breaks this project |
| LPUART | Expansion protocol on low-power UART — also breaks this project |

After changing the setting, reboot the ESP32 (or use the FAP's Reboot Board
option). The status file should appear within a few seconds.

### `UART smoke tests failed` or tools returning empty output
Check the physical connection between the WiFi Dev Board and Flipper Zero:
- The GPIO header must be fully seated
- Flipper must be powered on
- Flipper CLI must be accessible (not in a full-screen app)
- **Expansion Modules must be set to None** (see above)

### Commands timeout (`execute_command: timeout`)
- Default timeout is 500ms — some commands (like `subghz rx`) take longer
- The Flipper might be running a full-screen app that blocks the CLI
- Try restarting the Flipper

### `Storage error: File not found` for SD card config
The SD card isn't inserted, or the path doesn't exist. The firmware enters the
"waiting for config" loop until `config.txt` is created.

---

## Serial Monitor Issues (Linux)

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

### `espflash monitor` shows `Protocol error` after RESET

`espflash monitor` expects the ESP-IDF boot stub protocol. On the ESP32-S2 with
USB CDC, the running firmware doesn't speak this protocol. Use a plain serial
terminal instead:
```bash
screen /dev/ttyACM0 115200
# Press Ctrl+A then K to exit screen
```

Note: USB CDC console output requires `CONFIG_ESP_CONSOLE_USB_CDC=y` in
`sdkconfig.defaults` and a clean rebuild (`cargo clean`).

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
