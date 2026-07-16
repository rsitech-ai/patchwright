#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:?signed app or DMG required}"
PUBLIC_RESULT="${2:?public evidence path required}"
PRIVATE_DIR="${3:?private evidence directory required}"
KIND="${4:?kind app or dmg required}"
[[ "$KIND" == app || "$KIND" == dmg ]] || { echo "notary kind must be app or dmg" >&2; exit 64; }
[[ ! -L "$TARGET" && ( -d "$TARGET" || -f "$TARGET" ) ]] || { echo "notary target must be a real app or regular DMG" >&2; exit 65; }
[[ ! -e "$PUBLIC_RESULT" && ! -L "$PUBLIC_RESULT" ]] || { echo "public notary evidence path must be new" >&2; exit 65; }
PROFILE="${PATCHWRIGHT_NOTARY_PROFILE:-}"
[[ -n "$PROFILE" ]] || { echo "blocked:external — PATCHWRIGHT_NOTARY_PROFILE must name a Keychain notarytool profile" >&2; exit 78; }
mkdir -p "$PRIVATE_DIR" "$(dirname "$PUBLIC_RESULT")"
TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-notary.XXXXXX")"
trap '/usr/bin/trash "$TEMP_ROOT" >/dev/null 2>&1 || true' EXIT
SUBMIT_TARGET="$TARGET"
if [[ "$KIND" == app ]]; then
  SUBMIT_TARGET="$TEMP_ROOT/Patchwright.zip"
  /usr/bin/ditto -c -k --keepParent "$TARGET" "$SUBMIT_TARGET"
fi
SUBMISSION_SHA256="$(shasum -a 256 "$SUBMIT_TARGET" | awk '{print $1}')"
RAW_RESULT="$PRIVATE_DIR/notary-$KIND.json"
xcrun notarytool submit "$SUBMIT_TARGET" --keychain-profile "$PROFILE" --wait --output-format json >"$RAW_RESULT"
STATUS="$(jq -r '.status // empty' "$RAW_RESULT")"
ID="$(jq -r '.id // empty' "$RAW_RESULT")"
[[ "$STATUS" == Accepted && -n "$ID" ]] || { echo "notarization failed: status=${STATUS:-unknown}; private evidence retained" >&2; exit 65; }
xcrun notarytool log "$ID" --keychain-profile "$PROFILE" "$PRIVATE_DIR/notary-$ID.log.json" >/dev/null
xcrun stapler staple "$TARGET"
xcrun stapler validate "$TARGET"
FINAL_SHA256=""; [[ -f "$TARGET" ]] && FINAL_SHA256="$(shasum -a 256 "$TARGET" | awk '{print $1}')"
TEMP_PUBLIC="$PUBLIC_RESULT.tmp"
jq -n --arg kind "$KIND" --arg submission_sha256 "$SUBMISSION_SHA256" --arg final_sha256 "$FINAL_SHA256" \
  --arg status "$STATUS" --arg request_id "$ID" --arg completed_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '{schema_version:1,kind:$kind,submission_sha256:$submission_sha256,final_sha256:$final_sha256,status:$status,request_id:$request_id,stapled:true,stapler_validated:true,completed_at:$completed_at}' >"$TEMP_PUBLIC"
mv "$TEMP_PUBLIC" "$PUBLIC_RESULT"
echo "notarized and stapled: $TARGET"
