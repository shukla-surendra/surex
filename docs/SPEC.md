# my_social — Product & API Specification (v1)

Status: **v1, implemented in Phase 0** (see `ARCHITECTURE.md` §9).
Words like MUST and SHOULD follow RFC 2119.

---

## 1. Scope

v1 is a microblogging service:

- Accounts: register, log in, log out.
- Posts: create (≤ 280 chars, optional reply target), read, delete own.
- Likes on posts.
- Following other users.
- Timelines:
  - **Home:** posts from people you follow, plus your own.
  - **Global:** all posts.
  - **Profile:** one user's posts.
- Public profiles with follower, following and post counts.

Out of scope for v1 (planned later):
- Reposts and quotes
- Media
- Search
- Notifications
- Real-time push
- DMs
- Blocks and mutes
- Account deletion and export
- Ranked "For You" timeline
- Mobile token auth

---

## 2. Domain rules and limits

| Item | Rule |
|---|---|
| Username | 3–20 bytes, `[A-Za-z0-9_]`, unique **case-insensitively**, immutable |
| Password | 8–128 bytes. Stored only as an Argon2id hash |
| Post body | Trimmed, 1–280 Unicode scalar values. Control characters other than `\n` and `\t` are rejected |
| Reply | `reply_to` MUST reference an existing, non-deleted post |
| Following | At most **5,000** accounts per user. Users cannot follow themselves |
| Page size | `limit` default 30, clamped to 1–100 |
| Session lifetime | 30 days from login |
| Request body | ≤ 16 KiB |

### 2.1 Identifiers
All entity IDs are 64-bit **Snowflake** IDs:

```
 63      62 ........................ 22   21 ........ 12   11 ........ 0
 [0] [ 41-bit ms since 2026-01-01T00:00:00Z ] [ 10-bit worker ] [ 12-bit sequence ]
```

- Unique across up to 1,024 concurrently running ID generators (`--worker-id`).
- Strictly increasing per generator, roughly time-ordered globally. `ORDER BY id`
  equals creation order.
- **Serialized as JSON strings** (`"123456789"`), because JS numbers lose precision
  above 2^53.

### 2.2 Timestamps
Unix epoch **milliseconds**, as JSON numbers. A post's `created_at` is derived from its
ID.

---

## 3. Consistency model

| Operation | Guarantee |
|---|---|
| Register, follow, unfollow, like, unlike, create post, delete post | Durable and immediately visible to the actor once the response returns |
| Follower, following and post counts | Updated in the same transaction as the change (single-store mode) |
| Like counts (`like_count`) | **Eventually consistent**, normally within ~1 s. May briefly lag the actor's own `liked` flag |
| Home timeline | **Eventually consistent**. New posts reach followers' timelines asynchronously, normally within ~1 s |
| Deleted posts | Never returned once the delete response returns (they are filtered when posts are loaded for display) |
| Global and profile timelines | Read directly from storage, so consistent once the write returns |

Because filtering happens when posts are loaded, a page MAY contain fewer than `limit`
items even when more exist. Clients MUST keep paging while `next` is non-null.

---

## 4. HTTP API

Base path: `/api/v1`. Bodies are JSON (`Content-Type: application/json`).

### 4.1 Authentication
- Session cookie `sid` set on register and login: `HttpOnly; SameSite=Strict; Path=/`,
  plus `Secure` when `--secure-cookies` is on.
- The server stores only `SHA-256(token)`. The raw token is 256 bits of OS randomness.
- Endpoints marked 🔒 require a valid session (`401 unauthorized` otherwise).
- Endpoints marked 👁 work anonymously but personalize the response (`liked`,
  `followed_by_me`) when a session is present.

### 4.2 Pagination
Cursor-based, newest first: `?before=<post_id>&limit=<n>`. The response is:

```json
{ "items": [ ... ], "next": "7391...|null" }
```

Pass `next` as `before` to fetch the following page. `next: null` means the end.

