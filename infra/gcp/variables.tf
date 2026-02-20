variable "gcp_project" {
  description = "GCP project ID"
  type        = string
}

variable "gcp_region" {
  description = "GCP region to deploy into"
  type        = string
  default     = "us-central1"
}

variable "gcp_zone" {
  description = "GCP zone for the Compute Engine instance"
  type        = string
  default     = "us-central1-a"
}

variable "dns_zone_name" {
  description = "Cloud DNS managed zone resource name (e.g., 'example-com'). Used as the GCP resource identifier."
  type        = string
}

variable "dns_domain" {
  description = "Apex domain for the Cloud DNS zone, with trailing dot (e.g., 'example.com.')."
  type        = string
}

variable "relay_subdomain" {
  description = "Subdomain for the relay server. Relay will be reachable at <relay_subdomain>.<dns_domain (without dot)>."
  type        = string
  default     = "relay"
}

variable "ssh_public_key" {
  description = "SSH public key in GCP metadata format: 'username:ssh-ed25519 AAAA...' (e.g., 'ubuntu:ssh-ed25519 AAAA...')"
  type        = string
}

variable "artifacts_bucket" {
  description = "Name of the GCS bucket where CI uploads the relay binary (e.g., 'myorg-flipper-mcp-artifacts')."
  type        = string
}

variable "relay_port" {
  description = "Internal port the flipper-mcp-relay process listens on (Caddy proxies to this)."
  type        = number
  default     = 9090
}
