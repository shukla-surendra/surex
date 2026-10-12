# 0001: Modular monolith with swappable ports

- Status: Accepted, implemented in Phase 0
- Date: 2026-10-12

## Context
The code must be ready for Twitter-level scale. It must also run cheaply on one server
today, operated by a very small team. Microservices from day one would multiply
operational cost (deployments, networking, observability) long before traffic justifies
it. A classic monolith with storage calls scattered through handlers, on the other
hand, cannot be split later without a rewrite.

## Decision
- Build a **modular monolith**: one Cargo workspace and one `social` binary.
- Each concern talks to infrastructure only through traits ("ports"):
  - `UserStore`, `SessionStore`, `PostStore`, `GraphStore`, `LikeStore`
  - `TimelineCache`
  - `EventBus`
- Implementations are chosen at startup (`--store`, `--cache`, `--bus`), and roles are
  chosen with `--roles` (`all`, `gateway`, `fanout`, `counters`).
- Every port method is a single-partition lookup or a bounded range scan, keyed by
  `user_id` or `post_id`. That keeps the ports implementable on a sharded store.

## Alternatives considered
- **Microservices now:** rejected. Network hops, deployment and tracing overhead with
  no traffic to justify them.
- **Plain monolith:** rejected. Scaling would require a rewrite.
- **Generic ORM abstraction:** rejected. It hides query shapes, and query shape is
  exactly what decides whether something can be sharded.

## Consequences
- Phase 1 runs as one process: SQLite, in-process cache, in-process bus.
- Scaling out means adding implementations (Scylla, Dragonfly, Redpanda) plus
  configuration. Business logic does not change.
- In-process calls between services will need gRPC clients once roles run in separate
  processes (Phase 3).
- The `async_trait` dynamic dispatch costs one heap allocation per call. That is
  negligible next to I/O.

## Revisit when
A role needs an independent release cadence or team. Then split it into its own
`svc-*` crate and deployment.
