#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

./script/verify_codex_schema.sh
codex login status 2>&1 | grep -q 'Logged in'
cargo test -p patchwright-engine --test codex_protocol --test codex_process --test codex_session --test codex_rpc --test codex_approvals --test codex_cancellation
cargo test -p patchwright-engine --test real_codex -- --ignored --nocapture
