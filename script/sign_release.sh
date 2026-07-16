#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?app path required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

resolve_identity() {
  local requested=""
  local identities
  if [[ "${PATCHWRIGHT_DEVELOPER_ID+x}" == x ]]; then
    requested="$PATCHWRIGHT_DEVELOPER_ID"
    [[ -n "$requested" ]] || return 1
  fi
  identities="$(security find-identity -p codesigning -v 2>/dev/null | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p')"
  if [[ "${PATCHWRIGHT_DEVELOPER_ID+x}" == x ]]; then
    [[ "$requested" == Developer\ ID\ Application:* ]] || return 1
    [[ "$(printf '%s\n' "$identities" | grep -Fxc "$requested")" == 1 ]] || return 1
    printf '%s' "$requested"
    return 0
  fi
  [[ "$(printf '%s\n' "$identities" | sed '/^$/d' | wc -l | tr -d ' ')" == 1 ]] || return 1
  printf '%s' "$identities"
}

IDENTITY="$(resolve_identity || true)"
if [[ -z "$IDENTITY" ]]; then
  echo "blocked:external — exactly one Developer ID Application identity is required" >&2
  exit 78
fi
"$ROOT_DIR/script/validate_bundle.sh" "$APP_PATH"
/usr/bin/xattr -cr "$APP_PATH"
SPARKLE="$APP_PATH/Contents/Frameworks/Sparkle.framework"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  "$SPARKLE/Versions/B/XPCServices/Installer.xpc"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --preserve-metadata=entitlements \
  "$SPARKLE/Versions/B/XPCServices/Downloader.xpc"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  "$SPARKLE/Versions/B/Autoupdate"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  "$SPARKLE/Versions/B/Updater.app"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  "$SPARKLE"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --entitlements "$ROOT_DIR/Packaging/patchwright-engine.entitlements" \
  "$APP_PATH/Contents/Helpers/patchwright-engine"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --entitlements "$ROOT_DIR/Packaging/patchwright-relay.entitlements" \
  "$APP_PATH/Contents/Helpers/patchwright-relay"
/usr/bin/codesign --force --sign "$IDENTITY" --options runtime --timestamp \
  --entitlements "$ROOT_DIR/Packaging/Patchwright.entitlements" \
  "$APP_PATH"
"$ROOT_DIR/script/verify_signing.sh" "$APP_PATH"
echo "Developer ID signed: $APP_PATH"
