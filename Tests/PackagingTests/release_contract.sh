#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-release-contract.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  echo "release contract failed: $*" >&2
  exit 1
}

require_file() {
  local path="$1"
  [[ -f "$ROOT_DIR/$path" ]] || fail "missing $path"
}

require_text() {
  local path="$1"
  local expected="$2"
  grep -Fq -- "$expected" "$ROOT_DIR/$path" || fail "$path is missing required text: $expected"
}

for required in \
  LICENSE-MIT \
  LICENSE-APACHE \
  CONTRIBUTING.md \
  SECURITY.md \
  CODE_OF_CONDUCT.md \
  PRIVACY.md \
  SUPPORT.md; do
  require_file "$required"
done

for required in \
  .github/workflows/ci.yml \
  .github/release.yml \
  rust-toolchain.toml \
  docs/direct-download.md; do
  require_file "$required"
done
if find "$ROOT_DIR/docs/release/0.1.0" -type f -print -quit 2>/dev/null | grep -q .; then
  fail "obsolete App Store 0.1.0 release dossier must be removed"
fi
require_text .github/workflows/ci.yml 'permissions:'
require_text .github/workflows/ci.yml 'contents: read'
require_text .github/workflows/ci.yml 'persist-credentials: false'
require_text .github/workflows/ci.yml './script/verify.sh'
require_text .github/workflows/ci.yml './script/smoke.sh'
require_text rust-toolchain.toml 'channel = "1.91.0"'
require_text README.md 'docs/direct-download.md'
require_text docs/direct-download.md 'Developer ID Application'
require_text docs/direct-download.md 'Apple notarization'
require_text docs/direct-download.md 'GitHub Releases'
require_text docs/release-checklist.md 'notarized-candidate'
require_text docs/release-checklist.md 'promoted-release'
if grep -En 'App Store|App Store Connect|Mac App Store' README.md docs/release-checklist.md docs/release-readiness.md docs/production-plan.md; then
  fail "direct-distribution documentation must not claim an App Store release lane"
fi
[[ -x "$ROOT_DIR/script/generate_app_icon.sh" ]] || fail "script/generate_app_icon.sh must be executable"
require_text Assets/PatchwrightIcon-source.svg 'viewBox="0 0 1024 1024"'

require_text LICENSE-MIT "Permission is hereby granted, free of charge"
require_text LICENSE-APACHE "Apache License"
require_text LICENSE-APACHE "Version 2.0, January 2004"
require_text CONTRIBUTING.md "Developer Certificate of Origin"
require_text CONTRIBUTING.md "Signed-off-by:"
require_text SECURITY.md "security/advisories/new"
require_text CODE_OF_CONDUCT.md "Contributor Covenant"
require_text CODE_OF_CONDUCT.md "version 2.1"
require_text PRIVACY.md "local-first"
require_text SUPPORT.md "best-effort"

for required in \
  Assets/PatchwrightIcon-source.svg \
  Assets/PatchwrightIcon-source.png \
  Assets/README.md \
  Packaging/Patchwright.icns \
  script/generate_app_icon.sh; do
  require_file "$required"
done

SOURCE_ICON="$ROOT_DIR/Assets/PatchwrightIcon-source.png"
[[ "$(/usr/bin/sips -g format "$SOURCE_ICON" 2>/dev/null | awk '/format:/{print $2}')" == png ]] \
  || fail "icon source must be PNG"
[[ "$(/usr/bin/sips -g pixelWidth "$SOURCE_ICON" 2>/dev/null | awk '/pixelWidth:/{print $2}')" == 1024 ]] \
  || fail "icon source width must be 1024"
[[ "$(/usr/bin/sips -g pixelHeight "$SOURCE_ICON" 2>/dev/null | awk '/pixelHeight:/{print $2}')" == 1024 ]] \
  || fail "icon source height must be 1024"
/usr/bin/sips -g profile "$SOURCE_ICON" 2>/dev/null | grep -Fq 'sRGB' \
  || fail "icon source must use an sRGB profile"

