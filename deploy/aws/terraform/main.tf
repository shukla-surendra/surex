# Minimal-cost single-instance deployment: one Graviton EC2 instance runs the same
# Docker Compose stack as local development (ScyllaDB, the Rust API, the React app
# behind nginx). App images come from two private ECR repositories.
#
# Not production-grade on purpose: no TLS, no load balancer, one node. See
# docs/aws/DEPLOYMENT.md for the full architecture.

data "aws_caller_identity" "current" {}

data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }
  filter {
    name   = "default-for-az"
    values = ["true"]
  }
}

data "aws_ssm_parameter" "al2023_arm64" {
  name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64"
}

locals {
  registry = "${data.aws_caller_identity.current.account_id}.dkr.ecr.${var.region}.amazonaws.com"
}

# ---- container registry ------------------------------------------------------------

resource "aws_ecr_repository" "app" {
  for_each             = toset(["social", "web"])
  name                 = "${var.name}/${each.key}"
  image_tag_mutability = "MUTABLE"
  force_delete         = true # `terraform destroy` removes pushed images too

  image_scanning_configuration {
    scan_on_push = true
  }
}

# ---- instance identity: pull from ECR, reachable through SSM (no SSH) ---------------

resource "aws_iam_role" "instance" {
  name = "${var.name}-instance"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "ssm" {
  role       = aws_iam_role.instance.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_role_policy_attachment" "ecr_read" {
  role       = aws_iam_role.instance.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

resource "aws_iam_instance_profile" "instance" {
  name = "${var.name}-instance"
  role = aws_iam_role.instance.name
}

# ---- network: HTTP from one CIDR only ---------------------------------------------

resource "aws_security_group" "app" {
  name        = "${var.name}-app"
  description = "surex test stack: HTTP from the allowed CIDR"
  vpc_id      = data.aws_vpc.default.id

  ingress {
    description = "Web app"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = [var.allowed_cidr]
  }

  egress {
    description = "Image pulls and AWS APIs"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ---- the instance ---------------------------------------------------------------------

resource "aws_instance" "app" {
  ami                         = data.aws_ssm_parameter.al2023_arm64.value
  instance_type               = var.instance_type
  subnet_id                   = sort(data.aws_subnets.default.ids)[0]
  vpc_security_group_ids      = [aws_security_group.app.id]
  iam_instance_profile        = aws_iam_instance_profile.instance.name
  associate_public_ip_address = true

  user_data = templatefile("${path.module}/user_data.sh.tftpl", {
    region             = var.region
    registry           = local.registry
    social_image       = "${aws_ecr_repository.app["social"].repository_url}:latest"
    web_image          = "${aws_ecr_repository.app["web"].repository_url}:latest"
    disable_rate_limit = var.disable_rate_limit
  })
  user_data_replace_on_change = true

  root_block_device {
    volume_type = "gp3"
    volume_size = var.disk_gb
    encrypted   = true
  }

  metadata_options {
    http_tokens = "required" # IMDSv2 only
  }

  tags = {
    Name = "${var.name}-app"
  }
}
