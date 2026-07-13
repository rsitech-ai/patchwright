#!/usr/bin/env bash
set -euo pipefail
MODE="${1:-run}"
APP_NAME="Patchwright"
BUNDLE_ID="ai.patchwright.app"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGING_ROOT="${PATCHWRIGHT_STAGING_ROOT:-$HOME/.patchwright/staged}"
APP_BUNDLE="$STAGING_ROOT/$APP_NAME.app"
DIST_APP_BUNDLE="$ROOT_DIR/dist/$APP_NAME.app"
APP_MACOS="$APP_BUNDLE/Contents/MacOS"
ENGINE_HELPER="$APP_BUNDLE/Contents/Helpers/patchwright-engine"
LEGACY_ENGINE_HELPER="$DIST_APP_BUNDLE/Contents/Helpers/patchwright-engine"
pkill -x "$APP_NAME" >/dev/null 2>&1 || true
pkill -f "^$ENGINE_HELPER serve --socket " >/dev/null 2>&1 || true
pkill -f "^$LEGACY_ENGINE_HELPER serve --socket " >/dev/null 2>&1 || true
for _ in {1..50}; do
  if ! pgrep -x "$APP_NAME" >/dev/null && ! pgrep -f "^$ENGINE_HELPER serve --socket " >/dev/null; then break; fi
  sleep 0.05
done
cd "$ROOT_DIR"
swift build -c release
cargo build --release -p patchwright-engine -p patchwright-relay
mkdir -p "$STAGING_ROOT" "$ROOT_DIR/dist" "$APP_MACOS"
mkdir -p "$APP_BUNDLE/Contents/Helpers"
cp "$(swift build -c release --show-bin-path)/$APP_NAME" "$APP_MACOS/$APP_NAME"
cp "$ROOT_DIR/target/release/patchwright-engine" "$APP_BUNDLE/Contents/Helpers/patchwright-engine"
cp "$ROOT_DIR/target/release/patchwright-relay" "$APP_BUNDLE/Contents/Helpers/patchwright-relay"
chmod +x "$APP_MACOS/$APP_NAME"
chmod +x "$APP_BUNDLE/Contents/Helpers/patchwright-engine" "$APP_BUNDLE/Contents/Helpers/patchwright-relay"
/usr/libexec/PlistBuddy -c "Clear dict" "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $APP_NAME" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string $BUNDLE_ID" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundleName string $APP_NAME" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string APPL" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :LSMinimumSystemVersion string 26.0" "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :NSPrincipalClass string NSApplication" "$APP_BUNDLE/Contents/Info.plist"
clean_bundle_metadata() {
  /usr/bin/xattr -cr "$APP_BUNDLE"
  # File Provider can retain these root attributes even after `xattr -cr`.
  # Either attribute makes codesign reject an otherwise valid staged bundle.
  /usr/bin/xattr -d com.apple.FinderInfo "$APP_BUNDLE" 2>/dev/null || true
  /usr/bin/xattr -d 'com.apple.fileprovider.fpfs#P' "$APP_BUNDLE" 2>/dev/null || true
}
clean_bundle_metadata
/usr/bin/codesign --force --deep --sign - "$APP_BUNDLE"
/usr/bin/codesign --verify --deep --strict "$APP_BUNDLE"
if [[ -e "$DIST_APP_BUNDLE" && ! -L "$DIST_APP_BUNDLE" ]]; then
  /usr/bin/trash "$DIST_APP_BUNDLE"
fi
ln -sfn "$APP_BUNDLE" "$DIST_APP_BUNDLE"
verify_bundle() {
  /usr/bin/codesign --verify --deep --strict "$APP_BUNDLE"
}
open_app() { /usr/bin/open -n "$APP_BUNDLE"; verify_bundle; }
case "$MODE" in
  run) open_app ;;
  --debug|debug) lldb -- "$APP_MACOS/$APP_NAME" ;;
  --logs|logs) open_app; /usr/bin/log stream --info --style compact --predicate "process == \"$APP_NAME\"" ;;
  --telemetry|telemetry) open_app; /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\"" ;;
  --verify|verify)
    open_app
    for _ in {1..100}; do pgrep -x "$APP_NAME" >/dev/null && break; sleep 0.05; done
    pgrep -x "$APP_NAME" >/dev/null
    for _ in {1..100}; do pgrep -f "^$ENGINE_HELPER serve --socket " >/dev/null && break; sleep 0.05; done
    pgrep -f "^$ENGINE_HELPER serve --socket " >/dev/null
    verify_bundle
    ;;
  *) echo "usage: $0 [run|--debug|--logs|--telemetry|--verify]" >&2; exit 2 ;;
esac
