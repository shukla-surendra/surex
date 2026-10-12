# Architecture Decision Records

Each record captures one significant decision: its context, the options weighed, and
the consequences. Records are immutable once accepted. To change a decision, add a new
record that supersedes the old one and update the old one's status.

| # | Decision | Status |
|---|---|---|
| [0001](0001-modular-monolith-with-ports.md) | Modular monolith with swappable ports, one binary with `--roles` | Accepted |
| [0002](0002-primary-datastore.md) | Primary datastore: SQLite now, ScyllaDB from Phase 2 | Accepted |
| [0003](0003-timeline-cache.md) | Timeline cache: in-process now, Dragonfly from Phase 2 | Accepted |
| [0004](0004-event-log.md) | Event log: in-process now, Redpanda from Phase 2 | Accepted |
| [0005](0005-hybrid-fanout-timelines.md) | Hybrid push/pull fan-out for home timelines | Accepted, implemented |
| [0006](0006-snowflake-ids.md) | 64-bit Snowflake IDs, serialized as strings | Accepted, implemented |
| [0007](0007-search-engine.md) | Search: Quickwit / Tantivy | Accepted (Phase 2) |
| [0008](0008-bare-metal.md) | Run on bare metal, not cloud VMs | Accepted |
| [0009](0009-no-hand-written-assembly.md) | No hand-written assembly; build-level CPU tuning instead | Accepted |
| [0010](0010-eventually-consistent-counters.md) | Eventually consistent, batched like counters | Accepted, implemented |
| [0011](0011-opaque-session-tokens.md) | Opaque, hashed session tokens in HttpOnly cookies (not JWT) | Accepted, implemented |
| [0012](0012-aws-hosting-option.md) | AWS as a hosting option (amends 0008) | **Proposed** |

## Prior art: Twitter's storage stack

Several records compare against Twitter, the closest existing system at the target
scale. Sources are Twitter's public engineering blog posts and talks, mostly from
2010–2022. Little has been published since the company became X, so its current stack
is not publicly known.

| Data | Twitter used | Our equivalent |
|---|---|---|
| Tweets, users, core data | MySQL → sharded MySQL via **Gizzard** (store "T-bird") → **Manhattan** (in-house distributed KV, ~2014) | ScyllaDB ([0002](0002-primary-datastore.md)) |
| Social graph | **FlockDB** (graph store on sharded MySQL) | ScyllaDB `followers` / `following` tables |
| Home timelines | **Redis** lists of tweet IDs ("Haplo"), hybrid fan-out | Dragonfly ([0003](0003-timeline-cache.md)), same algorithm ([0005](0005-hybrid-fanout-timelines.md)) |
| Caching | Memcached (**Twemcache** fork) + **Twemproxy** | Dragonfly / in-process |
| Event streams | EventBus, DistributedLog → **Kafka** (~2018) | Redpanda ([0004](0004-event-log.md)) |
| Search | **Earlybird** (Lucene-based, real-time) | Quickwit / Tantivy ([0007](0007-search-engine.md)) |
| Media | **Blobstore** (in-house) | MinIO → Ceph, behind a CDN |
| Analytics | Hadoop/HDFS, Vertica, later Google BigQuery | Out of scope for now |
| IDs | **Snowflake** (Twitter invented it, 2010) | Snowflake ([0006](0006-snowflake-ids.md)) |

Twitter evaluated Cassandra for tweets around 2010 and abandoned the migration. The
operational pain of running Cassandra at that scale was part of the motivation for
building Manhattan.

The lesson we take: Twitter built custom databases because, in 2010, nothing
open-source fit its scale. Today, open-source systems cover each role, so we adopt
Twitter's *architecture* (precomputed timelines, hybrid fan-out, user-keyed sharding,
an event log) without its custom infrastructure.
