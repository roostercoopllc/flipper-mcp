variable "aws_region" {
  description = "AWS region to deploy into"
  type        = string
  default     = "us-east-1"
}

variable "dns_zone" {
  description = "Apex domain for the Route53 hosted zone (e.g., 'example.com'). NS records will be output so you can delegate from your registrar."
  type        = string
}

variable "relay_subdomain" {
  description = "Subdomain for the relay server. Relay will be reachable at <relay_subdomain>.<dns_zone>."
  type        = string
  default     = "relay"
}

variable "ssh_public_key" {
  description = "SSH public key material for EC2 access (contents of ~/.ssh/id_ed25519.pub)."
  type        = string
}

variable "allowed_ssh_cidrs" {
  description = "CIDR blocks allowed to SSH into the relay server. Restrict to your IP in production."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "artifacts_bucket" {
  description = "Name of the S3 bucket where CI uploads the relay binary (e.g., 'myorg-flipper-mcp-artifacts')."
  type        = string
}

variable "relay_port" {
  description = "Internal port the flipper-mcp-relay process listens on (Caddy proxies to this)."
  type        = number
  default     = 9090
}
