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
  "the Patchwright production repository is forbidden" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=S1KORRRR \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=Patchwright \
  PATCHWRIGHT_GITHUB_E2E_ALLOWLIST=S1KORRRR/Patchwright \
  PATCHWRIGHT_GITHUB_E2E_CONFIRM=authorize:S1KORRRR/Patchwright

assert_blocked \
  "PATCHWRIGHT_GITHUB_E2E_ALLOWLIST must exactly equal example/qualification" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=example \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=qualification

assert_blocked \
  "set PATCHWRIGHT_GITHUB_E2E_CONFIRM=authorize:example/qualification for this one run" \
  PATCHWRIGHT_GITHUB_E2E_OWNER=example \
  PATCHWRIGHT_GITHUB_E2E_REPOSITORY=qualification \
  PATCHWRIGHT_GITHUB_E2E_ALLOWLIST=example/qualification

grep -Fq -- '--arg expectedHeadSha "$PR_HEAD_SHA"' "$SMOKE" || {
  echo "live review smoke must bind the review action to the captured head SHA" >&2
  exit 1
}
grep -Fq 'kind:"review",pullRequestNumber:$pullRequestNumber,expectedHeadSha:$expectedHeadSha' "$SMOKE" || {
  echo "live review smoke must include expectedHeadSha inside the review action" >&2
  exit 1
}
grep -Fq -- '--arg expectedBaseSha "$BASE_SHA"' "$SMOKE" || {
  echo "live draft PR smoke must bind the action to the captured base SHA" >&2
  exit 1
}
grep -Fq 'kind:"draftPullRequest",title:$title,head:$head,base:$base,expectedBaseSha:$expectedBaseSha' "$SMOKE" || {
  echo "live draft PR smoke must include expectedBaseSha inside the action" >&2
  exit 1
}
cargo test --manifest-path "$ROOT_DIR/Cargo.toml" -p patchwright-core \
  --test github_action_contract github_app_smoke_draft_action_deserializes --quiet

printf 'GitHub App smoke safety contract passed\n'
