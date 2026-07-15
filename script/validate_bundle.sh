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
SPARKLE="$APP_PATH/Contents/Frameworks/Sparkle.framework"
SPARKLE_BINARY="$SPARKLE/Versions/B/Sparkle"
SPARKLE_AUTOUPDATE="$SPARKLE/Versions/B/Autoupdate"
SPARKLE_UPDATER="$SPARKLE/Versions/B/Updater.app"
SPARKLE_UPDATER_BINARY="$SPARKLE_UPDATER/Contents/MacOS/Updater"
SPARKLE_DOWNLOADER="$SPARKLE/Versions/B/XPCServices/Downloader.xpc"
SPARKLE_DOWNLOADER_BINARY="$SPARKLE_DOWNLOADER/Contents/MacOS/Downloader"
SPARKLE_INSTALLER="$SPARKLE/Versions/B/XPCServices/Installer.xpc"
SPARKLE_INSTALLER_BINARY="$SPARKLE_INSTALLER/Contents/MacOS/Installer"
[[ -d "$SPARKLE" && ! -L "$SPARKLE" ]] || die "missing Contents/Frameworks/Sparkle.framework"
[[ -d "$SPARKLE_UPDATER" && ! -L "$SPARKLE_UPDATER" ]] || die "missing Sparkle Updater.app"
[[ -d "$SPARKLE_DOWNLOADER" && ! -L "$SPARKLE_DOWNLOADER" ]] || die "missing Sparkle Downloader.xpc"
[[ -d "$SPARKLE_INSTALLER" && ! -L "$SPARKLE_INSTALLER" ]] || die "missing Sparkle Installer.xpc"
for required in "$PLIST" "$MAIN" "$ENGINE" "$RELAY" "$SPARKLE_BINARY" "$SPARKLE_AUTOUPDATE" \
  "$SPARKLE_UPDATER_BINARY" "$SPARKLE_DOWNLOADER_BINARY" "$SPARKLE_INSTALLER_BINARY"; do
  [[ -f "$required" ]] || die "missing ${required#"$APP_PATH/"}"
done
for executable in "$MAIN" "$ENGINE" "$RELAY" "$SPARKLE_BINARY" "$SPARKLE_AUTOUPDATE" \
  "$SPARKLE_UPDATER_BINARY" "$SPARKLE_DOWNLOADER_BINARY" "$SPARKLE_INSTALLER_BINARY"; do
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

EXPECTED_SPARKLE_LINKS="$(cat <<'EOF'
Autoupdate|Versions/Current/Autoupdate
Headers|Versions/Current/Headers
Modules|Versions/Current/Modules
PrivateHeaders|Versions/Current/PrivateHeaders
Resources|Versions/Current/Resources
Sparkle|Versions/Current/Sparkle
Updater.app|Versions/Current/Updater.app
Versions/Current|B
XPCServices|Versions/Current/XPCServices
EOF
)"
while IFS='|' read -r relative expected_target; do
  link="$SPARKLE/$relative"
  [[ -L "$link" ]] || die "missing canonical Sparkle symlink: $relative"
  actual_target="$(readlink "$link")"
  if [[ "$actual_target" == /* || "/$actual_target/" == */../* ]]; then
    die "invalid Sparkle symlink target: $relative"
  fi
  [[ "$actual_target" == "$expected_target" ]] || die "Sparkle symlink target mismatch: $relative"
  [[ -e "$link" ]] || die "dangling Sparkle symlink: $relative"
done <<<"$EXPECTED_SPARKLE_LINKS"

while IFS= read -r link; do
  relative="${link#"$SPARKLE/"}"
  if ! printf '%s\n' "$EXPECTED_SPARKLE_LINKS" | cut -d '|' -f 1 | grep -Fqx "$relative"; then
    die "unexpected Sparkle symlink: $relative"
  fi
done < <(find "$SPARKLE" -type l -print | LC_ALL=C sort)
if [[ -n "$(find "$APP_PATH" -type l ! -path "$SPARKLE/*" -print -quit)" ]]; then
  die "symlinks outside Sparkle.framework are not allowed"
fi

validate_sparkle_plist() {
  local plist="$1"
  local identifier="$2"
  [[ -f "$plist" && ! -L "$plist" ]] || die "missing Sparkle metadata: ${plist#"$APP_PATH/"}"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist" 2>/dev/null || true)" == 2.9.2 ]] \
    || die "Sparkle version mismatch: ${plist#"$APP_PATH/"}"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$plist" 2>/dev/null || true)" == 2057 ]] \
    || die "Sparkle build mismatch: ${plist#"$APP_PATH/"}"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist" 2>/dev/null || true)" == "$identifier" ]] \
    || die "Sparkle bundle identifier mismatch: ${plist#"$APP_PATH/"}"
}
validate_sparkle_plist "$SPARKLE/Versions/B/Resources/Info.plist" org.sparkle-project.Sparkle
validate_sparkle_plist "$SPARKLE_UPDATER/Contents/Info.plist" org.sparkle-project.Sparkle.Updater
validate_sparkle_plist "$SPARKLE_DOWNLOADER/Contents/Info.plist" org.sparkle-project.DownloaderService
validate_sparkle_plist "$SPARKLE_INSTALLER/Contents/Info.plist" org.sparkle-project.InstallerLauncher

/usr/bin/otool -L "$MAIN" | grep -Fq '@rpath/Sparkle.framework/Versions/B/Sparkle' \
  || die "Patchwright executable is missing the Sparkle install-name reference"
/usr/bin/otool -l "$MAIN" | awk '
  $1 == "cmd" && $2 == "LC_RPATH" { in_rpath = 1; next }
  in_rpath && $1 == "path" { print $2; in_rpath = 0 }
' | grep -Fqx '@executable_path/../Frameworks' \
  || die "Patchwright executable is missing the app Frameworks rpath"
/usr/bin/otool -D "$SPARKLE_BINARY" | grep -Fqx '@rpath/Sparkle.framework/Versions/B/Sparkle' \
  || die "Sparkle framework install name mismatch"
if [[ -n "$(find "$APP_PATH" -perm -002 -print -quit)" ]]; then
  die "world-writable bundle content"
fi
if /usr/bin/xattr -lr "$APP_PATH" 2>/dev/null | grep -Eq 'com\.apple\.(quarantine|FinderInfo)|com\.apple\.fileprovider'; then
  die "forbidden extended attributes"
fi

if [[ "$MODE" == "--require-signed" ]]; then
  for signed_object in "$SPARKLE_INSTALLER" "$SPARKLE_DOWNLOADER" "$SPARKLE_AUTOUPDATE" \
    "$SPARKLE_UPDATER" "$SPARKLE" "$ENGINE" "$RELAY" "$APP_PATH"; do
    /usr/bin/codesign --verify --strict --verbose=2 "$signed_object" \
      || die "signature invalid: ${signed_object#"$APP_PATH/"}"
  done
  /usr/bin/codesign --verify --deep --strict --verbose=2 "$APP_PATH" || die "app signature invalid"
elif [[ -n "$MODE" ]]; then
  die "unknown mode: $MODE"
fi

echo "bundle structure verified: $APP_PATH"
