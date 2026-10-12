# my_social — Architecture & Scaling Plan

Target: a Twitter-class social network whose **code** is ready for Twitter-level scale,
while it **runs** today on one or a few bare-metal servers at minimum cost.

The core idea: **build a modular monolith with shard-ready data and swappable
storage.** The same binary runs everything in one process on one box today. Later it
runs as separate services across hundreds of boxes, by configuration, not by a rewrite.

---

## 1. Design targets

| Metric (public Twitter-era figures, approximate) | Value | What it implies |
|---|---|---|
| Posts/day | ~500M (≈6k/s avg, 150k/s record peak) | Writes must be partitioned and append-only |
| Timeline reads | ~300k QPS peak | Timelines are **precomputed** and served from RAM |
| Fan-out deliveries | ~1M+ timeline inserts/s at peak | Async fan-out through a durable log |
| Follower skew | Few accounts with 100M+ followers | Hybrid push/pull timeline |
| Post data | ~1 KB × 500M × RF3 ≈ 1.5 TB/day | Horizontally sharded wide-column store |

Non-goals for now: ML ranking, ads, and video transcoding at scale. The plan leaves
room for them.

---

## 2. Principles

1. **Shard key from day one.** Every table is keyed by `user_id` or `post_id`. No
   query needs a cross-shard join. The current `SELECT ... WHERE user_id IN (SELECT
   followee ...)` home feed is a pull-model join that cannot scale. It gets replaced
   (§5.3).
2. **Globally unique, time-sortable 64-bit IDs (Snowflake).** These replace SQLite
   `AUTOINCREMENT`. IDs double as the pagination cursor and the sort key.
3. **Stateless compute, stateful storage.** API nodes hold no session state. Any node
   can serve any request.
4. **Every write emits an event.** Fan-out, counters, search indexing, notifications and
   analytics all consume the log. The request path never does them inline.
5. **Storage behind traits.** `SQLite` (dev / single box) and `Scylla + Dragonfly`
   (cluster) implement the same Rust traits.
6. **Mechanical sympathy over exotic code.** Use thread-per-core where profiling proves
   it pays, io_uring, NUMA pinning and zero-copy serialization. **No hand-written
   assembly:** the workload is I/O- and memory-bound, and portable SIMD intrinsics cover
   the rare CPU hot spot.
7. **Cells.** At large scale, the fleet is split into independent cells (each one a full
   copy of the stack serving a slice of users). This limits blast radius and makes
   capacity linear.

---

## 3. High-level architecture

```
                    Clients (web / mobile)
                            │  HTTPS / HTTP3, WebSocket
                ┌───────────▼────────────┐
                │ L4 LB: XDP/eBPF (Katran)│  ← anycast BGP at multi-DC stage
                │   or HAProxy+keepalived │
                └───────────┬────────────┘
                ┌───────────▼────────────┐
                │  Edge / API Gateway     │  Rust (hyper + rustls), auth, rate limit
                └──┬─────┬──────┬─────┬──┘
     gRPC (tonic)  │     │      │     │
   ┌───────────────▼┐ ┌──▼────┐ ┌▼──────────┐ ┌▼───────────┐ ┌────────────┐
   │ Post (write)   │ │Timeline│ │ Social    │ │ Engagement │ │ Identity   │
   │                │ │ (read) │ │ Graph     │ │ (likes,cnt)│ │ (auth)     │
   └───────┬────────┘ └──┬────┘ └─────┬─────┘ └─────┬──────┘ └─────┬──────┘
           │ events      │            │             │              │
   ┌───────▼─────────────┴────────────┴─────────────┴──────────────┴──────┐
   │                 Event log: Redpanda (Kafka API)                       │
   └──┬──────────────┬──────────────┬─────────────┬───────────────────────┘
  ┌───▼─────┐  ┌─────▼─────┐  ┌─────▼─────┐  ┌─────▼──────┐
  │ Fan-out │  │ Counters  │  │  Search   │  │ Notify /   │
  │ workers │  │ aggregator│  │  indexer  │  │ realtime   │
  └───┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬──────┘
      │              │              │              │
  ┌───▼──────────────▼──┐  ┌────────▼──────┐  ┌────▼─────┐  ┌──────────────┐
  │ Dragonfly (timeline │  │ Quickwit /    │  │  NATS    │  │ Media: MinIO │
  │ cache, hot counters)│  │ Tantivy       │  │ (pubsub) │  │ / Ceph + CDN │
  └─────────────────────┘  └───────────────┘  └──────────┘  └──────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │ ScyllaDB: users, posts, graph, likes, timelines-cold (source of truth)│
  └──────────────────────────────────────────────────────────────────────┘
```

