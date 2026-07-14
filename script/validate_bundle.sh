#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:-}"
MODE="${2:-}"
EXPECTED_BUNDLE_ID="${PATCHWRIGHT_BUNDLE_ID:-ai.patchwright.app}"
EXPECTED_MINIMUM="${PATCHWRIGHT_MINIMUM_SYSTEM:-26.0}"

die() {
  echo "bundle validation failed: $*" >&2
  exit 65
}

[[ -n "$APP_PATH" && -d "$APP_PATH" && "$APP_PATH" == *.app ]] || die "expected an .app bundle path"
[[ ! -L "$APP_PATH" ]] || die "bundle root must not be a symlink"
PLIST="$APP_PATH/Contents/Info.plist"
MAIN="$APP_PATH/Contents/MacOS/Patchwright"
ENGINE="$APP_PATH/Contents/Helpers/patchwright-engine"
RELAY="$APP_PATH/Contents/Helpers/patchwright-relay"
for required in "$PLIST" "$MAIN" "$ENGINE" "$RELAY"; do
  [[ -f "$required" ]] || die "missing ${required#"$APP_PATH/"}"
done
for executable in "$MAIN" "$ENGINE" "$RELAY"; do
  [[ -x "$executable" ]] || die "not executable: ${executable#"$APP_PATH/"}"
  /usr/bin/lipo -archs "$executable" 2>/dev/null | tr ' ' '\n' | grep -Eq '^arm64(e)?$' \
    || die "arm64 architecture missing: ${executable#"$APP_PATH/"}"
done

plist_value() { /usr/libexec/PlistBuddy -c "Print :$1" "$PLIST" 2>/dev/null || true; }
[[ "$(plist_value CFBundleIdentifier)" == "$EXPECTED_BUNDLE_ID" ]] || die "bundle identifier mismatch"
[[ "$(plist_value CFBundleExecutable)" == "Patchwright" ]] || die "bundle executable mismatch"
[[ "$(plist_value CFBundlePackageType)" == "APPL" ]] || die "bundle package type mismatch"
[[ "$(plist_value LSMinimumSystemVersion)" == "$EXPECTED_MINIMUM" ]] || die "minimum system mismatch"
[[ "$(plist_value CFBundleShortVersionString)" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9]+)*$ ]] || die "invalid marketing version"
[[ "$(plist_value CFBundleVersion)" =~ ^[1-9][0-9]*$ ]] || die "invalid build number"

if [[ -n "$(find "$APP_PATH" -type l -print -quit)" ]]; then
  die "symlinks are not allowed inside the app bundle"
fi
if [[ -n "$(find "$APP_PATH" -perm -002 -print -quit)" ]]; then
  die "world-writable bundle content"
fi
if /usr/bin/xattr -lr "$APP_PATH" 2>/dev/null | grep -Eq 'com\.apple\.(quarantine|FinderInfo)|com\.apple\.fileprovider'; then
  die "forbidden extended attributes"
fi

if [[ "$MODE" == "--require-signed" ]]; then
  /usr/bin/codesign --verify --strict --verbose=2 "$ENGINE" || die "engine signature invalid"
  /usr/bin/codesign --verify --strict --verbose=2 "$RELAY" || die "relay signature invalid"
  /usr/bin/codesign --verify --deep --strict --verbose=2 "$APP_PATH" || die "app signature invalid"
elif [[ -n "$MODE" ]]; then
  die "unknown mode: $MODE"
fi

echo "bundle structure verified: $APP_PATH"
