#!/usr/bin/env bash
# Seeds a running server and load-tests the hot read paths with oha
# (`cargo install oha`). Usage: scripts/loadtest.sh [base_url] [duration]
#
# Start the server with per-IP limits off, or a single load generator just measures
# the 429 path:
#   cargo run --release -p social -- --no-rate-limit --db /tmp/load.db
set -euo pipefail

BASE=${1:-http://127.0.0.1:3000}
DURATION=${2:-20s}
API=$BASE/api/v1
JAR=$(mktemp -d)
trap 'rm -rf "$JAR"' EXIT
J='content-type: application/json'

command -v oha >/dev/null || { echo "oha not found: cargo install oha" >&2; exit 1; }

name=load_$RANDOM
curl -sf -c "$JAR/c" -H "$J" -d "{\"username\":\"$name\",\"password\":\"password123\"}" "$API/auth/register" >/dev/null
for i in $(seq 1 25); do
  curl -sf -b "$JAR/c" -H "$J" -d "{\"body\":\"load test post $i\"}" "$API/posts" >/dev/null
done
COOKIE=$(awk '$6 == "sid" { print "sid=" $7 }' "$JAR/c")

echo "== global timeline (anonymous)"
oha -z "$DURATION" -c 64 --no-tui "$API/timeline/global"

echo "== home timeline (authenticated, cache hit path)"
oha -z "$DURATION" -c 64 --no-tui -H "cookie: $COOKIE" "$API/timeline/home"