### Why these components (bare-metal and cost lens)

Each choice is recorded with its alternatives in [`docs/adr/`](adr/README.md). That
index also maps every component to what Twitter used for the same job.

| Concern | Choice | Reason |
|---|---|---|
| Primary store | **ScyllaDB** | C++ and thread-per-core (shard-per-core). It saturates NVMe and cores on metal. Linear scale-out. Cassandra-compatible, with no JVM GC pauses |
| Event log | **Redpanda** | Kafka API, C++, thread-per-core, no ZooKeeper or JVM. About 3× less hardware than Kafka for the same throughput |
| Cache | **Dragonfly** | Redis-compatible and multi-threaded, so it uses all cores on one box instead of many Redis shards |
| Pub/sub (realtime) | **NATS** | Tiny footprint, millions of msgs/s, simple clustering |
| Search | **Quickwit / Tantivy** | Rust, can index on object storage, cheap for append-only post data |
| Media | **MinIO** (small) → **Ceph** (large) + CDN | S3 API on your own disks; put a CDN in front for egress |
| Internal RPC | **gRPC (tonic)**, plus **rkyv** for hot internal payloads | Typed contracts. rkyv gives zero-copy deserialization where it matters |
| Single-box DB | **SQLite (WAL)** | Zero ops. Keep it behind the same traits for dev and the first deployment |

Postgres is the alternative primary store if relational features matter more than
write scale. Users and auth fit Postgres well (Citus for sharding). Posts, timelines
and likes do not.

---

## 4. Data model (shard-ready)

IDs: Snowflake `i64` = 41-bit ms timestamp | 10-bit worker id | 12-bit sequence
(≈4M IDs/s per worker, sortable by time).

```
users            PK (user_id)              username, display, created, flags
users_by_name    PK (username_lower)       user_id                 -- uniqueness via LWT
posts            PK (post_id)              author_id, body, reply_to, media[], created
user_posts       PK (author_id), CK post_id DESC                    -- profile timeline
following        PK (user_id),  CK followee_id                      -- who I follow
followers        PK (user_id),  CK follower_id                      -- who follows me
likes            PK (post_id),  CK user_id                          -- did X like Y
user_likes       PK (user_id),  CK post_id DESC
counters         PK (post_id)   likes, reposts, replies (Scylla counters / aggregated)
user_stats       PK (user_id)   followers, following, posts
sessions         PK (token_hash) user_id, expires                   -- TTL
home_timeline    PK (user_id),  CK post_id DESC  (cold copy, TTL)   -- cache backfill
```

Rules: denormalize per query. Write to both directions of the graph in one batch. Every
read is a single-partition lookup or a bounded range scan.

---

## 5. Key flows

### 5.1 Create post
1. Gateway authenticates, applies the rate limit, and validates (≤280 chars).
2. Post service generates a Snowflake ID and writes `posts` and `user_posts` (quorum).
3. It publishes `PostCreated{post_id, author_id, ts}` to Redpanda (partitioned by
   `author_id`). For exactly-once, use an outbox table plus a relay.
4. It returns `201` to the user (p99 target < 20 ms). Everything else is async.

