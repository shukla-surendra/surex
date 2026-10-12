output "url" {
  description = "Open this in the browser (reachable from allowed_cidr only)."
  value       = "http://${aws_instance.app.public_ip}"
}

output "public_dns" {
  value = "http://${aws_instance.app.public_dns}"
}

output "instance_id" {
  value = aws_instance.app.id
}

output "ecr_social" {
  value = aws_ecr_repository.app["social"].repository_url
}

output "ecr_web" {
  value = aws_ecr_repository.app["web"].repository_url
}

output "shell" {
  description = "Open a shell on the instance (needs the Session Manager plugin)."
  value       = "aws ssm start-session --region ${var.region} --target ${aws_instance.app.id}"
}

output "boot_log" {
  description = "Follow the bootstrap log from that shell."
  value       = "sudo tail -f /var/log/surex-boot.log"
}
