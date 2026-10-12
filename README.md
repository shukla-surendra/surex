# my_social

A Twitter-style social network in Rust. It is built to run on one bare-metal box
today and to scale out to a sharded cluster without a rewrite.

- `docs/ARCHITECTURE.md`: target architecture and phased scaling plan
- `docs/SPEC.md`: product rules, HTTP API, consistency model, configuration
- `docs/aws/DEPLOYMENT.md`: AWS deployment & services plan with Mermaid diagrams
- `docs/adr/`: architecture decision records (why each technology was chosen, with
  Twitter's stack as prior art)

## Run

```sh
cargo run --release -p social            # http://127.0.0.1:3000, data in ./social.db
cargo run --release -p social -- --help  # all flags (also listed in SPEC §10)
```

## Test

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
scripts/loadtest.sh                      # needs `cargo install oha`; start with --no-rate-limit
```

## Layout

| Path | What |
|---|---|
| `crates/core` | IDs (Snowflake), models, events, errors, validation |
| `crates/storage` | Storage traits |
| `crates/storage-sqlite` | SQLite implementation |
| `crates/cache` | Timeline cache trait + in-process implementation |
| `crates/events` | Event bus trait + in-process implementation |
| `crates/services` | Business logic, fan-out and counter workers |
| `crates/gateway` | HTTP API (axum) |
| `bin/social` | The single server binary |
| `web/index.html` | Web client (embedded in the binary) |

## Production builds

`.cargo/config.toml` uses `target-cpu=native` for local builds. For servers, build for
the fleet's CPU instead (ARCHITECTURE §7.2):

```sh
RUSTFLAGS="-C target-cpu=x86-64-v4" cargo build --release -p social
```
