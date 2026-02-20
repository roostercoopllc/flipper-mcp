locals {
  # dns_domain has trailing dot (e.g., "example.com."); strip it for FQDN use
  dns_domain_clean = trimsuffix(var.dns_domain, ".")
  relay_domain     = "${var.relay_subdomain}.${local.dns_domain_clean}"
}

# ── Ubuntu 22.04 LTS image ─────────────────────────────────────────────────────
data "google_compute_image" "ubuntu" {
  family  = "ubuntu-2204-lts"
  project = "ubuntu-os-cloud"
}

# ── Service account for the relay VM ──────────────────────────────────────────
resource "google_service_account" "relay" {
  account_id   = "flipper-mcp-relay"
  display_name = "flipper-mcp relay server"
}

# Grant the SA objectViewer on the artifacts bucket only
resource "google_storage_bucket_iam_member" "relay_artifacts_read" {
  bucket = google_storage_bucket.artifacts.name
  role   = "roles/storage.objectViewer"
  member = "serviceAccount:${google_service_account.relay.email}"
}

# ── Static external IP ─────────────────────────────────────────────────────────
resource "google_compute_address" "relay" {
  name   = "flipper-mcp-relay"
  region = var.gcp_region
}

# ── Firewall rules ─────────────────────────────────────────────────────────────
resource "google_compute_firewall" "relay_allow" {
  name    = "flipper-mcp-relay-allow"
  network = "default"

  allow {
    protocol = "tcp"
    ports    = ["22", "80", "443"]
  }

  source_ranges = ["0.0.0.0/0"]
  target_tags   = ["flipper-mcp-relay"]
}

# ── Compute Engine VM ──────────────────────────────────────────────────────────
resource "google_compute_instance" "relay" {
  name         = "flipper-mcp-relay"
  machine_type = "e2-micro"
  zone         = var.gcp_zone

  tags = ["flipper-mcp-relay"]

  boot_disk {
    initialize_params {
      image = data.google_compute_image.ubuntu.self_link
      size  = 20
      type  = "pd-standard"
    }
  }

  network_interface {
    network = "default"
    access_config {
      nat_ip = google_compute_address.relay.address
    }
  }

  service_account {
    email  = google_service_account.relay.email
    scopes = ["cloud-platform"]
  }

  metadata = {
    # GCP cloud-init uses the 'user-data' metadata key
    user-data    = templatefile("${path.module}/templates/cloud-init.yaml.tpl", {
      relay_domain     = local.relay_domain
      relay_port       = var.relay_port
      artifacts_bucket = var.artifacts_bucket
      gcp_project      = var.gcp_project
    })
    ssh-keys = var.ssh_public_key
  }

  # Replace on user-data change (cloud-init runs only on first boot)
  lifecycle {
    replace_triggered_by = [
      terraform_data.cloud_init_hash
    ]
    create_before_destroy = true
  }
}

# Trigger replacement when cloud-init template changes
resource "terraform_data" "cloud_init_hash" {
  input = sha256(templatefile("${path.module}/templates/cloud-init.yaml.tpl", {
    relay_domain     = local.relay_domain
    relay_port       = var.relay_port
    artifacts_bucket = var.artifacts_bucket
    gcp_project      = var.gcp_project
  }))
}