### 5.2 Fan-out (hybrid push/pull)
- **Normal authors (< ~10k followers):** fan-out workers page through `followers` and
  `LPUSH`+`LTRIM` the `post_id` into each **active** follower's Dragonfly timeline,
  capped at 800 IDs. Inactive users (no login in 30 days) are skipped and rebuilt on
  demand.
- **Celebrities (≥ threshold):** no push. Their posts are pulled at read time.
- Workers are idempotent (`post_id` is the dedupe key) and scale by partition count.

### 5.3 Read home timeline
1. `LRANGE timeline:{uid}` returns post IDs (cache hit ~99%).
2. Merge in recent posts from the celebrities the user follows (cached per celebrity).
3. Hydrate via a multi-get of `posts` and counters from cache, with a Scylla fallback.
   Assemble and return.
4. Cache miss (inactive user): rebuild from `following` → `user_posts` (k-way merge)
   and re-seed the cache.

Pagination uses the `before=post_id` cursor everywhere, never `OFFSET`.

### 5.4 Likes and counters
Writes go to `likes`/`user_likes` and emit an event. The aggregator batches increments
per post every ~1 s and writes them to `counters` and Dragonfly. Displayed counts are
eventually consistent, just as on Twitter.

### 5.5 Real-time
WebSocket gateway nodes subscribe to NATS subjects (`user.{id}`) for connected users
only. Fan-out workers publish to NATS for online followers.

---

## 6. Code architecture (Cargo workspace)

✅ = implemented in Phase 0; ⏳ = planned.

```
my_social/
├── crates/
│   ├── core/            ✅ domain types, Snowflake IdGen, validation, events, errors (no I/O)
│   ├── storage/         ✅ ports: UserStore, SessionStore, PostStore, GraphStore, LikeStore
│   ├── storage-sqlite/  ✅ impl for dev and single box
│   ├── storage-scylla/  ✅ impl for cluster (scylla crate)
│   ├── cache/           ✅ TimelineCache port + in-proc impl (moka, TinyLFU) | ⏳ Dragonfly (fred)
│   ├── events/          ✅ EventBus port + in-proc impl | ⏳ Redpanda (rdkafka)
│   ├── services/        ✅ identity, posts, likes, graph, timelines + fanout & counter workers
│   ├── gateway/         ✅ HTTP API, sessions, rate limit, request ids, tracing | ⏳ websocket
│   └── proto/           ⏳ gRPC definitions (tonic-build), once services are split
├── bin/social/          ✅ ONE binary: `social --roles=all` (or gateway,fanout,counters)
├── frontend/            ✅ React + TypeScript client (Vite), served separately (nginx/CDN)
├── scripts/             ✅ loadtest.sh (oha)
└── deploy/              ⏳ Ansible / Nomad jobs, kernel tuning, dashboards
```

Services live in one crate while they share a process. They split into `svc-*`
crates at Phase 3, when they become separately deployed fleets.

**Deployment modes from the same code:**
- `--roles=all --store=sqlite --bus=inproc --cache=inproc` → one box, one process
  (today).
- `--roles=all --store=scylla --bus=redpanda --cache=dragonfly` → small cluster.
- `--roles=fanout` / `--roles=gateway` ... → separate fleets at scale. Services talk
  through traits. In-process calls become gRPC by swapping the client implementation.

Frontend: a React + TypeScript app (`frontend/`), built to static assets and served
separately from the API (nginx locally, a CDN in production) on the same origin, so the
`SameSite=Strict` session cookie works without CORS. Mobile clients can
use the same API or gRPC-web.

---

## 7. Bare-metal platform

> AWS alternative: the same architecture mapped onto AWS (EC2/ECS on Graviton,
> CloudFront, S3, and self-managed Scylla, Redpanda and Dragonfly) is in
> [`aws/DEPLOYMENT.md`](aws/DEPLOYMENT.md). That choice is pending in
> [ADR 0012](adr/0012-aws-hosting-option.md) (Proposed).