ICONSET="$TMP_ROOT/Patchwright.iconset"
/usr/bin/iconutil --convert iconset --output "$ICONSET" "$ROOT_DIR/Packaging/Patchwright.icns" \
  || fail "Patchwright.icns is malformed"
while IFS='|' read -r icon_name icon_size; do
  icon_path="$ICONSET/$icon_name"
  [[ -f "$icon_path" && ! -L "$icon_path" ]] || fail "iconset is missing $icon_name"
  [[ "$(/usr/bin/sips -g pixelWidth "$icon_path" 2>/dev/null | awk '/pixelWidth:/{print $2}')" == "$icon_size" ]] \
    || fail "$icon_name width mismatch"
  [[ "$(/usr/bin/sips -g pixelHeight "$icon_path" 2>/dev/null | awk '/pixelHeight:/{print $2}')" == "$icon_size" ]] \
    || fail "$icon_name height mismatch"
done <<'EOF'
icon_16x16.png|16
icon_16x16@2x.png|32
icon_32x32.png|32
icon_32x32@2x.png|64
icon_128x128.png|128
icon_128x128@2x.png|256
icon_256x256.png|256
icon_256x256@2x.png|512
icon_512x512.png|512
icon_512x512@2x.png|1024
EOF
[[ "$(find "$ICONSET" -type f -name '*.png' | wc -l | tr -d ' ')" == 10 ]] \
  || fail "iconset must contain exactly ten PNG representations"

grep -Eq '^license = "MIT OR Apache-2\.0"$' "$ROOT_DIR/Cargo.toml" \
  || fail 'Cargo.toml must declare license = "MIT OR Apache-2.0"'

BUNDLE_COPYRIGHT="$(/usr/libexec/PlistBuddy -c 'Print :NSHumanReadableCopyright' "$ROOT_DIR/Packaging/Info.plist")"
[[ "$BUNDLE_COPYRIGHT" != *"All rights reserved"* ]] \
  || fail "bundle copyright must not claim All rights reserved"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$ROOT_DIR/Packaging/Info.plist" 2>/dev/null || true)" == Patchwright.icns ]] \
  || fail "CFBundleIconFile must be Patchwright.icns"

SPARKLE_FEED="$(/usr/libexec/PlistBuddy -c 'Print :SUFeedURL' "$ROOT_DIR/Packaging/Info.plist" 2>/dev/null || true)"
[[ "$SPARKLE_FEED" == 'https://github.com/s1korrrr/patchwright/releases/latest/download/appcast.xml' ]] \
  || fail "Sparkle feed must target the latest GitHub release appcast"

for signed_feed_key in SUVerifyUpdateBeforeExtraction SURequireSignedFeed; do
  [[ "$(/usr/libexec/PlistBuddy -c "Print :$signed_feed_key" "$ROOT_DIR/Packaging/Info.plist" 2>/dev/null || true)" == true ]] \
    || fail "$signed_feed_key must be true"
done

SPARKLE_PUBLIC_KEY="$(/usr/libexec/PlistBuddy -c 'Print :SUPublicEDKey' "$ROOT_DIR/Packaging/Info.plist" 2>/dev/null || true)"
[[ -n "$SPARKLE_PUBLIC_KEY" ]] || fail "SUPublicEDKey must be present"
KEY_BYTES="$(printf '%s' "$SPARKLE_PUBLIC_KEY" | /usr/bin/base64 -D 2>/dev/null | /usr/bin/wc -c | /usr/bin/tr -d ' ')"
[[ "$KEY_BYTES" == 32 ]] || fail "SUPublicEDKey must decode to exactly 32 bytes"
[[ "$SPARKLE_PUBLIC_KEY" == 'oMzk7aUjqsQFvrRBZDd5JsXaeTh8B4pQrJ7n6YHRWUA=' ]] \
  || fail "SUPublicEDKey must match the dedicated release Keychain account"

