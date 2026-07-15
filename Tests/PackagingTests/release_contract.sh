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
  grep -Fq "$expected" "$ROOT_DIR/$path" || fail "$path is missing required text: $expected"
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
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers"
  cp /usr/bin/true "$app/Contents/MacOS/Patchwright"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-engine"
  cp /usr/bin/true "$app/Contents/Helpers/patchwright-relay"
  chmod 755 "$app/Contents/MacOS/Patchwright" "$app/Contents/Helpers/patchwright-engine" "$app/Contents/Helpers/patchwright-relay"
  cp "$ROOT_DIR/Packaging/Info.plist" "$app/Contents/Info.plist"
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
