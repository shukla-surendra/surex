# 0007: Search: Quickwit / Tantivy

- Status: Accepted, planned for Phase 2
- Date: 2026-10-12

## Context
Post search is append-only, time-ordered, very high volume, and mostly asks for recent
results. Twitter built **Earlybird**, a real-time search system on Lucene, for this.

## Decision
- Use **Quickwit**, built on **Tantivy** (a Rust library similar to Lucene).
- An indexer consumes `post_created` and `post_deleted` events from the log
  ([0004](0004-event-log.md)).
- Index segments live on object storage (MinIO or Ceph).

## Alternatives considered
| Option | Verdict |
|---|---|
| **Quickwit / Tantivy** | **Chosen.** Rust, search engine that keeps its indexes on object storage, which is cheap for append-only data. Designed for logs and event data, which matches the shape of posts |
| Elasticsearch / OpenSearch | Feature-rich, but heavy JVM clusters and expensive at this volume on metal |
| Meilisearch | Excellent search-as-you-type for small datasets. Not built for billions of documents |
| Postgres full-text | Fine at Phase 1 scale, not beyond |

## Consequences
- Search results lag writes by the indexing interval (seconds).
- Usernames and typeahead may need a separate small index (or Meilisearch) for
  prefix matching.