for target in \
  '#build-and-verify' \
  'https://github.com/s1korrrr/patchwright/releases' \
  'LICENSE-MIT' \
  'LICENSE-APACHE' \
  'CONTRIBUTING.md' \
  'SECURITY.md' \
  'PRIVACY.md' \
  'SUPPORT.md' \
  'CODE_OF_CONDUCT.md'; do
  grep -Fq "]($target)" "$ROOT_DIR/README.md" || fail "README.md is missing link to $target"
done

make_fixture() {
  local app="$1"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Frameworks"
  mkdir -p "$app/Contents/Resources"
  cp "$SWIFT_BIN_DIR/Patchwright" "$app/Contents/MacOS/Patchwright"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-engine"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-relay"
  chmod 755 "$app/Contents/MacOS/Patchwright" "$app/Contents/Helpers/patchwright-engine" "$app/Contents/Helpers/patchwright-relay"
  cp "$ROOT_DIR/Packaging/Info.plist" "$app/Contents/Info.plist"
  cp "$ROOT_DIR/Packaging/Patchwright.icns" "$app/Contents/Resources/Patchwright.icns"
  /usr/bin/ditto "$SWIFT_BIN_DIR/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"
}

assert_rejected() {
  local app="$1"
  local expected="$2"
  local output="$3"
  if "$ROOT_DIR/script/validate_bundle.sh" "$app" >"$output" 2>&1; then
    fail "bundle validation accepted: $expected"
  fi
  grep -Fq "$expected" "$output" || fail "bundle rejection was not explicit: $expected"
}

for required in \
  Packaging/Info.plist \
  Packaging/Patchwright.entitlements \
  Packaging/patchwright-engine.entitlements \
  Packaging/patchwright-relay.entitlements \
  script/validate_bundle.sh \
  script/build_release_components.sh \
  script/assert_release_assembly.sh \
  script/sign_release.sh \
  script/verify_signing.sh \
  script/create_dmg.sh \
  script/generate_candidate_evidence.py \
  script/verify_ed25519.swift \
  script/notarize_release.sh \
  script/package_release.sh \
  script/verify_distribution.sh \
  script/release_readiness.sh; do
  [[ -f "$ROOT_DIR/$required" ]] || fail "missing $required"
done
require_text script/package_release.sh 'TEMPORARY_KEYCHAINS=()'
require_text script/package_release.sh 'ORIGINAL_KEYCHAINS+=("$keychain_line")'
require_text script/package_release.sh 'TEMPORARY_KEYCHAINS+=("$keychain_line")'

ASSEMBLY="$TMP_ROOT/assembly.json"
jq -n '{dirty:false,candidate:true}' >"$ASSEMBLY"
"$ROOT_DIR/script/assert_release_assembly.sh" "$ASSEMBLY"
jq -n '{dirty:true,candidate:false}' >"$ASSEMBLY"
if "$ROOT_DIR/script/assert_release_assembly.sh" "$ASSEMBLY" >"$TMP_ROOT/assembly.out" 2>&1; then
  fail "dirty non-candidate assembly was accepted for release"
fi
grep -q 'release assembly is not a clean candidate' "$TMP_ROOT/assembly.out" \
  || fail "dirty assembly rejection was not explicit"

APP="$TMP_ROOT/Patchwright.app"
(cd "$ROOT_DIR" && swift build -c release --product Patchwright)
SWIFT_BIN_DIR="$(cd "$ROOT_DIR" && swift build -c release --show-bin-path)"
make_fixture "$APP"
"$ROOT_DIR/script/validate_bundle.sh" "$APP"

MISSING_ICON="$TMP_ROOT/missing-icon.app"
make_fixture "$MISSING_ICON"
rm "$MISSING_ICON/Contents/Resources/Patchwright.icns"
assert_rejected "$MISSING_ICON" "missing Contents/Resources/Patchwright.icns" "$TMP_ROOT/missing-icon.out"

