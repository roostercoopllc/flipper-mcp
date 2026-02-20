terraform {
  required_version = ">= 1.6"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 6.0"
    }
  }

  # State is stored in GCS.
  # Backend config is supplied at `tofu init` time via -backend-config flags
  # (see infra/bootstrap/gcp.sh for bucket creation).
  backend "gcs" {
    prefix = "relay/terraform/state"
    # bucket passed via -backend-config or GOOGLE_BACKEND_CREDENTIALS env
  }
}

provider "google" {
  project = var.gcp_project
  region  = var.gcp_region
}
