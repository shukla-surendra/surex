# 0008: Run on bare metal, not cloud VMs

- Status: Accepted
- Date: 2026-10-12

## Context
The goals are the most performance per unit of hardware and the lowest cost. The load
is steady (social traffic has daily cycles but no extreme bursts at the infrastructure
level), and the team is willing to own the operations.

## Decision
- Deploy on dedicated servers, rented (Hetzner, OVH, Latitude, etc.) or colocated.
- Choose components that are efficient on metal: thread-per-core C++ and Rust systems
  (ScyllaDB, Redpanda, Dragonfly) and no JVM on the hot path.
- Tune the OS and runtime per `ARCHITECTURE.md` §7: NUMA pinning, `SO_REUSEPORT`,
  io_uring, IRQ affinity.

## Alternatives considered
- **Public cloud VMs or Kubernetes:** elastic and fully managed, but typically 3–10×
  the cost for steady load, egress fees, and noisy-neighbor variance.
- **Hybrid** (metal for steady load, cloud for bursts or DR): a valid later option. Not
  needed now.

## Consequences
- We own hardware failure: N+1 capacity, RF3 replication, automated reprovisioning
  (PXE plus Ansible) before Phase 2.
- Capacity is added in days, not minutes. Plan ahead with load tests.
- Build for the fleet's CPU, not the developer laptop ([0009](0009-no-hand-written-assembly.md)).
