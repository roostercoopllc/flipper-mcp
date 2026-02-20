locals {
  relay_domain = "${var.relay_subdomain}.${var.dns_zone}"
}

# ── AMI ────────────────────────────────────────────────────────────────────────
data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"] # Canonical

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# ── SSH key pair ───────────────────────────────────────────────────────────────
resource "aws_key_pair" "relay" {
  key_name   = "flipper-mcp-relay"
  public_key = var.ssh_public_key
}

# ── Security group ─────────────────────────────────────────────────────────────
resource "aws_security_group" "relay" {
  name        = "flipper-mcp-relay"
  description = "flipper-mcp relay server"

  ingress {
    description = "SSH"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = var.allowed_ssh_cidrs
  }

  ingress {
    description = "HTTP (Let's Encrypt ACME challenge)"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "HTTPS / WSS (Caddy)"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ── IAM role — allows EC2 to fetch the relay binary from S3 ───────────────────
data "aws_iam_policy_document" "assume_ec2" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["ec2.amazonaws.com"]
    }
  }
}

data "aws_iam_policy_document" "relay_s3" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["arn:aws:s3:::${var.artifacts_bucket}/relay/*"]
  }
}

resource "aws_iam_role" "relay" {
  name               = "flipper-mcp-relay"
  assume_role_policy = data.aws_iam_policy_document.assume_ec2.json
}

resource "aws_iam_role_policy" "relay_s3" {
  name   = "relay-s3-read"
  role   = aws_iam_role.relay.id
  policy = data.aws_iam_policy_document.relay_s3.json
}

resource "aws_iam_instance_profile" "relay" {
  name = "flipper-mcp-relay"
  role = aws_iam_role.relay.name
}

# ── EC2 instance ───────────────────────────────────────────────────────────────
resource "aws_instance" "relay" {
  ami                  = data.aws_ami.ubuntu.id
  instance_type        = "t3.micro"
  key_name             = aws_key_pair.relay.key_name
  iam_instance_profile = aws_iam_instance_profile.relay.name

  vpc_security_group_ids = [aws_security_group.relay.id]

  user_data = templatefile("${path.module}/templates/cloud-init.yaml.tpl", {
    relay_domain     = local.relay_domain
    relay_port       = var.relay_port
    artifacts_bucket = var.artifacts_bucket
    aws_region       = var.aws_region
  })

  # Replace instance if user_data changes (cloud-init only runs on first boot)
  user_data_replace_on_change = true

  root_block_device {
    volume_size           = 20
    volume_type           = "gp3"
    delete_on_termination = true
  }

  lifecycle {
    create_before_destroy = true
  }
}

# ── Elastic IP ─────────────────────────────────────────────────────────────────
resource "aws_eip" "relay" {
  domain = "vpc"
}

resource "aws_eip_association" "relay" {
  instance_id   = aws_instance.relay.id
  allocation_id = aws_eip.relay.id
}
