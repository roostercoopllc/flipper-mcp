output "relay_ip" {
  description = "Static external IP of the relay server"
  value       = google_compute_address.relay.address
}

output "relay_url" {
  description = "WebSocket tunnel URL for the ESP32 device (pass to wifi-config.sh --relay)"
  value       = "wss://${local.relay_domain}/tunnel"
}

output "relay_health_url" {
  description = "Health check URL"
  value       = "https://${local.relay_domain}/health"
}

output "zone_nameservers" {
  description = "Cloud DNS NS records — update your domain registrar to delegate to these"
  value       = google_dns_managed_zone.relay.name_servers
}

output "ssh_command" {
  description = "SSH command to access the relay server"
  value       = "ssh ubuntu@${google_compute_address.relay.address}"
}

output "wifi_config_cmd" {
  description = "Ready-to-run wifi-config.sh command to configure the device"
  value       = "./scripts/wifi-config.sh --relay wss://${local.relay_domain}/tunnel"
}

output "artifacts_bucket" {
  description = "GCS bucket where CI should upload the relay binary"
  value       = google_storage_bucket.artifacts.name
}