SYMLINKED_ICON="$TMP_ROOT/symlinked-icon.app"
make_fixture "$SYMLINKED_ICON"
rm "$SYMLINKED_ICON/Contents/Resources/Patchwright.icns"
ln -s "$ROOT_DIR/Packaging/Patchwright.icns" "$SYMLINKED_ICON/Contents/Resources/Patchwright.icns"
assert_rejected "$SYMLINKED_ICON" "icon resource must be a regular non-symlink file" "$TMP_ROOT/symlinked-icon.out"

MALFORMED_ICON="$TMP_ROOT/malformed-icon.app"
make_fixture "$MALFORMED_ICON"
printf 'not an icon\n' >"$MALFORMED_ICON/Contents/Resources/Patchwright.icns"
assert_rejected "$MALFORMED_ICON" "malformed Contents/Resources/Patchwright.icns" "$TMP_ROOT/malformed-icon.out"

MISMATCHED_ICON="$TMP_ROOT/mismatched-icon.app"
make_fixture "$MISMATCHED_ICON"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIconFile Other.icns' "$MISMATCHED_ICON/Contents/Info.plist"
assert_rejected "$MISMATCHED_ICON" "bundle icon declaration mismatch" "$TMP_ROOT/mismatched-icon.out"

SPARKLE="$APP/Contents/Frameworks/Sparkle.framework"
EXPECTED_SPARKLE_LINKS="$TMP_ROOT/expected-sparkle-links.txt"
cat >"$EXPECTED_SPARKLE_LINKS" <<'EOF'
Autoupdate -> Versions/Current/Autoupdate
Headers -> Versions/Current/Headers
Modules -> Versions/Current/Modules
PrivateHeaders -> Versions/Current/PrivateHeaders
Resources -> Versions/Current/Resources
Sparkle -> Versions/Current/Sparkle
Updater.app -> Versions/Current/Updater.app
Versions/Current -> B
XPCServices -> Versions/Current/XPCServices
EOF
find "$SPARKLE" -type l -print0 \
  | while IFS= read -r -d '' link; do printf '%s -> %s\n' "${link#"$SPARKLE/"}" "$(readlink "$link")"; done \
  | LC_ALL=C sort >"$TMP_ROOT/actual-sparkle-links.txt"
cmp "$EXPECTED_SPARKLE_LINKS" "$TMP_ROOT/actual-sparkle-links.txt" \
  || fail "resolved Sparkle.framework does not have the canonical nine-symlink layout"

MISSING_FRAMEWORK="$TMP_ROOT/missing-framework.app"
make_fixture "$MISSING_FRAMEWORK"
rm -rf "$MISSING_FRAMEWORK/Contents/Frameworks/Sparkle.framework"
assert_rejected "$MISSING_FRAMEWORK" "missing Contents/Frameworks/Sparkle.framework" "$TMP_ROOT/missing-framework.out"

CORRUPT_FRAMEWORK="$TMP_ROOT/corrupt-framework.app"
make_fixture "$CORRUPT_FRAMEWORK"
/usr/libexec/PlistBuddy -c 'Set :CFBundleShortVersionString 2.9.1' \
  "$CORRUPT_FRAMEWORK/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/Info.plist"
assert_rejected "$CORRUPT_FRAMEWORK" "Sparkle version mismatch" "$TMP_ROOT/corrupt-framework.out"

EXTRA_LINK="$TMP_ROOT/extra-link.app"
make_fixture "$EXTRA_LINK"
ln -s Versions/Current/Resources "$EXTRA_LINK/Contents/Frameworks/Sparkle.framework/Extra"
assert_rejected "$EXTRA_LINK" "unexpected Sparkle symlink" "$TMP_ROOT/extra-link.out"

ABSOLUTE_LINK="$TMP_ROOT/absolute-link.app"
make_fixture "$ABSOLUTE_LINK"
rm "$ABSOLUTE_LINK/Contents/Frameworks/Sparkle.framework/Resources"
ln -s /private/etc/passwd "$ABSOLUTE_LINK/Contents/Frameworks/Sparkle.framework/Resources"
assert_rejected "$ABSOLUTE_LINK" "invalid Sparkle symlink target" "$TMP_ROOT/absolute-link.out"

