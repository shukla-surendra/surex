# my_social

A Twitter-style social network in Rust. It is built to run on one bare-metal box
today and to scale out to a sharded cluster without a rewrite.

- `docs/ARCHITECTURE.md`: target architecture and phased scaling plan
- `docs/SPEC.md`: product rules, HTTP API, consistency model, configuration
- `docs/aws/DEPLOYMENT.md`: AWS deployment & services plan with Mermaid diagrams
- `docs/adr/`: architecture decision records (why each technology was chosen, with
  Twitter's stack as prior art)

## Run

The backend (`bin/social`) is a JSON API under `/api/v1`. The frontend (`frontend/`)
is a React app that calls it on the same origin.

```sh
cargo run --release -p social            # API on http://127.0.0.1:3000, data in ./social.db
cargo run --release -p social -- --help  # all flags (also listed in SPEC §10)

cd frontend && npm install && npm run dev  # UI on http://localhost:5173, proxies /api to :3000
```

### With Docker Compose (frontend + backend + ScyllaDB)

```sh
docker compose up --build        # UI on http://localhost:8080, API also on 127.0.0.1:3000
docker compose exec scylla cqlsh # inspect the `social` keyspace
docker compose down -v           # stop and delete the Scylla data volume
```

The image is built for a generic CPU. Pass a specific target for production builds, e.g.
`docker build --build-arg TARGET_CPU=neoverse-n1 .` for Graviton.

### With ScyllaDB (app on the host)

```sh
# Local single-node Scylla. --broadcast-rpc-address lets the driver reach it from the host.
docker run -d --name scylla -p 9042:9042 scylladb/scylla:2025.1 \
  --smp 1 --memory 1G --overprovisioned 1 --developer-mode 1 \
  --broadcast-rpc-address 127.0.0.1 --rpc-address 0.0.0.0

cargo run --release -p social -- --store scylla   # creates keyspace `social` on first start
```

Scylla options (`--scylla-nodes`, `--scylla-rf`, `--scylla-dc`, ...) are in SPEC §10.
The cache and event bus are still in-process, so run one instance with `--roles=all`
until the Dragonfly and Redpanda backends land.

## Test

```sh
cargo test --workspace
SOCIAL_TEST_SCYLLA=127.0.0.1:9042 cargo test -p social-services   # same flows on Scylla
cargo clippy --workspace --all-targets
scripts/loadtest.sh                      # needs `cargo install oha`; start with --no-rate-limit
```

## Layout

| Path | What |
|---|---|
| `crates/core` | IDs (Snowflake), models, events, errors, validation |
| `crates/storage` | Storage traits |
| `crates/storage-sqlite` | SQLite implementation |
| `crates/storage-scylla` | ScyllaDB implementation |
| `crates/cache` | Timeline cache trait + in-process implementation |
| `crates/events` | Event bus trait + in-process implementation |
| `crates/services` | Business logic, fan-out and counter workers |
| `crates/gateway` | HTTP API (axum) |
| `bin/social` | The single server binary |
| `frontend/` | React + TypeScript web client (Vite); nginx serves it in Docker |

## Production builds

`.cargo/config.toml` uses `target-cpu=native` for local builds. For servers, build for
the fleet's CPU instead (ARCHITECTURE §7.2):

```sh
RUSTFLAGS="-C target-cpu=x86-64-v4" cargo build --release -p social
```
