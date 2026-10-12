# Scaling plan: from one box to horizontal scale

Status: **proposal, not started.** This turns the target architecture in
[`ARCHITECTURE.md`](ARCHITECTURE.md) §9 and [`aws/DEPLOYMENT.md`](aws/DEPLOYMENT.md) §5–6
into ordered, testable work against the code as it is today.

"Infinite" scale is not a real target. What we can build is **horizontal scale**:
every tier grows by adding nodes, with no single machine, process or partition that
everything goes through, and with known hot spots (celebrities, viral posts, the
global feed) handled explicitly. After this plan, capacity is bounded by budget, not
by design.

---

## 1. What limits scale today

The storage tier already scales (ScyllaDB). Everything else is single-process. Each
item below either stops a second app instance from working correctly, or becomes a
hot spot under load.

### Blockers: a second instance is incorrect or impossible

| # | Limit | Where | Effect with 2+ app instances |
|---|---|---|---|
| B1 | Timeline cache is in-process (moka) | `crates/cache` | Each instance has its own home timelines; fan-out on one instance never reaches users served by another. Users see stale or missing posts. |
| B2 | Event bus is in-process | `crates/events`, check in `bin/social/src/main.rs` | Events never leave the process that produced them. `--roles` cannot be split; the binary refuses to start that way. |
| B3 | Snowflake worker id is a static flag | `--worker-id`, `IdGen` | Two instances with the same id can mint **duplicate post/user ids**. Ephemeral containers make static ids unworkable. |
| B4 | Images on local disk | `crates/storage-fs` | An image uploaded to one instance is a 404 on every other. |
| B5 | Schema created/altered at app start | `ScyllaStore::open`, SQLite migrations | N instances starting together race on DDL. Schema changes need to be a deploy step, not a side effect. |

### Hot spots: correct, but stop scaling at some size

| # | Hot spot | Where | Why it stops scaling |
|---|---|---|---|
| H1 | One `posts_by_day` partition per day | Scylla `recent()` | Every post in the world writes one partition: a single-shard write bottleneck. |
| H2 | Celebrity check on **every** home read | `following_with_min_followers` | Reads the viewer's whole following list (≤ 5 000) plus their follower counts per request. |
| H3 | Cache-miss rebuild fans out per followee | `home_from_store` → `by_authors` | One query per followed account (up to 5 001) when a timeline is not cached. |
| H4 | Unbounded partitions | `followers`, `user_posts`, `likes` | A 50M-follower account is one 50M-row partition; a viral post's likers are one partition. Large partitions hurt compaction, repair and latency. |
| H5 | Session lookup hits the database on every request | `SessionStore::user_for` | Most expensive constant on the read path at high QPS. |
| H6 | Hydration reads posts and users from the database every time | `App::hydrate` | Hot posts and authors are read thousands of times per second. |
| H7 | Rate limits are per process | `crates/gateway/src/ratelimit.rs` | Limits multiply with instance count; a client can be served N× its quota. |
| H8 | Trusts exactly one proxy hop | `--trust-proxy` | Behind CloudFront + ALB the "client IP" is the CDN edge. |
| H9 | Counter updates are not idempotent and not reconciled | `apply_like_deltas`, follow/post counters | Retries or crashes drift counts; no repair job. |
| H10 | No metrics or readiness signal | gateway | Autoscaling has nothing to scale on; load balancers can route to tasks whose dependencies are down. |

---

## 2. Target architecture (AWS)

```
                    Route 53
                       │
              CloudFront + WAF ─────────────── S3: web app (React build)
              │   (TLS, caching, edge rate limits)  S3: media (presigned uploads)
              ▼
     Application Load Balancer (3 AZs)
              │
   ┌──────────┴───────────────────────────────────────────┐
   │ ECS on EC2 Graviton (or EKS), autoscaled per service │
   │   gateway   ×N   social --roles=gateway              │  scales on RPS / CPU
   │   fanout    ×N   social --roles=fanout               │  scales on consumer lag
   │   counters  ×N   social --roles=counters             │  scales on consumer lag
   └──────────┬───────────────┬───────────────┬───────────┘
              │               │               │
        ScyllaDB (RF3,   Dragonfly / Valkey   Redpanda / Kafka
        3+ nodes, 1/AZ)  timelines, sessions, events, partitioned
                         hot objects, limits   by author / post
```