### 4.3 Errors
Every non-2xx response has this shape:

```json
{ "error": { "code": "invalid_request", "message": "post must be 1-280 characters" } }
```

| HTTP | `code` | When |
|---|---|---|
| 400 | `invalid_request` | Validation failure, malformed JSON, bad path or query |
| 401 | `unauthorized` | Missing, expired or invalid session |
| 401 | `invalid_credentials` | Wrong username or password (indistinguishable, timing-equalized) |
| 403 | `forbidden` | Acting on another user's resource |
| 404 | `not_found` | Unknown user, post or route |
| 409 | `conflict` | Username taken |
| 413 | — | Body > 16 KiB (plain response from the body limiter) |
| 429 | `rate_limited` | Rate limit exceeded. A `Retry-After` header (seconds) is present |
| 500 | `internal` | Unexpected failure (details are logged, never returned) |
| 503 | `unavailable` | Backend saturated or unavailable |

Every response carries `x-request-id`. It echoes the client's value or is a generated
UUID, and appears in server logs.

### 4.4 Objects

```jsonc
// User (display_name and bio are "" when unset)
{ "id": "string", "username": "alice", "created_at": 1791736976000,
  "display_name": "Alice A.", "bio": "Builds things." }

// Profile = User + counts
{ "id": "…", "username": "alice", "created_at": 0,
  "followers": 10, "following": 3, "posts": 42, "followed_by_me": false }

// Post (replying_to is the parent author's username, for replies)
{ "id": "…", "author": User, "body": "hello", "reply_to": "…|null",
  "replying_to": "bob|null", "created_at": 0, "like_count": 3, "reply_count": 2,
  "liked": false, "media": [Media] }

// Media (an image; url serves the bytes)
{ "id": "…", "url": "/api/v1/media/…", "content_type": "image/png",
  "width": 960, "height": 640, "alt": "A red tomato" }
```

### 4.5 Endpoints

| Method & path | Auth | Request | Success |
|---|---|---|---|
| `POST /auth/register` | — | `{username, password}` | `201` User + `Set-Cookie` |
| `POST /auth/login` | — | `{username, password}` | `200` User + `Set-Cookie` |
| `POST /auth/logout` | — | — | `204`, cookie cleared, session revoked |
| `GET /me` | 🔒 | — | `200` User |
| `PATCH /me` | 🔒 | `{display_name, bio}` (≤ 50 / ≤ 160 chars; `""` clears) | `200` User |
| `GET /timeline/home` | 🔒 | `?before&limit` | `200` Page<Post> |
| `GET /timeline/global` | 👁 | `?before&limit` | `200` Page<Post> |
| `POST /media` | 🔒 | raw image bytes (PNG, JPEG, GIF or WebP; ≤ 5 MB; ≤ 8000 px a side) | `201` Media (`alt` empty) |
| `GET /media/{id}` | — | — | `200` image bytes, cacheable forever |
| `POST /posts` | 🔒 | `{body, reply_to?, media?: [{id, alt}]}` (≤ 4 own images; `body` may be empty with images) | `201` Post |
| `GET /posts/{id}` | 👁 | — | `200` Post |
| `GET /posts/{id}/replies` | 👁 | `?after&limit` | `200` Page<Post>, oldest first |
| `DELETE /posts/{id}` | 🔒 author | — | `204` |
| `PUT /posts/{id}/like` | 🔒 | — | `204` (idempotent) |
| `DELETE /posts/{id}/like` | 🔒 | — | `204` (idempotent) |
| `GET /users/{username}` | 👁 | — | `200` Profile |
| `GET /users/{username}/posts` | 👁 | `?before&limit` | `200` Page<Post> |
| `PUT /users/{username}/follow` | 🔒 | — | `204` (idempotent) |
| `DELETE /users/{username}/follow` | 🔒 | — | `204` (idempotent) |
| `GET /users/{username}/followers` | — | `?after&limit` | `200` Page<User>, ascending id |
| `GET /users/{username}/following` | — | `?after&limit` | `200` Page<User>, ascending id |

