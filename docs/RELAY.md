# Relay Server

The relay enables cross-network access — reach your Flipper Zero from a VPS, home lab, or anywhere.

## Architecture

```
Flipper → ESP32-S2 ──WebSocket──► Relay Server ◄──HTTP── Claude / MCP client
                      (persistent)    (public IP)
```

The device initiates the outbound WebSocket connection. The relay proxies incoming MCP HTTP requests to the device over that connection. No port forwarding on the Flipper's local network is required.

---

## Build the Relay

```bash
./scripts/build-relay.sh
# Binary: ./target/release/flipper-mcp-relay
```

Or manually:
```bash
cargo build --release -p flipper-mcp-relay
```

---

## Run the Relay

```bash
# Listen on all interfaces, port 9090
./target/release/flipper-mcp-relay --listen 0.0.0.0:9090

# Or install and run as a system service
sudo cp ./target/release/flipper-mcp-relay /usr/local/bin/
flipper-mcp-relay --listen 0.0.0.0:9090
```

Endpoints:
- `ws://<host>:9090/tunnel` — Device WebSocket connection
- `http://<host>:9090/mcp` — MCP clients POST here
- `http://<host>:9090/health` — Health check + connected device info
- `http://<host>:9090/sse` — Legacy SSE (GET)
- `http://<host>:9090/messages` — Legacy SSE messages (POST)

---

## Configure the Device

### Via wifi-config.sh

```bash
# Plain relay (no TLS)
./scripts/wifi-config.sh \
    --ssid YourWiFi \
    --password YourPassword \
    --relay ws://your-server.example.com:9090/tunnel

# Cloud relay with TLS (from tofu output)
./scripts/wifi-config.sh \
    --ssid YourWiFi \
    --password YourPassword \
    --relay wss://relay.example.com/tunnel
```

### Via SD card config

Add to `/ext/apps_data/flipper_mcp/config.txt`:
```
relay_url=wss://relay.example.com/tunnel
```

### Via AP captive portal

The portal doesn't currently have a relay URL field. Use one of the above methods instead.

---

## Cloud Deployment (OpenTofu)

