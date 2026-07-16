#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?app path required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# This prerequisite also discovers every Sparkle bundle, dylib, and Mach-O and
# rejects anything outside the pinned five-object signing model before any
# per-object or aggregate signature verification runs.
"$ROOT_DIR/script/validate_bundle.sh" "$APP_PATH" --require-signed

MAIN_DETAILS="$(/usr/bin/codesign -dvvv "$APP_PATH" 2>&1)"
AUTHORITY="$(printf '%s\n' "$MAIN_DETAILS" | sed -n 's/^Authority=//p' | head -n 1)"
TEAM="$(printf '%s\n' "$MAIN_DETAILS" | sed -n 's/^TeamIdentifier=//p' | head -n 1)"
case "$AUTHORITY" in
  "Developer ID Application:"*) ;;
  *) echo "signing verification failed: wrong identity class" >&2; exit 65 ;;
esac
[[ -n "$TEAM" && "$TEAM" != not\ set ]] || { echo "signing verification failed: Team ID missing" >&2; exit 65; }
printf '%s\n' "$MAIN_DETAILS" | grep -Eq '^Timestamp=' || { echo "signing verification failed: secure timestamp missing" >&2; exit 65; }
printf '%s\n' "$MAIN_DETAILS" | grep -Eq 'flags=.*runtime' || { echo "signing verification failed: Hardened Runtime missing" >&2; exit 65; }

SPARKLE="$APP_PATH/Contents/Frameworks/Sparkle.framework"
while IFS= read -r nested; do
  DETAILS="$(/usr/bin/codesign -dvvv "$nested" 2>&1)"
  printf '%s\n' "$DETAILS" | grep -Fq "Authority=$AUTHORITY" || { echo "signing verification failed: nested Developer ID authority mismatch" >&2; exit 65; }
  printf '%s\n' "$DETAILS" | grep -Fq "TeamIdentifier=$TEAM" || { echo "signing verification failed: nested Team ID mismatch" >&2; exit 65; }
  printf '%s\n' "$DETAILS" | grep -Eq 'flags=.*runtime' || { echo "signing verification failed: nested runtime missing" >&2; exit 65; }
  printf '%s\n' "$DETAILS" | grep -Eq '^Timestamp=' || { echo "signing verification failed: nested timestamp missing" >&2; exit 65; }
  ENTITLEMENTS="$(/usr/bin/codesign -d --entitlements :- "$nested" 2>/dev/null || true)"
  if [[ -n "$ENTITLEMENTS" ]] && ! printf '%s' "$ENTITLEMENTS" \
    | plutil -convert json -o - - 2>/dev/null \
    | jq -e 'type == "object" and (keys | length == 0)' >/dev/null; then
    echo "signing verification failed: unreviewed entitlements on ${nested#"$APP_PATH/"}" >&2
    exit 65
  fi
done <<EOF
$SPARKLE/Versions/B/XPCServices/Installer.xpc
$SPARKLE/Versions/B/XPCServices/Downloader.xpc
$SPARKLE/Versions/B/Autoupdate
$SPARKLE/Versions/B/Updater.app
$SPARKLE
$APP_PATH/Contents/Helpers/patchwright-engine
$APP_PATH/Contents/Helpers/patchwright-relay
$APP_PATH
EOF

/usr/bin/codesign --verify --deep --strict --verbose=2 "$APP_PATH" \
  || { echo "signing verification failed: aggregate bundle verification failed" >&2; exit 65; }
/usr/sbin/spctl --assess --type execute --verbose=4 "$APP_PATH" 2>&1 || true
echo "Developer ID signature verified: team=$TEAM"
