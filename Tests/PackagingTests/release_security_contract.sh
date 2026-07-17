#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-release-security-contract.XXXXXX")"
TMP_ROOT="$(cd "$TMP_ROOT" && pwd -P)"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  printf 'release security contract failed: %s\n' "$*" >&2
  exit 1
}

assert_rejected() {
  local expected="$1"
  shift
  local output="$TMP_ROOT/rejected-$RANDOM.out"
  if "$@" >"$output" 2>&1; then
    fail "command unexpectedly accepted: $expected"
  fi
  grep -Fq "$expected" "$output" || fail "rejection was not explicit: $expected"
}

CHECKSUM_VERIFIER="$ROOT_DIR/script/verify_checksum_sidecar.py"
SOURCE_VERIFIER="$ROOT_DIR/script/verify_release_source.py"
NOTARY_PARSER="$ROOT_DIR/script/parse_notary_log.py"
EVIDENCE_WRITER="$ROOT_DIR/script/write_owner_evidence.py"
[[ -x "$CHECKSUM_VERIFIER" ]] || fail "missing executable checksum verifier"
[[ -x "$SOURCE_VERIFIER" ]] || fail "missing executable source verifier"
[[ -x "$NOTARY_PARSER" ]] || fail "missing executable notary log parser"
[[ -x "$EVIDENCE_WRITER" ]] || fail "missing executable owner evidence writer"

DMG="$TMP_ROOT/Patchwright-0.1.0.dmg"
SIDECAR="$DMG.sha256"
printf 'notarized fixture\n' >"$DMG"
DIGEST="$(shasum -a 256 "$DMG" | awk '{print $1}')"
printf '%s  %s\n' "$DIGEST" "$(basename "$DMG")" >"$SIDECAR"
SIDECAR_BEFORE="$(stat -f '%d:%i:%z:%m:%c' "$SIDECAR"):$(shasum -a 256 "$SIDECAR" | awk '{print $1}')"
"$CHECKSUM_VERIFIER" --artifact "$DMG" --sidecar "$SIDECAR"
SIDECAR_AFTER="$(stat -f '%d:%i:%z:%m:%c' "$SIDECAR"):$(shasum -a 256 "$SIDECAR" | awk '{print $1}')"
[[ "$SIDECAR_AFTER" == "$SIDECAR_BEFORE" ]] || fail "checksum verification mutated its sidecar"

printf '%064d  %s\n' 0 "$(basename "$DMG")" >"$TMP_ROOT/wrong.sha256"
assert_rejected "checksum sidecar digest does not match artifact" \
  "$CHECKSUM_VERIFIER" --artifact "$DMG" --sidecar "$TMP_ROOT/wrong.sha256"
ln -s "$SIDECAR" "$TMP_ROOT/symlink.sha256"
assert_rejected "checksum sidecar must be a regular non-symlink file" \
  "$CHECKSUM_VERIFIER" --artifact "$DMG" --sidecar "$TMP_ROOT/symlink.sha256"
ln -s "$DMG" "$TMP_ROOT/symlink.dmg"
assert_rejected "artifact must be a regular non-symlink file" \
  "$CHECKSUM_VERIFIER" --artifact "$TMP_ROOT/symlink.dmg" --sidecar "$SIDECAR"

grep -Fq 'CHECKSUM_PATH="${2:?existing checksum sidecar required}"' "$ROOT_DIR/script/verify_distribution.sh" \
  || fail "distribution verification must require an explicit checksum sidecar"
grep -Fq 'verify_checksum_sidecar.py" --artifact "$DMG_PATH" --sidecar "$CHECKSUM_PATH"' \
  "$ROOT_DIR/script/verify_distribution.sh" \
  || fail "distribution verification must delegate to the no-follow checksum verifier"
if grep -Eq '>[[:space:]]*"?\$CHECKSUM_PATH|>[[:space:]]*"?\$DMG_PATH\.sha256' "$ROOT_DIR/script/verify_distribution.sh"; then
  fail "distribution verification must not overwrite the checksum sidecar"
