#!/usr/bin/env bash
# Bootstrap: Create S3 state bucket + DynamoDB lock table for OpenTofu remote state.
# Run once before `tofu init` in infra/aws/.
#
# Usage:
#   ./infra/bootstrap/aws.sh [state-bucket] [lock-table] [region]
#
# Defaults:
#   state-bucket : flipper-mcp-tfstate
#   lock-table   : flipper-mcp-tflock
#   region       : us-east-1  (or $AWS_DEFAULT_REGION)
set -euo pipefail

BUCKET="${1:-flipper-mcp-tfstate}"
TABLE="${2:-flipper-mcp-tflock}"
REGION="${3:-${AWS_DEFAULT_REGION:-us-east-1}}"

echo "==> Bootstrapping AWS remote state"
echo "    Bucket : s3://$BUCKET"
echo "    Table  : $TABLE"
echo "    Region : $REGION"
echo

# ── S3 bucket ──────────────────────────────────────────────────────────────────
if aws s3api head-bucket --bucket "$BUCKET" 2>/dev/null; then
  echo "[skip] S3 bucket '$BUCKET' already exists"
else
  if [[ "$REGION" == "us-east-1" ]]; then
    aws s3api create-bucket \
      --bucket "$BUCKET" \
      --region "$REGION"
  else
    aws s3api create-bucket \
      --bucket "$BUCKET" \
      --region "$REGION" \
      --create-bucket-configuration "LocationConstraint=$REGION"
  fi
  echo "[ok]   Created S3 bucket: $BUCKET"
fi

aws s3api put-bucket-versioning \
  --bucket "$BUCKET" \
  --versioning-configuration Status=Enabled
echo "[ok]   Versioning enabled"

aws s3api put-bucket-encryption \
  --bucket "$BUCKET" \
  --server-side-encryption-configuration '{
    "Rules": [{
      "ApplyServerSideEncryptionByDefault": {"SSEAlgorithm": "AES256"},
      "BucketKeyEnabled": true
    }]
  }'
echo "[ok]   AES-256 encryption enabled"

aws s3api put-public-access-block \
  --bucket "$BUCKET" \
  --public-access-block-configuration \
    "BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true"
echo "[ok]   Public access blocked"

# ── DynamoDB lock table ────────────────────────────────────────────────────────
if aws dynamodb describe-table --table-name "$TABLE" --region "$REGION" &>/dev/null; then
  echo "[skip] DynamoDB table '$TABLE' already exists"
else
  aws dynamodb create-table \
    --table-name "$TABLE" \
    --attribute-definitions AttributeName=LockID,AttributeType=S \
    --key-schema AttributeName=LockID,KeyType=HASH \
    --billing-mode PAY_PER_REQUEST \
    --region "$REGION" \
    --output text --query "TableDescription.TableName" > /dev/null
  echo "[ok]   Created DynamoDB table: $TABLE"
fi

echo
echo "Bootstrap complete. Next steps:"
echo
echo "  cd infra/aws"
echo "  cp terraform.tfvars.example terraform.tfvars   # fill in your values"
echo "  tofu init \\"
echo "    -backend-config=\"bucket=$BUCKET\" \\"
echo "    -backend-config=\"region=$REGION\" \\"
echo "    -backend-config=\"dynamodb_table=$TABLE\""
echo "  tofu plan"
echo "  tofu apply"
