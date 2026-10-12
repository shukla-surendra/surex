# 0004: Event log: in-process now, Redpanda from Phase 2

- Status: Accepted. In-process bus implemented; Redpanda planned for Phase 2.
- Date: 2026-10-12

## Context
Every write emits an event (`SPEC.md` §6). Fan-out, counters, and later search
indexing and notifications consume these events away from the request path. At scale
this needs a durable, partitioned, replayable log with consumer groups.

Twitter moved from in-house systems (EventBus, DistributedLog) to **Kafka** around 2018.

## Decision
- **Phases 0–1:** an in-process bus (`tokio::mpsc` per consumer group, bounded, so a
  full buffer slows publishers down). Not durable.
- **Phase 2 onwards:** **Redpanda** behind the same `EventBus` trait. Partition keys are
  `author_id`, `follower` or `post_id`, depending on the event. Use the
  transactional-outbox pattern where exactly-once delivery matters.

## Alternatives considered
| Option | Verdict |
|---|---|
| **Redpanda** | **Chosen.** Speaks the Kafka API (whole ecosystem and clients), written in C++ with thread-per-core, no JVM or ZooKeeper, and needs noticeably fewer nodes for the same throughput on metal |
| Apache Kafka | The industry standard and Twitter's choice. JVM tuning and a larger hardware footprint |
| NATS JetStream | Simpler and lighter, but a weaker fit for high-throughput, partitioned, replayable logs. Kept for realtime pub/sub (`ARCHITECTURE.md` §3) |
| Apache Pulsar | Separates storage from compute, but runs more systems (BookKeeper, ZooKeeper) |

## Consequences
- Consumers must be idempotent. They already are: timeline insertion deduplicates by
  `post_id`.
- With the in-process bus, events in flight are lost on a crash. Timelines recover by
  rebuilding from storage. Like-count deltas need a reconciliation job (`SPEC.md` §11).
- Kafka-protocol compatibility keeps the option to switch to Apache Kafka or a managed
  service.

## Revisit when
Redpanda licensing or support terms change materially. Then switch to Apache Kafka;
the client code stays the same.