PARENT_LINK="$TMP_ROOT/parent-link.app"
make_fixture "$PARENT_LINK"
rm "$PARENT_LINK/Contents/Frameworks/Sparkle.framework/Resources"
ln -s ../Resources "$PARENT_LINK/Contents/Frameworks/Sparkle.framework/Resources"
assert_rejected "$PARENT_LINK" "invalid Sparkle symlink target" "$TMP_ROOT/parent-link.out"

DANGLING_LINK="$TMP_ROOT/dangling-link.app"
make_fixture "$DANGLING_LINK"
rm -rf "$DANGLING_LINK/Contents/Frameworks/Sparkle.framework/Versions/B/Resources"
assert_rejected "$DANGLING_LINK" "dangling Sparkle symlink" "$TMP_ROOT/dangling-link.out"

missing_index=0
while IFS='|' read -r missing_relative expected_error; do
  MISSING_HELPER="$TMP_ROOT/missing-sparkle-helper-$missing_index.app"
  make_fixture "$MISSING_HELPER"
  rm -rf "$MISSING_HELPER/Contents/Frameworks/Sparkle.framework/$missing_relative"
  assert_rejected "$MISSING_HELPER" "$expected_error" "$TMP_ROOT/missing-helper-$missing_index.out"
  missing_index=$((missing_index + 1))
done <<'EOF'
Versions/B/Sparkle|missing Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle
Versions/B/Autoupdate|missing Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate
Versions/B/Updater.app|missing Sparkle Updater.app
Versions/B/XPCServices/Downloader.xpc|missing Sparkle Downloader.xpc
Versions/B/XPCServices/Installer.xpc|missing Sparkle Installer.xpc
EOF

MISSING_RPATH="$TMP_ROOT/missing-framework-rpath.app"
make_fixture "$MISSING_RPATH"
/usr/bin/install_name_tool -delete_rpath '@executable_path/../Frameworks' "$MISSING_RPATH/Contents/MacOS/Patchwright"
assert_rejected "$MISSING_RPATH" "Patchwright executable is missing the app Frameworks rpath" "$TMP_ROOT/missing-rpath.out"

WRONG_INSTALL_NAME="$TMP_ROOT/wrong-framework-install-name.app"
make_fixture "$WRONG_INSTALL_NAME"
/usr/bin/install_name_tool -id '@rpath/Sparkle.framework/Versions/B/NotSparkle' \
  "$WRONG_INSTALL_NAME/Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle"
assert_rejected "$WRONG_INSTALL_NAME" "Sparkle framework install name mismatch" "$TMP_ROOT/wrong-install-name.out"

LEGITIMATE_RESOURCES="$TMP_ROOT/legitimate-sparkle-resources.app"
make_fixture "$LEGITIMATE_RESOURCES"
printf 'non-code release notes fixture\n' \
  >"$LEGITIMATE_RESOURCES/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/release-notes.txt"
printf '#!/bin/sh\nexit 0\n' \
  >"$LEGITIMATE_RESOURCES/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/resource-helper.sh"
chmod 755 "$LEGITIMATE_RESOURCES/Contents/Frameworks/Sparkle.framework/Versions/B/Resources/resource-helper.sh"
"$ROOT_DIR/script/validate_bundle.sh" "$LEGITIMATE_RESOURCES"

unexpected_index=0
while IFS='|' read -r object_kind injected_relative; do
  UNEXPECTED_CODE="$TMP_ROOT/unexpected-sparkle-code-$unexpected_index.app"
  make_fixture "$UNEXPECTED_CODE"
  injected="$UNEXPECTED_CODE/Contents/Frameworks/Sparkle.framework/$injected_relative"
  case "$object_kind" in
    app|xpc) mkdir -p "$injected" ;;
    dylib)
      cp "$UNEXPECTED_CODE/Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle" "$injected"
      chmod 755 "$injected"
      ;;
    macho)
      cp /usr/bin/true "$injected"
      chmod 755 "$injected"
      ;;
    *) fail "unknown unexpected-code fixture kind: $object_kind" ;;
  esac
  assert_rejected "$UNEXPECTED_CODE" "unexpected Sparkle nested code object" "$TMP_ROOT/unexpected-code-$unexpected_index.out"
  if "$ROOT_DIR/script/verify_signing.sh" "$UNEXPECTED_CODE" >"$TMP_ROOT/unexpected-signing-$unexpected_index.out" 2>&1; then
    fail "signing verification accepted an unexpected Sparkle $object_kind object"
  fi
  grep -Fq "unexpected Sparkle nested code object" "$TMP_ROOT/unexpected-signing-$unexpected_index.out" \
    || fail "signing verification did not apply the discovered-object contract for $object_kind"
  unexpected_index=$((unexpected_index + 1))