Same binary, same storage ports. Each backend is chosen by flag, as ADR 0001
intended: `--store=scylla --cache=dragonfly --bus=redpanda --media=s3`.

---

## 3. Work plan

Each milestone ends in a deployable state with a measurable exit check. Code
milestones (C) and infrastructure milestones (I) can overlap.

### M1: Make a second instance correct (blockers B1–B5)

| Task | Change |
|---|---|
| C1.1 Shared timeline cache | `crates/cache-redis`: `TimelineCache` on Dragonfly/Valkey (Redis protocol, `fred` crate). Timelines as capped sorted sets (`ZADD` + `ZREMRANGEBYRANK`), pipelined fan-out pushes, `complete` flag as a sibling key. |
| C1.2 Shared event bus | `crates/events-kafka`: `EventBus` on Redpanda/Kafka (`rdkafka`). Topic `events`, keyed by author id (fan-out order per author); consumer groups `fanout` and `counters`; at-least-once with commits after processing. Lift the `--bus=inproc` restriction for `--bus=kafka`. |
| C1.3 Worker-id leasing | On start, claim a free id 0–1023 with an LWT `INSERT … IF NOT EXISTS USING TTL 60`, renew every 20 s, exit if renewal fails. `--worker-id` stays as an override for tests. |
| C1.4 Object-storage media | `crates/storage-s3`: `MediaStore` on S3. Uploads become **presigned PUTs** straight from the browser (`POST /media` returns the URL plus an id); a confirm step reads the object head to validate type and size. Reads are served by CloudFront from S3, not the app. |
| C1.5 Migrations as a job | `social migrate` subcommand runs DDL and exits; the server only checks the schema version and refuses to start on a mismatch. Run it as a one-off task before each deploy. |

**Exit:** two gateways and two workers behind a load balancer pass the existing
end-to-end test suite run against the shared cache, bus and store, plus a new test
that posts through instance A and reads the follower's timeline through instance B.

### M2: Remove the hot spots on the request path (H2, H3, H5–H8, H10)

| Task | Change |
|---|---|
| C2.1 Celebrity set | Maintain `celebs:{viewer}` (the followees above the threshold) on follow and unfollow, and when an account crosses the threshold (counters worker). Home reads become one set read instead of a 5 000-row scan. |
| C2.2 Bounded cache-miss rebuild | Rebuild from the persisted `home_timeline` table (ARCHITECTURE §4, the cold copy written by fan-out), not from every followee's posts. Followee-scan stays as the last resort, capped and parallel-limited. |
| C2.3 Session cache | Cache `token_hash → user_id` in Dragonfly with the session's TTL; delete on logout. |
| C2.4 Object caches | Read-through cache for posts and users used by hydration (short TTL, invalidated on delete/profile edit); counts merged in from counter keys. |
| C2.5 Distributed rate limits | Token buckets in Dragonfly (Lua or `CL.THROTTLE`-style script), keyed by client IP and by user id; coarse limits also at the edge with AWS WAF rate rules. |
| C2.6 Proxy hops | `--trusted-proxy-hops N`, and prefer `CloudFront-Viewer-Address` when present. |
| C2.7 Observability | `/metrics` (Prometheus): request latency and status by route, store/cache latency, consumer lag, fan-out queue depth. `/readyz` checks the store, cache and bus; `/healthz` stays a liveness check. OpenTelemetry traces. |

**Exit:** load test (scripts/loadtest.sh scaled out, or k6) at a target such as
**20k home-timeline reads/s and 2k posts/s on 3 gateway tasks**, p99 < 150 ms, with
cache hit rate > 95 % and no database read on the session path.

### M3: Make data partitions bounded (H1, H4, H9)

| Task | Change |
|---|---|
| C3.1 Sharded global feed | `posts_by_day` → `posts_by_bucket (day, shard)` with 16–64 shards; reads merge shards. At large scale the global feed becomes a cached, sampled stream built by a worker instead of a live query. |
| C3.2 Bucketed wide partitions | `followers (user_id, bucket)`, `user_posts (author_id, month)`, `likes (post_id, bucket)`; readers page across buckets. Fan-out for huge accounts is already pull-based (celebrity path), so follower buckets are read only by the fan-out worker. |
| C3.3 Counter reconciliation | Periodic job recomputes counts from edges for recently active ids and corrects drift; counters stay approximate between runs (ADR 0010). |
| C3.4 Data migration tooling | A resumable copier for the table reshapes above (dual-write, backfill, switch reads, drop old). |

