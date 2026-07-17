#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="$(mktemp -d)"
SOCKET="$STATE_DIR/engine.sock"
DATABASE="$STATE_DIR/engine.sqlite3"
stop_engine() {
  local pid="${ENGINE_PID:-}"
  [[ -n "$pid" ]] || return 0
  if kill -0 "$pid" >/dev/null 2>&1; then
    kill -TERM "$pid" >/dev/null 2>&1 || true
    for _ in {1..40}; do
      kill -0 "$pid" >/dev/null 2>&1 || break
      sleep 0.05
    done
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill -KILL "$pid" >/dev/null 2>&1 || true
    fi
  fi
  wait "$pid" >/dev/null 2>&1 || true
  ENGINE_PID=""
}
cleanup() { stop_engine; rm -rf "$STATE_DIR"; }
trap cleanup EXIT
cd "$ROOT_DIR"
cargo build -p patchwright-engine
target/debug/patchwright-engine serve --socket "$SOCKET" --database "$DATABASE" &
ENGINE_PID=$!
for _ in {1..100}; do test -S "$SOCKET" && break; sleep 0.05; done
RESPONSE="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}' | nc -U "$SOCKET")"
echo "$RESPONSE" | grep -q '"status":"ok"'
test -s "$DATABASE"
stop_engine
test ! -e "$SOCKET"
