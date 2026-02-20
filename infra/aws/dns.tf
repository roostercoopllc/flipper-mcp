# ── Route53 hosted zone ────────────────────────────────────────────────────────
resource "aws_route53_zone" "relay" {
  name = var.dns_zone
}

# ── A record: relay.<dns_zone> → Elastic IP ────────────────────────────────────
resource "aws_route53_record" "relay" {
  zone_id = aws_route53_zone.relay.zone_id
  name    = local.relay_domain
  type    = "A"
  ttl     = 300
  records = [aws_eip.relay.public_ip]
}