done <<'EOF'
app|Versions/B/Injected.app
xpc|Versions/B/XPCServices/Injected.xpc
dylib|Versions/B/Injected.dylib
macho|Versions/B/Resources/InjectedTool
EOF

require_text script/build_release_components.sh 'swift build -c release --show-bin-path'
require_text script/build_release_components.sh '/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$APP_PATH/Contents/Frameworks/Sparkle.framework"'
require_text script/build_and_run.sh '/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"'
require_text script/build_release_components.sh 'cp "$ROOT_DIR/Packaging/Patchwright.icns" "$APP_PATH/Contents/Resources/Patchwright.icns"'
require_text script/build_and_run.sh 'cp "$ROOT_DIR/Packaging/Patchwright.icns" "$APP_BUNDLE/Contents/Resources/Patchwright.icns"'
require_text Sources/PatchwrightApp/Views/SettingsView.swift 'SetupGuidance.readOnlyGitHub'
require_text Sources/PatchwrightApp/Views/SettingsView.swift 'SetupGuidance.maximumPermissions'
for expected_copy in \
  'No GitHub App or private key is required' \
  'does not issue GitHub mutations' \
  'Codex is not bundled' \
  'no publisher App credential or private key'; do
  require_text README.md "$expected_copy"
done
if grep -Fq 'Git, and the Codex CLI' "$ROOT_DIR/README.md"; then
  fail "README must not call Codex a mandatory source-build requirement"
fi
require_text script/sign_release.sh '--preserve-metadata=entitlements'
require_text script/sign_release.sh 'PATCHWRIGHT_SIGNING_KEYCHAIN'
require_text script/sign_release.sh 'security find-identity -p codesigning -v "${identity_keychain_args[@]}"'
require_text script/sign_release.sh 'CODESIGN_KEYCHAIN_ARGS=(--keychain "$SIGNING_KEYCHAIN")'
require_text script/sign_release.sh '"${CODESIGN_KEYCHAIN_ARGS[@]}"'
require_text script/verify_signing.sh 'grep -Eq '\''flags=.*runtime'\'' <<<"$MAIN_DETAILS"'
require_text script/verify_signing.sh 'grep -Eq '\''flags=.*runtime'\'' <<<"$DETAILS"'
if grep -Eq 'printf .*\| grep -[EF]*q' "$ROOT_DIR/script/verify_signing.sh"; then
  fail "signing verification must not combine pipefail with early-exit grep"
fi
if grep -Eq 'codesign .*--deep|codesign --force --deep' "$ROOT_DIR/script/sign_release.sh"; then
  fail "release signing must not use codesign --deep"
fi
if grep -Eq 'codesign --force --deep' "$ROOT_DIR/script/build_and_run.sh"; then
  fail "local app signing must enumerate Sparkle nested code rather than use --deep"
fi
python3 - "$ROOT_DIR/script/sign_release.sh" <<'PY'
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_text(encoding="utf-8")
if source.count('/usr/bin/codesign --force') != source.count('"${CODESIGN_KEYCHAIN_ARGS[@]}"'):
    raise SystemExit("release contract failed: every release codesign call must select the configured keychain")