**Exit:** no partition above ~100 MB in `nodetool tablestats`/`cfstats` after a synthetic
"celebrity with 10M followers" and "viral post with 1M likes" test.

### I1: Production infrastructure (Terraform)

Modules as laid out in `aws/DEPLOYMENT.md` §10, replacing the single-instance test
stack in `deploy/aws`:

| Module | Contents |
|---|---|
| `network` | VPC across 3 AZs, private app and data subnets, VPC endpoints for S3/ECR (avoid NAT data charges), one NAT per AZ or fck-nat for cost |
| `edge` | Route 53, ACM certificate, CloudFront (web from S3, media from S3, `/api/*` to the ALB), WAF with rate rules |
| `app` | ECS cluster on Graviton with capacity providers; services `gateway`, `fanout`, `counters`; autoscaling on ALB requests per target and consumer lag; `migrate` task |
| `scylla` | 3 × `i4g`/`i8g` (NVMe) across AZs, rack = AZ, RF3, Scylla Manager backups to S3. *Shortcut:* ScyllaDB Cloud in our account. |
| `cache` | Dragonfly primary + replica on `r7g`/`r8g`. *Shortcut:* ElastiCache Valkey (more nodes for the same RAM, zero ops). |
| `events` | Redpanda 3 nodes with tiered storage to S3. *Shortcut:* Amazon MSK. |
| `media` | S3 bucket, private, CloudFront origin access control, lifecycle rules |
| `observability` | Managed Prometheus + Grafana, CloudWatch Logs, alarms on SLOs |
| `ci` | GitHub Actions with OIDC → ECR push → `migrate` → rolling ECS deploy |

**Exit:** `terraform apply` from empty to serving traffic in one run; blue/green deploy
with no failed requests during a load test; loss of one AZ keeps the app up
(Scylla RF3 + LOCAL_QUORUM, cache replica promotion, Redpanda RF3).

### M4 (later): beyond one region

Cell architecture from ARCHITECTURE Phase 4: users hashed to cells, each cell a full
stack (gateway, workers, Scylla DC, cache, bus); a thin global routing layer; Scylla
multi-DC replication for cross-cell reads of public data. Realtime (WebSocket) and
search (Quickwit) services join here.

---

## 4. Order, effort and cost

| Milestone | Depends on | Rough effort | Monthly cost when running (AWS, on-demand) |
|---|---|---|---|
| M1 shared cache, bus, ids, media, migrations | — | 2–3 weeks | — (code) |
| I1 production infrastructure | M1 | 2 weeks, overlapping | ~$4–6k minimum footprint (DEPLOYMENT §5); ~$1–1.5k with managed shortcuts at small sizes and Savings Plans |
| M2 request-path hot spots | M1 | 2 weeks | — |
| M3 bounded partitions | M1 | 2–3 weeks incl. data migration | — |
| M4 multi-region cells | M2, M3, I1 | months | roughly × number of regions |

A **low-cost staging** variant of I1 (1 Scylla node, ElastiCache Valkey
`cache.t4g.small`, MSK Serverless or a single Redpanda node, 2 small ECS tasks) runs
for roughly **$10–20/day**, which is enough to exercise every code path in M1–M3
before paying for the full footprint.

---

## 5. Decisions needed

| Decision | Options | Recommendation |
|---|---|---|
| Orchestrator | ECS on EC2 (ADR 0012) or EKS | **ECS** unless you specifically want Kubernetes (Scylla Operator, team skills). The app is one image with three roles. |
| Data tier | Self-managed on EC2, or managed (ScyllaDB Cloud, ElastiCache, MSK) | **Managed first** for staging and early production (less ops); move hot tiers to self-managed when the bill justifies it (DEPLOYMENT §3). |
| Event log | Redpanda or Kafka (MSK) | Code targets the Kafka protocol either way; choose per the data-tier decision. |
| Target scale for the first load test | e.g. 1M users, 20k reads/s | Sets instance sizes and the M2 exit numbers. |
| Budget ceiling | | Decides managed vs self-managed and Savings Plans. |
