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
./scripts/wifi-config.sh \
    --ssid YourWiFi \
    --password YourPassword \
    --relay ws://your-server.example.com:9090/tunnel
```

### Via SD card config

Add to `/ext/apps_data/flipper_mcp/config.txt`:
```
relay_url=ws://your-server.example.com:9090/tunnel
```

### Via AP captive portal

The portal doesn't currently have a relay URL field. Use one of the above methods instead.

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

- The relay has **no authentication**. Anyone who can reach port 9090 can use your Flipper.
- Recommended: firewall port 9090, use a VPN, or put the relay behind a reverse proxy (nginx/Caddy) with authentication.
- For a VPN-based setup: run the relay on the VPN server and configure the device's `relay_url` to the VPN IP.
- TLS: wrap with Caddy (`caddy reverse-proxy --from wss://…`) for encrypted WebSocket (wss://).

---

## Single-Device Model

The current relay supports one device at a time. If a second device connects, it replaces the first (the first device's WebSocket connection is dropped). Multi-device support is a planned future enhancement.
