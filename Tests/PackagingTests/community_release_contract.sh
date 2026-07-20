#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-community-release-contract.XXXXXX")"
trap '/usr/bin/trash "$TMP_ROOT" >/dev/null 2>&1 || true' EXIT

fail() {
  echo "community release contract failed: $*" >&2
  exit 1
}

PACKAGER="$ROOT_DIR/script/package_community_release.sh"
[[ -x "$PACKAGER" ]] || fail "missing executable script/package_community_release.sh"

FIXTURE_REPO="$TMP_ROOT/repository"
FIXTURE_APP="$TMP_ROOT/Patchwright.app"
OUTPUT_DIR="$TMP_ROOT/output"
mkdir -p "$FIXTURE_REPO/script" "$FIXTURE_APP/Contents/MacOS"
cp "$PACKAGER" "$FIXTURE_REPO/script/package_community_release.sh"
xcrun clang -arch arm64 -x c -o "$FIXTURE_APP/Contents/MacOS/Patchwright" - <<'C'
int main(void) { return 0; }
C
chmod 755 "$FIXTURE_APP/Contents/MacOS/Patchwright"
cat >"$FIXTURE_APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>Patchwright</string>
  <key>CFBundleIdentifier</key>
  <string>ai.patchwright.app</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.2.0</string>
  <key>CFBundleVersion</key>
  <string>3</string>
  <key>LSMinimumSystemVersion</key>
  <string>26.0</string>
</dict>
</plist>
PLIST
/usr/bin/codesign --force --sign - "$FIXTURE_APP"

git -C "$FIXTURE_REPO" init -q
git -C "$FIXTURE_REPO" config user.name Fixture
git -C "$FIXTURE_REPO" config user.email fixture@example.invalid
git -C "$FIXTURE_REPO" add script/package_community_release.sh
git -C "$FIXTURE_REPO" commit -qm fixture
git -C "$FIXTURE_REPO" tag v0.2.0-community.1

"$FIXTURE_REPO/script/package_community_release.sh" \
  --app "$FIXTURE_APP" \
  --output "$OUTPUT_DIR" \
  --version 0.2.0 \
  --build 3 \
  --tag v0.2.0-community.1

ARCHIVE="$OUTPUT_DIR/Patchwright-0.2.0-community.1-macos-arm64.zip"
CHECKSUM="$ARCHIVE.sha256"
MANIFEST="$OUTPUT_DIR/Patchwright-0.2.0-community.1-manifest.json"
[[ -f "$ARCHIVE" && -f "$CHECKSUM" && -f "$MANIFEST" ]] \
  || fail "packager did not emit the archive, checksum, and manifest"
(cd "$OUTPUT_DIR" && shasum -a 256 -c "$(basename "$CHECKSUM")")

EXPANDED="$TMP_ROOT/expanded"
mkdir -p "$EXPANDED"
/usr/bin/ditto -x -k "$ARCHIVE" "$EXPANDED"
[[ -d "$EXPANDED/Patchwright.app" ]] || fail "archive did not preserve the app bundle"
/usr/bin/codesign --verify --deep --strict "$EXPANDED/Patchwright.app"

COMMIT="$(git -C "$FIXTURE_REPO" rev-parse HEAD)"
jq -e \
  --arg commit "$COMMIT" \
  '.schema_version == 1 and
   .kind == "patchwright.community-prerelease" and
   .version == "0.2.0" and
   .build == "3" and
   .tag == "v0.2.0-community.1" and
   .git_commit == $commit and
   .signing == "ad-hoc" and
   .notarized == false and
   .minimum_macos == "26.0" and
   .architecture == "arm64"' \
  "$MANIFEST" >/dev/null || fail "manifest did not preserve the community release boundary"

printf 'dirty\n' >"$FIXTURE_REPO/dirty.txt"
if "$FIXTURE_REPO/script/package_community_release.sh" \
  --app "$FIXTURE_APP" --output "$OUTPUT_DIR/dirty" \
  --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/dirty.out" 2>&1; then
  fail "packager accepted a dirty release repository"
fi
grep -Fq 'community release worktree must be clean' "$TMP_ROOT/dirty.out" \
  || fail "dirty-tree rejection was not explicit"

git -C "$FIXTURE_REPO" add dirty.txt
git -C "$FIXTURE_REPO" commit -qm newer
if "$FIXTURE_REPO/script/package_community_release.sh" \
  --app "$FIXTURE_APP" --output "$OUTPUT_DIR/tag-mismatch" \
  --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/tag-mismatch.out" 2>&1; then
  fail "packager accepted a release tag that did not resolve to HEAD"
fi
grep -Fq 'community release tag must resolve to HEAD' "$TMP_ROOT/tag-mismatch.out" \
  || fail "tag mismatch rejection was not explicit"

echo "Patchwright community release contract passed"