fi

REPO="$TMP_ROOT/repo"
mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.name Fixture
git -C "$REPO" config user.email fixture@example.invalid
printf 'source fixture\n' >"$REPO/README.md"
git -C "$REPO" add README.md
git -C "$REPO" commit -qm fixture
COMMIT="$(git -C "$REPO" rev-parse HEAD)"
git -C "$REPO" tag v0.1.0
ARCHIVE="$TMP_ROOT/source.tar.gz"
git -C "$REPO" archive --format=tar.gz --output="$ARCHIVE" "$COMMIT"
SOURCE_DIGEST="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
"$SOURCE_VERIFIER" --repo "$REPO" --commit "$COMMIT" --tag v0.1.0 \
  --source-archive "$ARCHIVE" --source-archive-sha256 "$SOURCE_DIGEST" >/dev/null

FORGED_ARCHIVE="$TMP_ROOT/forged-source.tar.gz"
printf 'unrelated source bytes\n' >"$TMP_ROOT/wrong-source.txt"
tar -czf "$FORGED_ARCHIVE" -C "$TMP_ROOT" wrong-source.txt
FORGED_DIGEST="$(shasum -a 256 "$FORGED_ARCHIVE" | awk '{print $1}')"
assert_rejected "source archive content differs from candidate commit" \
  "$SOURCE_VERIFIER" --repo "$REPO" --commit "$COMMIT" --tag v0.1.0 \
  --source-archive "$FORGED_ARCHIVE" --source-archive-sha256 "$FORGED_DIGEST"

printf 'dirty\n' >>"$REPO/README.md"
assert_rejected "release worktree differs from candidate commit" \
  "$SOURCE_VERIFIER" --repo "$REPO" --commit "$COMMIT" --tag v0.1.0 \
  --source-archive "$ARCHIVE" --source-archive-sha256 "$SOURCE_DIGEST"
git -C "$REPO" restore README.md
printf 'untracked\n' >"$REPO/untracked.txt"
assert_rejected "release worktree contains untracked files" \
  "$SOURCE_VERIFIER" --repo "$REPO" --commit "$COMMIT" --tag v0.1.0 \
  --source-archive "$ARCHIVE" --source-archive-sha256 "$SOURCE_DIGEST"
rm "$REPO/untracked.txt"
printf 'tamper\n' >>"$ARCHIVE"
assert_rejected "source archive digest mismatch" \
  "$SOURCE_VERIFIER" --repo "$REPO" --commit "$COMMIT" --tag v0.1.0 \
  --source-archive "$ARCHIVE" --source-archive-sha256 "$SOURCE_DIGEST"

ACCEPTED_LOG="$TMP_ROOT/notary-accepted.json"
WARNING_LOG="$TMP_ROOT/notary-warning.json"
ERROR_LOG="$TMP_ROOT/notary-error.json"
jq -n '{jobId:"private-id",statusSummary:"Ready for distribution",issues:[]}' >"$ACCEPTED_LOG"
jq -n '{issues:[{severity:"warning",code:"fixture-warning",message:"private path"}]}' >"$WARNING_LOG"
jq -n '{issues:[{severity:"error",code:"fixture-error",message:"private path"}]}' >"$ERROR_LOG"
ACCEPTED_LOG_DIGEST="$(shasum -a 256 "$ACCEPTED_LOG" | awk '{print $1}')"
"$NOTARY_PARSER" --log "$ACCEPTED_LOG" --warning-policy reject >"$TMP_ROOT/notary-summary.json"
jq -e --arg digest "$ACCEPTED_LOG_DIGEST" '
  .log_sha256 == $digest and .issue_count == 0 and .error_count == 0 and
  .warning_count == 0 and .info_count == 0 and .warning_policy == "reject"
' "$TMP_ROOT/notary-summary.json" >/dev/null || fail "accepted notary summary is invalid"
assert_rejected "notarization log contains 1 error issue" \
  "$NOTARY_PARSER" --log "$ERROR_LOG" --warning-policy allow
