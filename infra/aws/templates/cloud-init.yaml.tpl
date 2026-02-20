#cloud-config
# flipper-mcp relay server — AWS cloud-init
# Runs on first boot. Installs the relay binary from S3, sets up Caddy (TLS),
# and creates a systemd service.

package_update: true
package_upgrade: true

packages:
  - awscli
  - ufw
  - curl
  - gnupg

runcmd:
  # ── Download relay binary from S3 using EC2 instance profile ──────────────
  - >
    aws s3 cp
    s3://${artifacts_bucket}/relay/flipper-mcp-relay
    /usr/local/bin/flipper-mcp-relay
    --region ${aws_region}
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