### 7.1 Node profiles (example, 2026 commodity hardware)
| Role | Spec | Notes |
|---|---|---|
| App / gateway / workers | 1× AMD EPYC (64–96c) or Ampere (96–192c ARM), 256 GB, 2×25 GbE | Stateless; the cheapest cost per request |
| Scylla | 32–64c, 256–512 GB, 4–8× NVMe (XFS, no RAID; Scylla replicates), 25 GbE | RF=3 across racks |
| Cache (Dragonfly) | 32c, 512 GB–1 TB RAM | Timelines: ~300M users × 800 × 8 B ≈ 2 TB raw for all users. Cache only the active ones |
| Redpanda | 32c, 128 GB, NVMe, 25 GbE | 3–5 nodes per cluster |
| Media (Ceph/MinIO) | HDD-dense + NVMe metadata | Behind a CDN |

### 7.2 Build for the fleet, not the laptop
`target-cpu=native` (in today's `.cargo/config.toml`) targets the **M4 build machine**.
For production, build per fleet:
- x86 EPYC: `-C target-cpu=x86-64-v4` (or `znver4`/`znver5`)
- ARM Ampere: `-C target-cpu=neoverse-n1` / `ampere1`

Use PGO (`cargo-pgo`) on recorded production traffic: typically 10–20% for free.
Use BOLT optionally.

### 7.3 OS and runtime tuning
- Linux (Debian/Ubuntu LTS), current kernel with io_uring, `performance` CPU governor.
- NUMA: one process per socket, pinned (`numactl`). IRQ affinity aligned with RSS
  queues.
- Networking: `SO_REUSEPORT` listeners per core, larger somaxconn/backlog, BBR, busy
  polling on hot nodes.
- Transparent hugepages off for Scylla/Dragonfly (their docs require it). Explicit
  hugepages where supported.
- Runtime: Tokio multi-threaded by default. Move the gateway and timeline read path to
  thread-per-core (`monoio` / `glommio` with io_uring) **only** if profiles show
  cross-core contention.
- Serialization: `simd-json` / `sonic-rs` for the public JSON API, `rkyv` internally.

### 7.4 Operations on metal
| Need | Tool |
|---|---|
| Provisioning | PXE + Tinkerbell or MAAS, Ansible for config |
| Scheduling | systemd units (≤10 nodes) → Nomad (simple, metal-friendly) or k3s/Kubernetes |
| Secrets | Vault / OpenBao |
| Metrics | VictoriaMetrics + Grafana |
| Logs | Vector → Loki or ClickHouse |
| Tracing | OpenTelemetry → Tempo / Jaeger |
| Backups | Scylla Manager snapshots → object store, off-site copy |
| Load balancing | HAProxy + keepalived (VRRP) → Katran/XDP + BGP ECMP at scale |

### 7.5 Cost note
Rented dedicated servers (Hetzner, OVH, Latitude, etc.) or colocation usually cost
3–10× less than equivalent cloud VMs for steady load, and egress is often bundled. The
trade-off is that you own the failure handling: plan N+1 capacity and use RF3
everywhere.

---

## 8. Reliability, security, abuse

- **SLOs:** timeline read p99 < 100 ms, post write p99 < 50 ms, 99.95% availability.
- **Backpressure:** bounded queues, load shedding at the gateway, circuit breakers on
  gRPC clients.
- **Rate limits:** per IP, per user and per endpoint. Use a token bucket in Dragonfly,
  with a local approximate pre-check.
- **Auth:** Argon2id passwords. Store a hashed session token, never the raw one (today
  it is stored raw). Use short-lived access tokens plus refresh tokens for mobile.
- **Abuse:** signup CAPTCHA/PoW, spam scoring in the event pipeline, block and mute
  lists checked at hydration.
- **Privacy/compliance:** account deletion propagated through events (tombstones), and
  a data export job.
- **DR:** RF3 within a DC. At the multi-DC stage, Scylla multi-DC replication with
  `LOCAL_QUORUM`.

---

## 9. Phased plan

Each phase runs the **same codebase**. Capacity figures are rough planning estimates and
must be validated by load tests.

### Phase 0: Refactor for scale-readiness (in progress; contract in `SPEC.md`)
- [x] Convert to a Cargo workspace (§6). Single `social` binary with `--roles`.
- [x] Snowflake IdGen replaces AUTOINCREMENT. All IDs become `i64` Snowflakes and are
      serialized as strings to JS (JS numbers lose precision above 2^53).
- [x] Define storage, cache and event traits. Port the current SQLite code into
      `storage-sqlite`.
- [x] Replace the pull-join home feed with a cached timeline, hybrid push/pull fan-out,
      and an in-process fan-out worker.
- [x] Event-driven like counters (batched aggregator).
- [x] Hashed session tokens, rate limiting, request IDs, structured logging (`tracing`),
      graceful shutdown.
- [x] Integration tests for all user flows (`crates/services/tests/flows.rs`).
- [x] `oha` load script (`scripts/loadtest.sh`).
- [ ] Prometheus `/metrics` endpoint.
- [ ] `criterion` micro-benchmarks and a CI performance gate.
- [ ] Like-counter reconciliation job (SPEC §11).

### Phase 1: One bare-metal server (≈ up to ~100k–1M users)
- Single dedicated box (e.g. 16–32 cores, 128 GB, 2× NVMe). Run `--roles=all`, SQLite
  WAL or single-node Scylla, in-process cache and bus.
- HAProxy or Caddy for TLS (or rustls directly). Nightly off-site backups.
- Observability stack on the same box, or on a tiny second one.

### Phase 2: Small cluster, 5–15 servers (≈ ~1–10M users)
- 3× Scylla (RF3), 3× Redpanda, 1–2× Dragonfly, 2+ stateless app nodes, HAProxy +
  keepalived.
- Switch implementations by config (`--store=scylla --bus=redpanda --cache=dragonfly`).
- Media on MinIO behind a CDN. NATS for realtime. Quickwit for search.
- Nomad for scheduling. Zero-downtime rolling deploys.

### Phase 3: Service split, 50–300 servers (≈ ~10–100M users)
- Separate fleets per role (gateway, timeline, fan-out, graph, engagement), scaled
  independently.
- Hybrid celebrity pull path, PGO builds, thread-per-core hot paths where justified.
- Multi-rack and multi-AZ. Katran/XDP load balancing. A dedicated spam/abuse pipeline.

### Phase 4: Twitter scale, thousands of servers, multi-region (≈ 100M–500M+ users)
- Cell architecture: users hashed to cells, each cell a full stack. A thin global
  routing layer.
- Multi-region Scylla, anycast edge, geo-aware routing, regional media caches.
- Ranking/recommendation service (candidate retrieval + scoring) feeding the
  "For You" timeline.

---

## 10. Remaining gaps between the code and this plan

| Today | Target |
|---|---|
| SQLite and Scylla storage; only in-proc cache and bus | Dragonfly and Redpanda behind the same traits (Phase 2) |
| `--roles` other than `all` need an external bus | Separate fleets once Redpanda lands |
| Logs only | Prometheus metrics + OpenTelemetry traces |
| Per-node rate limits | Shared buckets in Dragonfly |
| `target-cpu=native` for local builds | Per-fleet target CPU + PGO in the release pipeline |

## 11. Risks and open decisions

- **Scylla vs Postgres for Phase 1:** Scylla from day one means no migration later, but
  more operational overhead on one box. Recommendation: SQLite behind traits in Phase 1,
  Scylla at Phase 2. The traits make the switch a config change plus a data migration
  job.
- **Fan-out threshold and the inactive-user policy:** tune with real follower
  distributions.
- **Counter accuracy:** eventual consistency is accepted. Exact counts would need
  per-shard aggregation.
- **Operational load on metal:** hardware failures are your problem. Budget N+1 nodes
  and automate reprovisioning before Phase 2.
