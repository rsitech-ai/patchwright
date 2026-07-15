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

grep -Eq '^license = "MIT OR Apache-2\.0"$' "$ROOT_DIR/Cargo.toml" \
  || fail 'Cargo.toml must declare license = "MIT OR Apache-2.0"'

BUNDLE_COPYRIGHT="$(/usr/libexec/PlistBuddy -c 'Print :NSHumanReadableCopyright' "$ROOT_DIR/Packaging/Info.plist")"
[[ "$BUNDLE_COPYRIGHT" != *"All rights reserved"* ]] \
  || fail "bundle copyright must not claim All rights reserved"

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
  local swift_bin_dir
  swift_bin_dir="$(cd "$ROOT_DIR" && swift build -c release --show-bin-path)"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Frameworks"
  cp "$swift_bin_dir/Patchwright" "$app/Contents/MacOS/Patchwright"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-engine"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-relay"
  chmod 755 "$app/Contents/MacOS/Patchwright" "$app/Contents/Helpers/patchwright-engine" "$app/Contents/Helpers/patchwright-relay"
  cp "$ROOT_DIR/Packaging/Info.plist" "$app/Contents/Info.plist"
  /usr/bin/ditto "$swift_bin_dir/Sparkle.framework" "$app/Contents/Frameworks/Sparkle.framework"
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
  script/notarize_release.sh \
  script/verify_distribution.sh \
  script/release_readiness.sh; do
  [[ -f "$ROOT_DIR/$required" ]] || fail "missing $required"
done

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
make_fixture "$APP"
"$ROOT_DIR/script/validate_bundle.sh" "$APP"

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

require_text script/build_release_components.sh 'swift build -c release --show-bin-path'
require_text script/build_release_components.sh '/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$APP_PATH/Contents/Frameworks/Sparkle.framework"'
require_text script/build_and_run.sh '/usr/bin/ditto "$SPARKLE_FRAMEWORK" "$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"'
require_text script/sign_release.sh '--preserve-metadata=entitlements'
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

if PATCHWRIGHT_NOTARY_PROFILE= "$ROOT_DIR/script/notarize_release.sh" "$APP" "$TMP_ROOT/notary" >"$TMP_ROOT/notary.out" 2>&1; then
  fail "notarization succeeded without a Keychain profile"
fi
grep -q 'blocked:external.*PATCHWRIGHT_NOTARY_PROFILE' "$TMP_ROOT/notary.out" || fail "notary blocker was not explicit"

READINESS="$TMP_ROOT/readiness.json"
if "$ROOT_DIR/script/release_readiness.sh" --app "$APP" --json "$READINESS" >/dev/null 2>&1; then
  fail "readiness succeeded for an unsigned fixture"
fi
jq -e '.repo_ready == false and .developer_id == false and .release_candidate_ready == false' "$READINESS" >/dev/null \
  || fail "readiness JSON overstated the fixture"

if PATCHWRIGHT_CLEAN_MACHINE= "$ROOT_DIR/script/clean_machine_probe.sh" missing.dmg "$TMP_ROOT/clean" >"$TMP_ROOT/clean.out" 2>&1; then
  fail "clean-machine probe ran without its explicit clean-VM gate"
fi
grep -q 'blocked:external.*clean macOS' "$TMP_ROOT/clean.out" || fail "clean-machine blocker was not explicit"

echo "Patchwright release contract passed"