positions = [
    source.index('Installer.xpc'),
    source.index('Downloader.xpc'),
    source.index('Versions/B/Autoupdate'),
    source.index('Versions/B/Updater.app'),
    source.index('  "$SPARKLE"\n/usr/bin/codesign'),
    source.index('patchwright-engine'),
    source.index('patchwright-relay'),
    source.rindex('  "$APP_PATH"'),
]
if positions != sorted(positions) or len(set(positions)) != len(positions):
    raise SystemExit("release contract failed: nested signing order is not explicit and deepest-first")
PY
for verification_text in \
  'Installer.xpc' \
  'Downloader.xpc' \
  'Versions/B/Autoupdate' \
  'Versions/B/Updater.app' \
  'Sparkle.framework' \
  '"Developer ID Application:"*' \
  'Authority=$AUTHORITY' \
  'TeamIdentifier=' \
  'flags=.*runtime' \
  'Timestamp=' \
  'unreviewed entitlements'; do
  require_text script/verify_signing.sh "$verification_text"
done

/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier example.invalid' "$APP/Contents/Info.plist"
if "$ROOT_DIR/script/validate_bundle.sh" "$APP" >/dev/null 2>&1; then
  fail "mismatched bundle identifier was accepted"
fi
cp "$ROOT_DIR/Packaging/Info.plist" "$APP/Contents/Info.plist"

ln -s /private/etc/passwd "$APP/Contents/escape"
if "$ROOT_DIR/script/validate_bundle.sh" "$APP" >/dev/null 2>&1; then
  fail "escaping symlink was accepted"
fi
rm "$APP/Contents/escape"

if PATCHWRIGHT_DEVELOPER_ID= "$ROOT_DIR/script/sign_release.sh" "$APP" >"$TMP_ROOT/sign.out" 2>&1; then
  fail "signing succeeded without a Developer ID Application identity"
fi
grep -q 'blocked:external.*Developer ID Application' "$TMP_ROOT/sign.out" || fail "signing blocker was not explicit"

/usr/bin/codesign --force --sign - "$APP/Contents/Helpers/patchwright-engine" >/dev/null
/usr/bin/codesign --force --sign - "$APP/Contents/Helpers/patchwright-relay" >/dev/null
/usr/bin/codesign --force --sign - "$APP" >/dev/null
if "$ROOT_DIR/script/verify_signing.sh" "$APP" >"$TMP_ROOT/adhoc.out" 2>&1; then
  fail "ad-hoc signature was accepted as Developer ID"
fi
grep -q 'wrong identity class' "$TMP_ROOT/adhoc.out" || fail "ad-hoc rejection was not explicit"

if PATCHWRIGHT_NOTARY_PROFILE= "$ROOT_DIR/script/notarize_release.sh" \
    "$APP" "$TMP_ROOT/notary-app.json" "$TMP_ROOT/private-notary" app >"$TMP_ROOT/notary.out" 2>&1; then
  fail "notarization succeeded without a Keychain profile"
fi
grep -q 'blocked:external.*PATCHWRIGHT_NOTARY_PROFILE' "$TMP_ROOT/notary.out" || fail "notary blocker was not explicit"

[[ -x "$ROOT_DIR/script/package_release.sh" ]] || fail "script/package_release.sh must be executable"
[[ -x "$ROOT_DIR/script/generate_candidate_evidence.py" ]] || fail "script/generate_candidate_evidence.py must be executable"
require_text script/release.sh 'exec "$ROOT_DIR/script/package_release.sh" "$@"'
if grep -Eq 'release_readiness|promote_release|PATCHWRIGHT_(REPO|CODEX|GITHUB|CLEAN_MACHINE)_VERIFIED' \
    "$ROOT_DIR/script/release.sh"; then
  fail "release.sh must only delegate to candidate packaging"
fi

