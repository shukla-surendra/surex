# 0002: Primary datastore: SQLite now, ScyllaDB from Phase 2

- Status: Accepted. SQLite implemented; ScyllaDB planned for Phase 2.
- Date: 2026-10-12

## Context
The workload at scale: about 500M posts per day (150k/s peaks), about 300k timeline
reads per second, write-heavy append-only data, and access almost always by `user_id`
or `post_id`. Data grows by about 1.5 TB per day at RF3.

The store must run well on bare metal ([0008](0008-bare-metal.md)), scale horizontally
by adding nodes, and replicate across racks and datacenters.

Twitter's path, for reference (see the [ADR index](README.md#prior-art-twitters-storage-stack)):
1. MySQL.
2. Sharded MySQL using Gizzard.
3. A failed Cassandra migration for tweets.
4. Building **Manhattan**, an in-house distributed key-value store.

The lesson: at this scale, a horizontally partitioned key-value or wide-column store
wins over a relational database for the core data.

## Decision
- **Phases 0–1: SQLite in WAL mode**, behind the storage ports. Zero operations, and it
  handles a single box's load.
- **Phase 2 onwards: ScyllaDB** for users, posts, the graph, likes, counters and the
  cold timeline copy. Use RF3 with `LOCAL_QUORUM`.

## Alternatives considered
| Option | Verdict |
|---|---|
| **ScyllaDB** | **Chosen.** Cassandra-compatible data model (the shape Manhattan solved), written in C++ with one shard per core, so it saturates NVMe and cores on metal. No JVM GC pauses. Linear scale-out and multi-DC replication built in |
| Apache Cassandra | Same data model, but JVM GC pauses and higher hardware cost per operation. Twitter's own experience was painful |
| PostgreSQL + Citus | Strong for relational data, joins and transactions, but resharding is heavier and write throughput per node is lower. Good fallback for users and auth |
| MySQL + Vitess | Proven at YouTube and Slack scale and close to Twitter's Gizzard approach, but more moving parts to run on metal |
| CockroachDB / TiDB / YugabyteDB | Distributed SQL with strong consistency, but costs latency and hardware per write. The workload does not need cross-row transactions |
| Build our own (like Manhattan) | Rejected. Open-source options now cover the need |

## Consequences
- Data is modeled per query and denormalized (`ARCHITECTURE.md` §4). No joins at
  scale. The SQLite implementation keeps to the same access patterns.
- Uniqueness (usernames) needs Scylla lightweight transactions (LWT) on a
  `users_by_name` table.
- Counters (followers, likes) become Scylla counters or event-aggregated values
  ([0010](0010-eventually-consistent-counters.md)).
- Migrating from SQLite to Scylla needs a one-off data copy job. The application code
  does not change.

## Revisit when
- Product needs require multi-row transactions or ad-hoc relational queries. Then
  consider Postgres for that subset only.
- Or one SQLite box falls short before Phase 2 is ready. Then a single-node Scylla is
  the bridge.
