# 0005: Hybrid push/pull fan-out for home timelines

- Status: Accepted, implemented in Phase 0
- Date: 2026-10-12

## Context
The prototype built the home feed with a join at read time:
`posts WHERE author IN (SELECT followee ...)`. That cannot be sharded. Every read touches
every followed author's partition.

Pure fan-out on write fails the other way. One post from an account with 100M
followers would trigger 100M cache writes.

Twitter solved this with a hybrid: push to followers' Redis timelines on write, and
merge posts from very large accounts in at read time.

## Decision
The full algorithm is in `SPEC.md` §7.
- **Push** each post's ID into the author's own cached timeline and into each
  follower's, in batches of 1,000. This applies to authors below
  `--celebrity-threshold` (default 100,000 followers).
- **Pull** at read time: merge recent posts from followed accounts above the
  threshold, deduplicate, and keep the top N.
- Push only to timelines **already cached** (active users). On a cache miss, rebuild
  from storage.
- Cap cached timelines at 800 IDs. Deeper pages fall back to storage.
- **Delete** writes a tombstone. Deleted posts are filtered when timelines are loaded
  for display, never removed from every cached timeline.
- **Follow** merges the followee's 50 most recent posts into the cached timeline.
  **Unfollow** invalidates the cache.

## Consequences
- Write cost is bounded by the threshold. Read cost is bounded by the number of
  celebrities a user follows.
- Home timelines are eventually consistent: about 1 s from post to follower.
- A page can contain fewer than `limit` items after deleted posts are filtered, so
  clients follow `next` until it is null.
- Covered by tests: `crates/services/tests/flows.rs`.

## Revisit when
A ranked "For You" timeline is added. Ranking then sits on top of candidate retrieval,
for which this timeline is one source.