Timelines page backwards with `before`; replies and user lists page forwards with
`after`. In both cases pass the previous page's `next` value.

Operational endpoint (outside `/api/v1`): `GET /healthz` (`200 ok`). The web client is
a separate React app (`frontend/`) served from the same origin, which proxies `/api`
here.

Likes and follows use **PUT/DELETE** (set and unset) rather than a toggle. Retries are
safe, and clients never race themselves.

---

## 5. Rate limits

Token buckets per client IP. Behind a proxy, set `--trust-proxy` to use the last
`X-Forwarded-For` entry.

| Class | Applies to | Burst | Refill |
|---|---|---|---|
| auth | `POST /auth/register`, `POST /auth/login` | 10 | 10 / min |
| write | other non-GET requests | 30 | 1 / s |
| read | GET / HEAD | 300 | 50 / s |

Single-node limits are local. A cluster needs shared buckets in Dragonfly
(ARCHITECTURE §8).

---

## 6. Events

Every state change publishes an event to the bus after the write commits. Consumers
are idempotent.

```jsonc
{ "type": "post_created", "post_id": "…", "author_id": "…" }
{ "type": "post_deleted", "post_id": "…", "author_id": "…" }
{ "type": "followed",     "follower": "…", "followee": "…" }
{ "type": "unfollowed",   "follower": "…", "followee": "…" }
{ "type": "liked",        "post_id": "…", "user_id": "…" }
{ "type": "unliked",      "post_id": "…", "user_id": "…" }
```

| Consumer group | Consumes | Effect |
|---|---|---|
| `fanout` | `post_created`, `followed`, `unfollowed` | Maintains cached home timelines |
| `counters` | `liked`, `unliked` | Batches like-count deltas (flush every 500 ms or 1,000 posts) |

Partition key at cluster scale: `author_id` / `follower` / `post_id` respectively.

---

## 7. Home timeline algorithm

1. **Cache:** each *active* user's home timeline is a list of post IDs, newest first,
   capped at `timeline_cap` (default 800).
2. **Write path (fan-out on write):** on `post_created`, push the ID to the author's own
   timeline. If the author has fewer than `celebrity_threshold` followers (default
   100,000), also push it to every follower's timeline, in batches of 1,000.
   - Pushes only affect timelines already in the cache, so inactive users cost nothing.
3. **Read path:**
   1. Read the cache. On a miss, rebuild it from storage (following + self, newest
      `timeline_cap`).
   2. If the page runs past the cache cap, continue from storage.
   3. **Fan-out on read:** merge in posts from followed accounts at or above the
      celebrity threshold, de-duplicate, and keep the newest `limit`.
   4. Load full posts, authors and the viewer's like state in batched queries. Drop
      deleted or missing posts.
4. **Follow:** merge the followee's 50 most recent posts into the follower's cached
   timeline. **Unfollow:** invalidate the follower's cache (it is rebuilt on next read).

---

## 8. Security requirements

- Passwords use Argon2id (default parameters: 19 MiB, t=2, p=1). Hashing runs on the
  blocking pool, with concurrency bounded to the CPU count.
- Login timing is equalized. Unknown usernames still verify against a dummy hash.
- Session tokens are stored hashed. Expired sessions are purged every 10 minutes.
- CSRF: `SameSite=Strict` cookie. Mutating endpoints that take a body require a JSON
  content type.
- XSS: the API returns raw text. Clients MUST render it as text, never as HTML.
- SQL: parameterized statements only.
- Internal error details are never returned to clients.

---

## 9. Non-functional requirements

| Requirement | Target |
|---|---|
| Home timeline p99 (cache hit, single box) | < 20 ms server time |
| Create post p99 | < 30 ms server time (excludes async fan-out) |
| Fan-out latency p99 (≤ 10k followers) | < 1 s |
| Availability (Phase 2+) | 99.95% |
| Graceful shutdown | Stop accepting requests, finish in-flight ones, drain workers, flush counters |
| Observability | Structured logs (`--log-json`) with request ID, method, path, status, latency |

