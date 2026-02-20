#cloud-config
# flipper-mcp relay server — GCP cloud-init
# Runs on first boot. Installs gcloud CLI, downloads the relay binary from GCS,
# sets up Caddy (TLS), and creates a systemd service.

package_update: true
package_upgrade: true

packages:
  - ufw
  - curl
  - gnupg
  - apt-transport-https
  - ca-certificates

runcmd:
  # ── Install Google Cloud CLI ───────────────────────────────────────────────
  - curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /etc/apt/keyrings/cloud.google.gpg
  - >
    echo "deb [signed-by=/etc/apt/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main"
    > /etc/apt/sources.list.d/google-cloud-sdk.list
  - apt-get update -qq
  - apt-get install -y google-cloud-cli

  # ── Download relay binary from GCS using VM service account ───────────────
  - >
    gcloud storage cp
    gs://${artifacts_bucket}/relay/flipper-mcp-relay
    /usr/local/bin/flipper-mcp-relay
    --project=${gcp_project}
  - chmod +x /usr/local/bin/flipper-mcp-relay

  # ── Install Caddy from official apt repo ──────────────────────────────────
  - curl -fsSL https://apt.fury.io/caddy/pubring.gpg | gpg --dearmor -o /etc/apt/keyrings/caddy.gpg
  - >
    echo "deb [signed-by=/etc/apt/keyrings/caddy.gpg] https://apt.fury.io/caddy/ stable main"
    > /etc/apt/sources.list.d/caddy.list
  - apt-get update -qq
  - apt-get install -y caddy

  # ── Configure Caddy ────────────────────────────────────────────────────────
  - |
    cat > /etc/caddy/Caddyfile << 'CADDY'
    ${relay_domain} {
        reverse_proxy localhost:${relay_port}
    }
    CADDY

  # ── Create relay systemd service ───────────────────────────────────────────
  - |
    cat > /etc/systemd/system/flipper-mcp-relay.service << 'UNIT'
    [Unit]
    Description=Flipper MCP Relay Server
    After=network-online.target
    Wants=network-online.target

    [Service]
    ExecStart=/usr/local/bin/flipper-mcp-relay --listen 0.0.0.0:${relay_port}
    Restart=always
    RestartSec=5
    User=nobody
    AmbientCapabilities=
    NoNewPrivileges=true

    [Install]
    WantedBy=multi-user.target
    UNIT

  - systemctl daemon-reload
  - systemctl enable flipper-mcp-relay
  - systemctl start flipper-mcp-relay
  - systemctl enable caddy
  - systemctl restart caddy

  # ── Firewall: allow only SSH, HTTP (ACME), HTTPS ───────────────────────────
  - ufw default deny incoming
  - ufw default allow outgoing
  - ufw allow 22/tcp
  - ufw allow 80/tcp
  - ufw allow 443/tcp
  - ufw --force enable
