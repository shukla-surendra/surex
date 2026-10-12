# 0012: AWS as a hosting option

- Status: **Proposed.** Amends [0008](0008-bare-metal.md) if accepted.
- Date: 2026-10-12

## Context
[0008](0008-bare-metal.md) chose bare metal for the lowest unit cost. AWS is now being
considered for faster setup, managed edge and security services, elasticity, and
global reach.

## Proposal
Support AWS as a first-class deployment target with the **same application
architecture**. Details are in [`docs/aws/DEPLOYMENT.md`](../aws/DEPLOYMENT.md).
- Self-manage the hot data path on EC2 with local NVMe: ScyllaDB, Redpanda, Dragonfly,
  NATS.
- Use AWS-managed services for edge, TLS, DNS, load balancing, object storage, secrets,
  the container registry, observability and security tooling.
- Run app roles as ECS services on Graviton EC2, one image with different `--roles`.
- Use three AZs, with rack-aware replication and reads served from the local AZ to
  control cross-AZ transfer cost.

## Alternatives considered
- **Fully managed data tier** (Keyspaces or DynamoDB, ElastiCache, MSK, OpenSearch):
  least operational work, but several times the cost at target write volume, plus
  data-model lock-in with DynamoDB. Acceptable as a low-traffic Phase 2 shortcut.
- **EKS instead of ECS:** more portable and a larger ecosystem, but more to operate.
  ECS is enough for one image with a few roles. Revisit if the platform needs
  Kubernetes operators (e.g. the Scylla Operator).
- **Bare metal only** ([0008](0008-bare-metal.md)): lowest unit cost, highest
  operational effort.

## Consequences
- Requires the code changes in DEPLOYMENT.md §11. The most important are proxy-hop
  aware client IPs, and Snowflake worker-ID leasing for ephemeral tasks.
- Terraform modules and Ansible/Packer roles are shared with bare metal where possible,
  so the hosting choice stays reversible.
- Expect roughly 2–3× the bare-metal infrastructure cost at steady load, partly offset
  by lower operational effort.

## Decision needed
Accept this to make AWS the primary target, or keep it as a secondary option alongside
bare metal.