---

## 10. Configuration (`social` binary)

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--addr` | `SOCIAL_ADDR` | `127.0.0.1:3000` | Listen address |
| `--roles` | `SOCIAL_ROLES` | `all` | Any of `all,gateway,fanout,counters` |
| `--store` | `SOCIAL_STORE` | `sqlite` | Storage backend: `sqlite` or `scylla` |
| `--cache` | — | `inproc` | Timeline cache (`dragonfly` planned) |
| `--bus` | — | `inproc` | Event bus (`redpanda` planned). `inproc` requires `--roles=all` |
| `--db` | `SOCIAL_DB` | `social.db` | SQLite path |
| `--db-pool` | — | `16` | SQLite connection pool size |
| `--media-dir` | `SOCIAL_MEDIA_DIR` | `media` | Directory for uploaded images |
| `--scylla-nodes` | `SOCIAL_SCYLLA_NODES` | `127.0.0.1:9042` | Scylla contact points, comma-separated |
| `--scylla-keyspace` | `SOCIAL_SCYLLA_KEYSPACE` | `social` | Keyspace; created with all tables on first start |
| `--scylla-rf` | `SOCIAL_SCYLLA_RF` | `1` | Replication factor, used only when the keyspace is created (3 in production) |
| `--scylla-dc` | `SOCIAL_SCYLLA_DC` | — | Local datacenter for routing and `LOCAL_QUORUM` |
| `--scylla-user` | `SOCIAL_SCYLLA_USER` | — | Scylla username; password from `SOCIAL_SCYLLA_PASSWORD` |
| `--worker-id` | `SOCIAL_WORKER_ID` | `0` | Snowflake worker ID (0–1023), unique per process |
| `--celebrity-threshold` | — | `100000` | Followers at or above this count are fanned out on read |
| `--timeline-cap` | — | `800` | Post IDs kept per cached timeline |
| `--cache-users` | — | `200000` | Max cached timelines (TinyLFU eviction) |
| `--secure-cookies` | — | off | Add `Secure` to the session cookie (enable behind TLS) |
| `--trust-proxy` | `SOCIAL_TRUST_PROXY` | off | Use `X-Forwarded-For` for the client IP |
| `--no-rate-limit` | `SOCIAL_NO_RATE_LIMIT` | off | Disable per-IP limits (benchmarks, or limits enforced at the edge) |
| `--log-json` | — | off | JSON logs |
| `RUST_LOG` | env | `info` | Log filter |

---

## 11. Known limitations (v1)

- In-process cache and bus: events in flight at a crash are lost.
  - Timelines recover automatically, because the cache is rebuilt from storage.
  - Up to ~500 ms of like-count deltas can be lost. A reconciliation job is planned.
- The global timeline reads storage directly. At scale it becomes a cached, sampled
  stream.
- Rate limits are per-node.
- Images live on the server's local disk (`--media-dir`), so only one app node can
  serve them. Images of deleted posts, and uploads never attached to a post, are not
  garbage-collected yet. Object storage (S3/MinIO) behind a CDN replaces this at
  cluster scale.
- ScyllaDB backend:
  - Multi-table writes (a follow, a like, a post plus its counters) are not atomic
    across tables. A crash between statements can leave a counter off by one, or an
    edge in one direction only. A reconciliation job is planned.
  - The global timeline scans one `posts_by_day` partition per day, newest first.
    Every post of a day lands in one partition, so this is a hot spot at very high
    write rates.
  - Expired sessions are removed by Scylla TTL, not by the purge task.

- `--trust-proxy` trusts exactly one proxy hop (the last `X-Forwarded-For` entry).
  Behind two hops, such as CloudFront plus an ALB, that entry is the CDN edge's IP,
  not the client's. A configurable hop count is required before such deployments
  (`aws/DEPLOYMENT.md` §11).
