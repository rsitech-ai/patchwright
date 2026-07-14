#!/usr/bin/env bash
set -euo pipefail

DMG_PATH="${1:?notarized DMG required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ -f "$DMG_PATH" && "$DMG_PATH" == *.dmg ]] || { echo "distribution verification failed: invalid DMG path" >&2; exit 65; }
/usr/bin/codesign --verify --verbose=2 "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"
/usr/sbin/spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG_PATH"
MOUNT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-mount.XXXXXX")"
trap '/usr/bin/hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true; rmdir "$MOUNT" 2>/dev/null || true' EXIT
/usr/bin/hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$DMG_PATH"
[[ -d "$MOUNT/Patchwright.app" ]] || { echo "distribution verification failed: app missing from DMG" >&2; exit 65; }
[[ -L "$MOUNT/Applications" && "$(readlink "$MOUNT/Applications")" == /Applications ]] || { echo "distribution verification failed: Applications alias missing" >&2; exit 65; }
"$ROOT_DIR/script/validate_bundle.sh" "$MOUNT/Patchwright.app" --require-signed
"$ROOT_DIR/script/verify_signing.sh" "$MOUNT/Patchwright.app"
/usr/sbin/spctl --assess --type execute --verbose=4 "$MOUNT/Patchwright.app"
if /usr/bin/xattr -lr "$MOUNT/Patchwright.app" 2>/dev/null | grep -Eq 'com\.apple\.quarantine|com\.apple\.fileprovider'; then
  echo "distribution verification failed: forbidden xattr" >&2
  exit 65
fi
shasum -a 256 "$DMG_PATH" >"$DMG_PATH.sha256"
echo "notarized distribution verified: $DMG_PATH"
