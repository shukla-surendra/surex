# 0003: Timeline cache: in-process now, Dragonfly from Phase 2

- Status: Accepted. In-process cache (moka, TinyLFU) implemented; Dragonfly planned for
  Phase 2.
- Date: 2026-10-12

## Context
Home timelines must be served from RAM ([0005](0005-hybrid-fanout-timelines.md)). That
means a capped list of up to 800 post IDs per active user, with about 300k reads per
second and around a million list inserts per second at peak.

Twitter kept timelines as **Redis** lists and used **Memcached** (Twemcache) for other
caching. Each Redis process is single-threaded, so Twitter ran very many shards.

## Decision
- **Phases 0–1:** an in-process cache (`moka`, bounded, TinyLFU eviction) holding a
  sorted, capped list per user. Pushes only touch users already in the cache, so
  inactive users cost nothing.
- **Phase 2 onwards:** **Dragonfly**, Redis-protocol compatible, behind the same
  `TimelineCache` trait. Pushes become `LPUSHX` plus `LTRIM` (push only if the list
  exists, then cap it), batched in pipelines. It also stores hot counters and the
  shared rate-limit buckets.

## Alternatives considered
| Option | Verdict |
|---|---|
| **Dragonfly** | **Chosen.** Speaks the Redis protocol, multi-threaded and shared-nothing, so one large box replaces many Redis shards. Good memory efficiency |
| Redis Cluster | Proven, Twitter's choice. Single-threaded per shard, so it needs many processes per box and more operational work |
| KeyDB | Multi-threaded Redis fork, less actively developed |
| Memcached | No list type, so it cannot do capped timelines efficiently |
| Timelines only in ScyllaDB | Kept as the cold copy and fallback. Too slow and costly for the hot read path |

## Consequences
- The cache is a cache: losing it costs a rebuild from storage on the next read, never
  data.
- At Twitter scale, about 300M users × 800 IDs × 8 bytes ≈ 2 TB raw. Only active users
  are cached.
- The in-process cache does not survive restarts. That is acceptable, because
  timelines rebuild lazily.

## Revisit when
Memory per box becomes the bottleneck. Then shard by `user_id` across Dragonfly nodes,
or store IDs as packed binary blobs instead of lists.