assert_rejected "notarization warning policy rejected 1 warning issue" \
  "$NOTARY_PARSER" --log "$WARNING_LOG" --warning-policy reject
"$NOTARY_PARSER" --log "$WARNING_LOG" --warning-policy allow >"$TMP_ROOT/notary-warning-summary.json"
jq -e '.warning_count == 1 and .warning_policy == "allow" and (has("issues") | not)' \
  "$TMP_ROOT/notary-warning-summary.json" >/dev/null || fail "allowed warning summary leaked or omitted data"

EVIDENCE_PARENT="$TMP_ROOT/private-state"
mkdir -m 700 "$EVIDENCE_PARENT"
EVIDENCE_DIR="$EVIDENCE_PARENT/evidence"
printf '{"result":"pass"}\n' | "$EVIDENCE_WRITER" \
  --directory "$EVIDENCE_DIR" --name github-app-e2e-20260717T120000Z.json
[[ "$(stat -f '%Lp' "$EVIDENCE_DIR")" == 700 ]] || fail "evidence directory mode is not 700"
EVIDENCE_FILE="$EVIDENCE_DIR/github-app-e2e-20260717T120000Z.json"
[[ "$(stat -f '%Lp' "$EVIDENCE_FILE")" == 600 ]] || fail "evidence file mode is not 600"
jq -e '.result == "pass"' "$EVIDENCE_FILE" >/dev/null || fail "evidence content is invalid"
assert_rejected "evidence file already exists" bash -c \
  'printf '\''{"result":"overwrite"}\n'\'' | "$1" --directory "$2" --name "$3"' \
  _ "$EVIDENCE_WRITER" "$EVIDENCE_DIR" "$(basename "$EVIDENCE_FILE")"
ln -s "$EVIDENCE_DIR" "$TMP_ROOT/evidence-link"
assert_rejected "evidence directory must not be a symlink" bash -c \
  'printf '\''{}\n'\'' | "$1" --directory "$2" --name new.json' \
  _ "$EVIDENCE_WRITER" "$TMP_ROOT/evidence-link"
mkdir "$TMP_ROOT/loose-evidence"
assert_rejected "evidence directory must have mode 700" bash -c \
  'printf '\''{}\n'\'' | "$1" --directory "$2" --name new.json' \
  _ "$EVIDENCE_WRITER" "$TMP_ROOT/loose-evidence"
assert_rejected "evidence directory must be an absolute canonical path" bash -c \
  'printf '\''{}\n'\'' | "$1" --directory relative/evidence --name new.json' \
  _ "$EVIDENCE_WRITER"

python3 - "$ROOT_DIR/script/smoke_github_app.sh" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_text(encoding="utf-8")
definition = source.index("fail() {")
first_call = source.index('fail "')
if definition > first_call:
    raise SystemExit("release security contract failed: fail must be defined before its first use")
if "write_owner_evidence.py" not in source:
    raise SystemExit("release security contract failed: GitHub smoke must use the atomic owner evidence writer")
if '>"$EVIDENCE"' in source:
    raise SystemExit("release security contract failed: GitHub smoke must not create evidence with shell truncation")
PY

grep -Fq 'patchwright-relay -- serve' "$ROOT_DIR/README.md" \
  || fail "README relay command must include the serve subcommand"
grep -Fq 'PATCHWRIGHT_GITHUB_WEBHOOK_SECRET_FILE' "$ROOT_DIR/README.md" \
  || fail "README must use a webhook secret file, not a raw secret environment value"
grep -Fq 'does not automatically redeliver' "$ROOT_DIR/docs/production-plan.md" \
  || fail "operations docs must state GitHub automatic redelivery truth"
grep -Fq 'mode `0400` or `0600`' "$ROOT_DIR/docs/security.md" \
  || fail "security docs must require an owner-only relay secret file"
grep -Fq 'Tests/PackagingTests/release_security_contract.sh' "$ROOT_DIR/script/verify.sh" \
  || fail "the repository verification entrypoint must run the release security contract"

printf 'Patchwright release security contract passed\n'
