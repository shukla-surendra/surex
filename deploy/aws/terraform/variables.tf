variable "name" {
  description = "Prefix for every resource name."
  type        = string
  default     = "surex"
}

variable "region" {
  description = "AWS region."
  type        = string
  default     = "ap-south-1"
}

variable "allowed_cidr" {
  description = "Who may open the app on port 80, e.g. \"203.0.113.7/32\" for just your IP. Keep it narrow: the test stack serves plain HTTP."
  type        = string
}

variable "instance_type" {
  description = "Graviton (arm64) instance; needs ~4 GB RAM for Scylla + app."
  type        = string
  default     = "t4g.medium"
}

variable "disk_gb" {
  description = "Root volume size (gp3)."
  type        = number
  default     = 20
}

variable "disable_rate_limit" {
  description = "Turn off the API's per-IP rate limits (handy for loading fixtures quickly)."
  type        = bool
  default     = false
}
