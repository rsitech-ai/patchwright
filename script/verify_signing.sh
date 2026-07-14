#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?app path required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
"$ROOT_DIR/script/validate_bundle.sh" "$APP_PATH" --require-signed

MAIN_DETAILS="$(/usr/bin/codesign -dvvv "$APP_PATH" 2>&1)"
AUTHORITY="$(printf '%s\n' "$MAIN_DETAILS" | sed -n 's/^Authority=//p' | head -n 1)"
TEAM="$(printf '%s\n' "$MAIN_DETAILS" | sed -n 's/^TeamIdentifier=//p' | head -n 1)"
[[ "$AUTHORITY" == Developer\ ID\ Application:* ]] || { echo "signing verification failed: wrong identity class" >&2; exit 65; }
[[ -n "$TEAM" && "$TEAM" != not\ set ]] || { echo "signing verification failed: Team ID missing" >&2; exit 65; }
printf '%s\n' "$MAIN_DETAILS" | grep -Eq '^Timestamp=' || { echo "signing verification failed: secure timestamp missing" >&2; exit 65; }
printf '%s\n' "$MAIN_DETAILS" | grep -Eq 'flags=.*runtime' || { echo "signing verification failed: Hardened Runtime missing" >&2; exit 65; }

for nested in "$APP_PATH/Contents/Helpers/patchwright-engine" "$APP_PATH/Contents/Helpers/patchwright-relay"; do
  DETAILS="$(/usr/bin/codesign -dvvv "$nested" 2>&1)"
  printf '%s\n' "$DETAILS" | grep -Fq "TeamIdentifier=$TEAM" || { echo "signing verification failed: nested Team ID mismatch" >&2; exit 65; }
  printf '%s\n' "$DETAILS" | grep -Eq 'flags=.*runtime' || { echo "signing verification failed: nested runtime missing" >&2; exit 65; }
  printf '%s\n' "$DETAILS" | grep -Eq '^Timestamp=' || { echo "signing verification failed: nested timestamp missing" >&2; exit 65; }
done

if /usr/bin/codesign -d --entitlements :- "$APP_PATH" 2>/dev/null | plutil -convert json -o - - | jq -e 'keys | length > 0' >/dev/null; then
  echo "signing verification failed: unreviewed app entitlements" >&2
  exit 65
fi
/usr/sbin/spctl --assess --type execute --verbose=4 "$APP_PATH" 2>&1 || true
echo "Developer ID signature verified: team=$TEAM"
