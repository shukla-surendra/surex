# 0010: Eventually consistent, batched like counters

- Status: Accepted, implemented in Phase 0
- Date: 2026-10-12

## Context
Popular posts receive thousands of likes per second. Updating one counter row
synchronously on every like creates a hot partition and lock contention.

## Decision
- A like or unlike writes only the like record, which is idempotent: `PUT` and
  `DELETE /posts/{id}/like`.
- It then emits an event. A counter worker aggregates the deltas per post and flushes
  every 500 ms, or sooner once 1,000 posts have pending changes.
- The viewer's own `liked` flag is read from the like record, so it is immediately
  consistent.

## Consequences
- `like_count` lags by about 1 s. That is acceptable for social counts.
- With the in-process bus, up to about 500 ms of deltas can be lost on a crash. A
  reconciliation job is planned (`SPEC.md` §11).
- Follower, following and post counts stay transactional in single-store mode.
  They will move to the same event-driven scheme on Scylla.
