#!/usr/bin/env bash
# Bootstrap: Create GCS bucket for OpenTofu remote state.
# Run once before `tofu init` in infra/gcp/.
#
# Usage:
#   ./infra/bootstrap/gcp.sh [project] [state-bucket] [region]
#
# Defaults:
#   project      : current gcloud project
#   state-bucket : <project>-flipper-mcp-tfstate
#   region       : us-central1
set -euo pipefail

PROJECT="${1:-$(gcloud config get-value project 2>/dev/null)}"
if [[ -z "$PROJECT" ]]; then
  echo "ERROR: GCP project not set. Pass it as the first argument or run:"
  echo "  gcloud config set project YOUR_PROJECT_ID"
  exit 1
fi

BUCKET="${2:-${PROJECT}-flipper-mcp-tfstate}"
REGION="${3:-us-central1}"

echo "==> Bootstrapping GCP remote state"
echo "    Project : $PROJECT"
echo "    Bucket  : gs://$BUCKET"
echo "    Region  : $REGION"
echo

# ── GCS bucket ────────────────────────────────────────────────────────────────
if gcloud storage buckets describe "gs://$BUCKET" --project="$PROJECT" &>/dev/null; then
  echo "[skip] GCS bucket 'gs://$BUCKET' already exists"
else
  gcloud storage buckets create "gs://$BUCKET" \
    --project="$PROJECT" \
    --location="$REGION" \
    --uniform-bucket-level-access \
    --no-public-access-prevention
  echo "[ok]   Created GCS bucket: gs://$BUCKET"
fi

gcloud storage buckets update "gs://$BUCKET" --versioning
echo "[ok]   Versioning enabled"

echo
echo "Bootstrap complete. Next steps:"
echo
echo "  cd infra/gcp"
echo "  cp terraform.tfvars.example terraform.tfvars   # fill in your values"
echo "  tofu init -backend-config=\"bucket=$BUCKET\""
echo "  tofu plan"
echo "  tofu apply"
