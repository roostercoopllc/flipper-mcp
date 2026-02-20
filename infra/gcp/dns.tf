# ── Cloud DNS managed zone ─────────────────────────────────────────────────────
resource "google_dns_managed_zone" "relay" {
  name        = var.dns_zone_name
  dns_name    = var.dns_domain
  description = "flipper-mcp relay"
}

# ── A record: relay.<domain> → static IP ──────────────────────────────────────
resource "google_dns_record_set" "relay" {
  name         = "${local.relay_domain}."
  type         = "A"
  ttl          = 300
  managed_zone = google_dns_managed_zone.relay.name
  rrdatas      = [google_compute_address.relay.address]
}
