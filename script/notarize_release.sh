#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:?signed app or DMG required}"
EVIDENCE_DIR="${2:?evidence directory required}"
PROFILE="${PATCHWRIGHT_NOTARY_PROFILE:-}"
[[ -n "$PROFILE" ]] || { echo "blocked:external — PATCHWRIGHT_NOTARY_PROFILE must name a Keychain notarytool profile" >&2; exit 78; }
mkdir -p "$EVIDENCE_DIR"
SUBMIT_TARGET="$TARGET"
TEMP_ROOT=""
if [[ "$TARGET" == *.app ]]; then
  TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-notary.XXXXXX")"
  SUBMIT_TARGET="$TEMP_ROOT/Patchwright.zip"
  /usr/bin/ditto -c -k --keepParent "$TARGET" "$SUBMIT_TARGET"
fi
trap '[[ -z "$TEMP_ROOT" ]] || rm -rf "$TEMP_ROOT"' EXIT

RESULT="$EVIDENCE_DIR/notary-$(basename "$TARGET").json"
xcrun notarytool submit "$SUBMIT_TARGET" --keychain-profile "$PROFILE" --wait --output-format json >"$RESULT"
STATUS="$(jq -r '.status // empty' "$RESULT")"
ID="$(jq -r '.id // empty' "$RESULT")"
[[ "$STATUS" == Accepted && -n "$ID" ]] || {
  [[ -z "$ID" ]] || xcrun notarytool log "$ID" --keychain-profile "$PROFILE" "$EVIDENCE_DIR/notary-$ID.log.json" >/dev/null 2>&1 || true
  echo "notarization failed: status=${STATUS:-unknown}; evidence=$RESULT" >&2
  exit 65
}
xcrun notarytool log "$ID" --keychain-profile "$PROFILE" "$EVIDENCE_DIR/notary-$ID.log.json" >/dev/null
xcrun stapler staple "$TARGET"
xcrun stapler validate "$TARGET"
echo "notarized and stapled: $TARGET"
