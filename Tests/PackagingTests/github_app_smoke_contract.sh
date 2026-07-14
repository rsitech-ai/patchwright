#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SMOKE="$ROOT_DIR/script/smoke_github_app.sh"

assert_blocked() {
  local expected="$1"
  shift
  local output status
  set +e
  output="$(env "$@" bash "$SMOKE" 2>&1)"
  status=$?
  set -e
  [[ "$status" == 78 ]] || {
    printf 'expected blocked exit 78, received %s\n%s\n' "$status" "$output" >&2
    exit 1
  }
  grep -Fq "$expected" <<<"$output" || {
    printf 'missing expected boundary %q\n%s\n' "$expected" "$output" >&2
    exit 1
  }
}

assert_blocked \
  "the Patchwright production repository is forbidden" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=s1korrrr \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=patchwright

assert_blocked \
  "PATCHWRIGHT_GITHUB_E2E_ALLOWLIST must exactly equal example/qualification" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=example \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=qualification

assert_blocked \
  "set PATCHWRIGHT_GITHUB_E2E_CONFIRM=authorize:example/qualification for this one run" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=example \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=qualification \
  PATCHWRIGHT_GITHUB_E2E_ALLOWLIST=example/qualification

printf 'GitHub App smoke safety contract passed\n'
