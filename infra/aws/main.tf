terraform {
  required_version = ">= 1.6"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  # State is stored in S3.
  # Backend config is supplied at `tofu init` time via -backend-config flags
  # (see infra/bootstrap/aws.sh for bucket/table creation).
  backend "s3" {
    key            = "relay/terraform.tfstate"
    encrypt        = true
    # bucket, region, dynamodb_table passed via -backend-config or env
  }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project   = "flipper-mcp"
      ManagedBy = "opentofu"
    }
  }
}
