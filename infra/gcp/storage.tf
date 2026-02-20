# ── GCS artifacts bucket ───────────────────────────────────────────────────────
# CI uploads the relay binary here; the Compute Engine VM downloads it at boot.

resource "google_storage_bucket" "artifacts" {
  name                        = var.artifacts_bucket
  location                    = var.gcp_region
  uniform_bucket_level_access = true
  force_destroy               = false

  versioning {
    enabled = true
  }
}