The `infra/` directory contains [OpenTofu](https://opentofu.org/) configurations that provision a production relay on AWS or GCP with a single `tofu apply`. Provisioned resources include a VM, static IP, DNS zone, S3/GCS artifact bucket, Caddy TLS termination, and a systemd service — the relay will be reachable at `wss://relay.example.com/tunnel` on first boot.

### Prerequisites

- [OpenTofu](https://opentofu.org/docs/intro/install/) ≥ 1.6 — `brew install opentofu` or package manager
- **AWS:** AWS CLI configured (`aws configure`) with permissions to create EC2, S3, IAM, Route53, DynamoDB resources
- **GCP:** `gcloud` CLI authenticated (`gcloud auth application-default login`) with editor access to a project

### AWS — first deploy

```bash
# 1. Create state bucket + DynamoDB lock table (run once)
./infra/bootstrap/aws.sh flipper-mcp-tfstate flipper-mcp-tflock us-east-1

# 2. Configure
cd infra/aws
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars — set dns_zone, ssh_public_key, artifacts_bucket

# 3. Initialise and apply
tofu init \
  -backend-config="bucket=flipper-mcp-tfstate" \
  -backend-config="region=us-east-1" \
  -backend-config="dynamodb_table=flipper-mcp-tflock"
tofu plan
tofu apply
```

After apply, update your domain registrar's nameservers to the values from `tofu output zone_nameservers`. DNS propagation usually takes a few minutes to an hour.

### GCP — first deploy

```bash
# 1. Create GCS state bucket (run once)
./infra/bootstrap/gcp.sh my-gcp-project my-project-tfstate us-central1

# 2. Configure
cd infra/gcp
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars — set gcp_project, dns_domain, ssh_public_key, artifacts_bucket

# 3. Initialise and apply
tofu init -backend-config="bucket=my-project-tfstate"
tofu plan
tofu apply
```

### Outputs

Both configurations produce the same outputs:

```bash
tofu output relay_url          # wss://relay.example.com/tunnel  (device config)
tofu output relay_health_url   # https://relay.example.com/health
tofu output zone_nameservers   # NS records to set at your registrar
tofu output ssh_command        # ssh ubuntu@<static-ip>
tofu output wifi_config_cmd    # ready-to-paste ./scripts/wifi-config.sh ... command
```

### Connect the device

Once DNS has propagated and Caddy has provisioned a TLS certificate:

```bash
# Use the ready-made command from tofu outputs
$(cd infra/aws && tofu output -raw wifi_config_cmd)

# Or manually:
./scripts/wifi-config.sh \
  --ssid YourSSID \
  --password YourPass \
  --relay wss://relay.example.com/tunnel
```

Verify the relay is reachable before flashing:
```bash
curl https://relay.example.com/health
# {"status":"ok","device_connected":false}  ← normal before device connects
```

### CI binary publishing

On every push to `main`, GitHub Actions builds the relay binary and uploads it to the cloud storage bucket. The VM downloads the binary at first boot.

Set these secrets in your repo (**Settings → Secrets and variables → Actions**):

| Secret | Purpose |
|--------|---------|
| `AWS_ACCESS_KEY_ID` | S3 upload |
| `AWS_SECRET_ACCESS_KEY` | S3 upload |
| `AWS_DEFAULT_REGION` | S3 upload |
| `AWS_ARTIFACTS_BUCKET` | S3 bucket name |
| `GCP_SA_KEY` | GCS upload — base64-encoded service account JSON with `storage.objectAdmin` on the artifacts bucket |
| `GCP_ARTIFACTS_BUCKET` | GCS bucket name |

Only configure the cloud(s) you use — the upload steps are skipped automatically if the corresponding secrets are absent.

> **Bootstrap note:** the bucket must exist before the first CI run uploads to it. Run `tofu apply` first, which creates the artifacts bucket; after that CI runs normally.

---

## Run as a systemd Service

Create `/etc/systemd/system/flipper-mcp-relay.service`:

```ini
[Unit]
Description=Flipper MCP Relay Server
After=network-online.target

[Service]
Type=simple
User=nobody
ExecStart=/usr/local/bin/flipper-mcp-relay --listen 0.0.0.0:9090
Restart=on-failure
RestartSec=5s
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now flipper-mcp-relay
sudo journalctl -u flipper-mcp-relay -f
```

---

## Run with Docker

Create `Dockerfile.relay`:
```dockerfile
FROM debian:bookworm-slim
COPY target/release/flipper-mcp-relay /usr/local/bin/
RUN chmod +x /usr/local/bin/flipper-mcp-relay
EXPOSE 9090
CMD ["flipper-mcp-relay", "--listen", "0.0.0.0:9090"]
```

```bash
./scripts/build-relay.sh
docker build -f Dockerfile.relay -t flipper-mcp-relay .
docker run -p 9090:9090 flipper-mcp-relay
```

---

## Enable the Tunnel in Firmware

The WebSocket client requires the `espressif/esp_websocket_client` managed component.

Add to `firmware/idf_component.yml`:
```yaml
dependencies:
  idf: ">=5.2.0"
  espressif/esp_websocket_client: ">=1.1.0"
```

Rebuild (requires internet to download the component on first build):
```bash
cd firmware && cargo clean && cargo build --release --target xtensa-esp32s2-espidf
```

Without this component, the device logs:
```
Tunnel component not built — add espressif/esp_websocket_client to idf_component.yml
```
and works fine for local access.

---

## Security Notes

- The relay has **no authentication**. Anyone who can reach the relay endpoint can use your Flipper.
- **Cloud deployments** (via `infra/`) expose only ports 22, 80, 443. Port 9090 is bound to `localhost` only — Caddy proxies it with TLS. This is the recommended setup.
- **Self-hosted deployments** on a raw VPS: firewall port 9090 and wrap with Caddy or nginx for TLS.
- For a VPN-based setup: run the relay on the VPN server and configure the device's `relay_url` to the VPN IP.
- If you need authentication, put an `basicauth` or `forward_auth` directive in the Caddyfile before the `reverse_proxy` line.

---

## Single-Device Model

The current relay supports one device at a time. If a second device connects, it replaces the first (the first device's WebSocket connection is dropped). Multi-device support is a planned future enhancement.
