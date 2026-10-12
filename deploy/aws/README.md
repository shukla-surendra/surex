# Minimal-cost AWS test deployment

One Graviton EC2 instance runs the same Docker Compose stack as local development
(ScyllaDB, the Rust API, the React app behind nginx). Meant for short tests: create
it, try it, destroy it. For the production architecture see
[`docs/aws/DEPLOYMENT.md`](../../docs/aws/DEPLOYMENT.md).

| Resource | Notes | ≈ cost (ap-south-1) |
|---|---|---|
| EC2 `t4g.medium` (2 vCPU, 4 GB, arm64) | Amazon Linux 2023, Docker Compose | $0.022/h |
| 20 GB gp3 root volume | encrypted | $0.003/h |
| Public IPv4 address | | $0.005/h |
| 2 ECR repositories | ~140 MB of images; deleted on destroy | ≈ $0 |
| IAM role + security group | SSM access, ECR pull; HTTP from one CIDR | free |

About **$0.03 per hour**. No load balancer, NAT gateway or managed database.

**Not production-grade:** plain HTTP (no TLS), a single node, data on the instance's
disk. Keep `allowed_cidr` narrow (your IP as `/32`) and use only demo accounts.

## Deploy

Needs the AWS CLI with credentials, Terraform ≥ 1.6, Docker, and the Session Manager
plugin (only for a shell on the instance).

```sh
cd deploy/aws/terraform
terraform init
terraform apply \
  -var allowed_cidr="$(curl -s https://checkip.amazonaws.com)/32" \
  -var disable_rate_limit=true        # optional: makes fixture loading instant

cd .. && ./push-images.sh             # build arm64 images and push to ECR
```

The instance installs Docker, waits for the images, and starts the stack; the app is
up 1–3 minutes after the push. Then load demo data and open the URL:

```sh
python3 scripts/seed.py --api "$(terraform -chdir=deploy/aws/terraform output -raw url)"
terraform -chdir=deploy/aws/terraform output url
```

Log in as `ada_codes` / `surex-demo-123` (see [`docs/DEMO_USERS.md`](../../docs/DEMO_USERS.md)).

## Operate

```sh
terraform -chdir=deploy/aws/terraform output -raw shell | sh   # shell via SSM
sudo tail -f /var/log/surex-boot.log                            # bootstrap log
cd /opt/surex && sudo docker compose ps                         # stack status
sudo docker compose logs -f social                              # API logs
```

To ship new code, re-run `./push-images.sh`, then on the instance:
`cd /opt/surex && sudo docker compose pull && sudo docker compose up -d`.

## Destroy

```sh
terraform -chdir=deploy/aws/terraform destroy \
  -var allowed_cidr=0.0.0.0/32        # any value; it is only required, not used
```

This deletes everything above, including the pushed images and all data on the
instance. Terraform state stays local in `deploy/aws/terraform/` (gitignored).