for packaging_text in \
  'refs/tags/v$VERSION^{commit}' \
  'generate_appcast' \
  '--account "$SPARKLE_ACCOUNT"' \
  'SPARKLE_ACCOUNT="ai.patchwright.app.release-v1"' \
  'PATCHWRIGHT_SIGNING_KEYCHAIN' \
  'security list-keychains -d user -s "$SIGNING_KEYCHAIN"' \
  'restore_keychain_search_list' \
  '--download-url-prefix "https://github.com/s1korrrr/patchwright/releases/download/v$VERSION"' \
  'sign_update' \
  '--verify "$APPCAST_PATH"' \
  'generate_candidate_evidence.py' \
  'generate_release_compliance.py' \
  'scan_publication_secrets.sh' \
  'verify_release_evidence.py" candidate' \
  'PATCHWRIGHT_CANDIDATE_MANIFEST=' \
  'PATCHWRIGHT_STATUS=notarized-candidate'; do
  require_text script/package_release.sh "$packaging_text"
done
python3 - "$ROOT_DIR/script/package_release.sh" <<'PY'
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_text(encoding="utf-8")
ordered = [
    '"$ROOT_DIR/script/sign_release.sh"',
    'generate_release_compliance.py',
    '# Preliminary scan',
    'generate_candidate_evidence.py',
    '# Final scan',
    '--phase checksums',
    'verify_release_evidence.py" candidate',
]
positions = [source.index(marker) for marker in ordered]
if positions != sorted(positions):
    raise SystemExit("candidate packaging order is not sign -> compliance -> preliminary scan -> evidence -> final scan -> freeze -> verify")
PY
if grep -En 'security[[:space:]]+export|SecItemExport|\.p12|export_selected_identity|--ed-key-file|private.?key' \
    "$ROOT_DIR/script/package_release.sh" "$ROOT_DIR/script/notarize_release.sh"; then
  fail "candidate packaging must never export or accept private signing key material"
fi

CHECKSUM_ROOT="$TMP_ROOT/checksum-root"
mkdir -p "$CHECKSUM_ROOT/evidence" "$CHECKSUM_ROOT/nested"
printf 'alpha\n' >"$CHECKSUM_ROOT/nested/a.txt"
"$ROOT_DIR/script/generate_release_metadata.sh" --phase checksums --output-root "$CHECKSUM_ROOT"
EXPECTED_DIGEST="$(shasum -a 256 "$CHECKSUM_ROOT/nested/a.txt" | awk '{print $1}')"
grep -Fxq "$EXPECTED_DIGEST  nested/a.txt" "$CHECKSUM_ROOT/evidence/SHA256SUMS" \
  || fail "SHA256SUMS must contain portable release-root-relative paths"
if grep -Fq "$CHECKSUM_ROOT" "$CHECKSUM_ROOT/evidence/SHA256SUMS"; then
  fail "SHA256SUMS leaked an absolute operator path"
fi

NONREGULAR_ROOT="$TMP_ROOT/nonregular-root"
mkdir -p "$NONREGULAR_ROOT/evidence"
mkfifo "$NONREGULAR_ROOT/unsupported.fifo"
if "$ROOT_DIR/script/generate_release_metadata.sh" --phase checksums --output-root "$NONREGULAR_ROOT" \
    >"$TMP_ROOT/nonregular.out" 2>&1; then
  fail "checksum freeze accepted a non-regular candidate entry"
fi
grep -Fq 'unsupported candidate file type' "$TMP_ROOT/nonregular.out" \
  || fail "checksum freeze did not report the non-regular entry"

READINESS="$TMP_ROOT/readiness.json"
if "$ROOT_DIR/script/release_readiness.sh" --app "$APP" --json "$READINESS" >/dev/null 2>&1; then
  fail "readiness succeeded for an unsigned fixture"
fi
[[ ! -e "$READINESS" ]] || fail "legacy readiness arguments must not create evidence"

if PATCHWRIGHT_CLEAN_MACHINE= "$ROOT_DIR/script/clean_machine_probe.sh" missing.dmg "$TMP_ROOT/clean" >"$TMP_ROOT/clean.out" 2>&1; then
  fail "clean-machine probe ran without its explicit clean-VM gate"
fi
grep -q 'blocked:external.*clean macOS' "$TMP_ROOT/clean.out" || fail "clean-machine blocker was not explicit"

echo "Patchwright release contract passed"
