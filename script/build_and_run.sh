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
if [[ -e "$APP_BUNDLE" || -L "$APP_BUNDLE" ]]; then
  /usr/bin/trash "$APP_BUNDLE"
fi
mkdir -p "$STAGING_ROOT" "$ROOT_DIR/dist" "$APP_MACOS"
mkdir -p "$APP_BUNDLE/Contents/Helpers" "$APP_BUNDLE/Contents/Frameworks" "$APP_BUNDLE/Contents/Resources"
SWIFT_BIN_DIR="$(swift build -c release --show-bin-path)"
SPARKLE_FRAMEWORK="$SWIFT_BIN_DIR/Sparkle.framework"
[[ -d "$SPARKLE_FRAMEWORK" && ! -L "$SPARKLE_FRAMEWORK" ]] \
  || { echo "Sparkle.framework missing from the resolved release build" >&2; exit 65; }
cp "$SWIFT_BIN_DIR/$APP_NAME" "$APP_MACOS/$APP_NAME"
/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"
cp "$ROOT_DIR/target/release/patchwright-engine" "$APP_BUNDLE/Contents/Helpers/patchwright-engine"
cp "$ROOT_DIR/target/release/patchwright-relay" "$APP_BUNDLE/Contents/Helpers/patchwright-relay"
chmod +x "$APP_MACOS/$APP_NAME"
chmod +x "$APP_BUNDLE/Contents/Helpers/patchwright-engine" "$APP_BUNDLE/Contents/Helpers/patchwright-relay"
cp "$ROOT_DIR/Packaging/Info.plist" "$APP_BUNDLE/Contents/Info.plist"
cp "$ROOT_DIR/Packaging/Patchwright.icns" "$APP_BUNDLE/Contents/Resources/Patchwright.icns"
clean_bundle_metadata() {
  /usr/bin/xattr -cr "$APP_BUNDLE"
  # File Provider can retain these root attributes even after `xattr -cr`.
  # Either attribute makes codesign reject an otherwise valid staged bundle.
  /usr/bin/xattr -d com.apple.FinderInfo "$APP_BUNDLE" 2>/dev/null || true
  /usr/bin/xattr -d 'com.apple.fileprovider.fpfs#P' "$APP_BUNDLE" 2>/dev/null || true
}
clean_bundle_metadata
"$ROOT_DIR/script/validate_bundle.sh" "$APP_BUNDLE"
SPARKLE="$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"
/usr/bin/codesign --force --sign - "$SPARKLE/Versions/B/XPCServices/Installer.xpc"
/usr/bin/codesign --force --sign - --preserve-metadata=entitlements \
  "$SPARKLE/Versions/B/XPCServices/Downloader.xpc"
/usr/bin/codesign --force --sign - "$SPARKLE/Versions/B/Autoupdate"
/usr/bin/codesign --force --sign - "$SPARKLE/Versions/B/Updater.app"
/usr/bin/codesign --force --sign - "$SPARKLE"
/usr/bin/codesign --force --sign - "$APP_BUNDLE/Contents/Helpers/patchwright-engine"
/usr/bin/codesign --force --sign - "$APP_BUNDLE/Contents/Helpers/patchwright-relay"
/usr/bin/codesign --force --sign - "$APP_BUNDLE"
/usr/bin/codesign --verify --deep --strict "$APP_BUNDLE"
if [[ -e "$DIST_APP_BUNDLE" && ! -L "$DIST_APP_BUNDLE" ]]; then
  /usr/bin/trash "$DIST_APP_BUNDLE"
fi
ln -sfn "$APP_BUNDLE" "$DIST_APP_BUNDLE"
verify_bundle() {
  /usr/bin/codesign --verify --deep --strict "$APP_BUNDLE"
}
verify_engine_health() {
  local socket="$HOME/.patchwright/engine.sock"
  local response=""
  for _ in {1..100}; do
    if [[ -S "$socket" ]]; then
      response="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}' | nc -w 1 -U "$socket" 2>/dev/null || true)"
      [[ "$response" == *'"status":"ok"'* ]] && return 0
    fi
    sleep 0.05
  done
  echo "engine RPC health check failed" >&2
  return 1
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
    verify_engine_health
    sleep 1
    pgrep -f "^$ENGINE_HELPER serve --socket " >/dev/null
    verify_bundle
    ;;
  *) echo "usage: $0 [run|--debug|--logs|--telemetry|--verify]" >&2; exit 2 ;;
esac
