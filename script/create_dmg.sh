#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?signed app path required}"
OUTPUT_PATH="${2:?output dmg path required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
"$ROOT_DIR/script/verify_signing.sh" "$APP_PATH"
IDENTITY="$(/usr/bin/codesign -dvvv "$APP_PATH" 2>&1 | sed -n 's/^Authority=\(Developer ID Application:.*\)$/\1/p' | head -n 1)"
[[ -n "$IDENTITY" ]] || { echo "Developer ID identity unavailable from app" >&2; exit 65; }
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_PATH/Contents/Info.plist")
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-dmg.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/payload"
/usr/bin/ditto "$APP_PATH" "$STAGE/payload/Patchwright.app"
ln -s /Applications "$STAGE/payload/Applications"
mkdir -p "$(dirname "$OUTPUT_PATH")"
TEMP_DMG="$STAGE/Patchwright-rw.dmg"
/usr/bin/hdiutil create -quiet -fs HFS+ -volname "Patchwright $VERSION" -srcfolder "$STAGE/payload" "$TEMP_DMG"
/usr/bin/hdiutil convert -quiet "$TEMP_DMG" -format UDZO -o "$OUTPUT_PATH"
/usr/bin/codesign --force --sign "$IDENTITY" --timestamp "$OUTPUT_PATH"
/usr/bin/codesign --verify --verbose=2 "$OUTPUT_PATH"
echo "signed DMG created: $OUTPUT_PATH"
