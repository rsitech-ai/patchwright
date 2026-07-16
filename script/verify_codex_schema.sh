#!/usr/bin/env bash
set -euo pipefail

EXPECTED_VERSION="${PATCHWRIGHT_CODEX_VERSION:-0.144.2}"
CODEX_BIN="${CODEX_BIN:-$(command -v codex || true)}"
if [[ -z "$CODEX_BIN" || ! -x "$CODEX_BIN" ]]; then
  echo "codex executable not found" >&2
  exit 1
fi

ACTUAL_VERSION="$($CODEX_BIN --version)"
if [[ "$ACTUAL_VERSION" != "codex-cli $EXPECTED_VERSION" ]]; then
  echo "expected codex-cli $EXPECTED_VERSION, found $ACTUAL_VERSION" >&2
  exit 1
fi

SCHEMA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-codex-schema.XXXXXX")"
trap 'rm -rf "$SCHEMA_DIR"' EXIT
"$CODEX_BIN" app-server generate-json-schema --experimental --out "$SCHEMA_DIR"

SCHEMA="$SCHEMA_DIR/codex_app_server_protocol.schemas.json"
for required_file in \
  "$SCHEMA" \
  "$SCHEMA_DIR/v1/InitializeParams.json" \
  "$SCHEMA_DIR/v1/InitializeResponse.json" \
  "$SCHEMA_DIR/v2/GetAccountResponse.json" \
  "$SCHEMA_DIR/v2/ThreadStartParams.json" \
  "$SCHEMA_DIR/v2/ThreadResumeParams.json" \
  "$SCHEMA_DIR/v2/TurnStartParams.json" \
  "$SCHEMA_DIR/v2/TurnSteerParams.json" \
  "$SCHEMA_DIR/v2/TurnInterruptParams.json"; do
  test -s "$required_file"
done

for method in \
  initialize initialized account/read \
  thread/start thread/resume \
  turn/start turn/steer turn/interrupt \
  item/started item/completed turn/completed error \
  item/commandExecution/requestApproval \
  item/fileChange/requestApproval \
  item/permissions/requestApproval; do
  if ! grep -Fq "\"$method\"" "$SCHEMA"; then
    echo "required Codex app-server method missing: $method" >&2
    exit 1
  fi
done

jq -e '.definitions.TurnStatus.enum == ["completed", "interrupted", "failed", "inProgress"]' \
  "$SCHEMA_DIR/v2/TurnCompletedNotification.json" >/dev/null

echo "Codex app-server schema verified for $ACTUAL_VERSION"
