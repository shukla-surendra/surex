# Demo users

Dummy accounts for local development and demos, created by the fixture in
[`fixtures/demo.json`](../fixtures/demo.json). **They are not real people, and the
password is public: never load this fixture into a production deployment.**

surex accounts have a **username and password only**; there is no email field.

All demo accounts share one password:

```
surex-demo-123
```

| Username | Display name | Profile | Good for trying |
|---|---|---|---|
| `ada_codes` | Ada Okafor | Backend engineer | Busy home feed, a post with a reply thread |
| `kai_ships` | Kai Nakamura | Product designer | Multi-line bio, mutual follows |
| `leo_draws` | Leo Marchetti | Illustrator | Posts with links |
| `sana_runs` | Sana Iqbal | Trail runner, analyst | Replies with @mentions |
| `mira_reads` | Mira Lindqvist | Librarian | A post with several replies, nested reply chain |
| `omar_cooks` | Omar Haddad | Home cook | Replying across threads |
| `zoe_plants` | Zoe Fontaine | Balcony gardener | Most-liked post |
| `newbie_nate` | *(none)* | New user, empty profile | Empty-profile and first-post states |

The fixture also creates 20 follows, 14 posts, 10 replies (including reply-to-a-reply
chains) and 32 likes.

## Images

Fixture posts carry images from [`fixtures/images/`](../fixtures/images/): twelve
illustrations drawn by `scripts/gen_fixture_images.py` (no stock photos, no external
downloads), each with alt text.

- `demo.json`: eight posts get a matching picture (Sana's sunrise run, Zoe's tomato,
  Omar's soup, Ada's latency chart, ...).
- [`fixtures/media.json`](../fixtures/media.json): extra posts that show every layout:
  four images (`leo_draws`), three (`omar_cooks`), two (`kai_ships`), an image-only post
  (`zoe_plants`), single images, and a reply with an image (`mira_reads`). It loads once
  (skipped when Leo's "Sketchbook dump" post exists), so it can be added to a database
  that already has the other fixtures.
- `pagination.json`: every 6th generated post by Ada and a few of Sana's run logs carry
  an image.

## Pagination data

[`fixtures/pagination.json`](../fixtures/pagination.json) adds generated bulk data so
every paged view has more than one page (pages hold 30 items):

| Where to look | What you get |
|---|---|
| `@ada_codes` → followers | 40 followers: the 4 demo users plus `crowd_01` … `crowd_36` |
| `@ada_codes` profile | 39 posts |
| Home feed, logged in as `ada_codes` | Posts from kai, sana and mira; 80+ items |
| Everyone feed | 100+ posts |
| `@kai_ships`'s "Design rule…" post | A thread with 42 replies ("Show more replies") |

The crowd users (`crowd_01` … `crowd_36`) use the same password. The bulk set is
generated from templates in the fixture and is loaded once: it is skipped if
`crowd_01` already exists.

## Loading the fixture

Start the backend (any store), then run the seeder. It only uses the HTTP API and
loads `demo.json`, `pagination.json`, then `media.json` by default.

```sh
# Turn off rate limits while seeding (a few hundred writes), then turn them back on.
SOCIAL_NO_RATE_LIMIT=true docker compose up -d social
python3 scripts/seed.py              # API at http://127.0.0.1:3000 by default
docker compose up -d social          # rate limits back on

python3 scripts/seed.py fixtures/demo.json   # only the small hand-written set
```

Without Docker, run the backend with `cargo run -p social -- --no-rate-limit`. With rate limits on,
the seeder still works but waits them out (several minutes for the bulk set).
Re-running is safe: existing users are logged in rather than recreated, follows and
likes are idempotent, a hand-written post is only created when its author has none
yet, and the bulk set is skipped once loaded.

To start from a clean database first: `docker compose down -v && docker compose up -d`
(this deletes **all** data in the local Scylla volume).
