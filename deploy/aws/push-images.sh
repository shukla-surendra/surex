#!/usr/bin/env bash
# Builds the backend and frontend images for arm64 (Graviton) and pushes them to the
# ECR repositories created by ./terraform. Run after `terraform apply`.
set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"
TF="terraform -chdir=terraform"
REGION="$($TF output -raw shell | sed -E 's/.*--region ([^ ]+).*/\1/')"
SOCIAL="$($TF output -raw ecr_social)"
WEB="$($TF output -raw ecr_web)"
REGISTRY="${SOCIAL%%/*}"

aws ecr get-login-password --region "$REGION" | docker login --username AWS --password-stdin "$REGISTRY"

docker build --platform linux/arm64 -t "$SOCIAL:latest" "$ROOT"
docker build --platform linux/arm64 -t "$WEB:latest" "$ROOT/frontend"
docker push "$SOCIAL:latest"
docker push "$WEB:latest"
echo "pushed. The instance pulls them automatically within ~20s."
