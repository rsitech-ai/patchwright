#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="$(mktemp -d)"
SOCKET="$STATE_DIR/engine.sock"
DATABASE="$STATE_DIR/engine.sqlite3"
cleanup() { test -z "${ENGINE_PID:-}" || kill "$ENGINE_PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; }
trap cleanup EXIT
cd "$ROOT_DIR"
cargo build -p patchwright-engine
target/debug/patchwright-engine serve --socket "$SOCKET" --database "$DATABASE" &
ENGINE_PID=$!
for _ in {1..100}; do test -S "$SOCKET" && break; sleep 0.05; done
RESPONSE="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}' | nc -U "$SOCKET")"
echo "$RESPONSE" | grep -q '"status":"ok"'
test -s "$DATABASE"

