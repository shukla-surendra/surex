# 0006: 64-bit Snowflake IDs, serialized as strings

- Status: Accepted, implemented in Phase 0
- Date: 2026-10-12

## Context
Auto-increment IDs need a single coordinator, which breaks the moment data is sharded
or several processes write. IDs also serve as the pagination cursor and the sort key,
so they should be ordered by time. Twitter invented Snowflake (2010) for exactly this
reason.

## Decision
- 64-bit layout: 41-bit milliseconds since 2026-01-01 UTC, a 10-bit worker ID, and a
  12-bit sequence.
- The generator is lock-free (atomic compare-and-swap). If the clock goes backwards or
  a millisecond's sequence numbers run out, it borrows from the next logical
  millisecond rather than blocking or producing duplicates.
- Each process gets a unique `--worker-id` (0–1023).
- IDs are **serialized as JSON strings**, because JavaScript numbers lose precision
  above 2^53.

## Alternatives considered
- **UUIDv4:** rejected. 128-bit and random, so poor index locality and no time ordering.
- **UUIDv7 / ULID:** time-ordered, but 128-bit, which doubles key size in every index
  and cache entry.
- **Database sequences:** rejected. A central bottleneck that cannot be sharded.

## Consequences
- About 4M IDs per second per worker, and 69 years of range from 2026.
- Worker IDs must be assigned without collisions (configuration now; a coordination
  service or lease later).
- An ID reveals its creation time. That is acceptable for posts, which are public with
  timestamps anyway.
