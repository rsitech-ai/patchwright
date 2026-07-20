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
OUTPUT_DIR="$TMP_ROOT/output"
mkdir -p "$FIXTURE_REPO/script" "$TMP_ROOT/builds"
cp "$PACKAGER" "$FIXTURE_REPO/script/package_community_release.sh"
printf 'Apache License\nVersion 2.0, January 2004\n' >"$FIXTURE_REPO/LICENSE"
printf 'Patchwright fixture notice\n' >"$FIXTURE_REPO/NOTICE"
cat >"$FIXTURE_REPO/script/build_release_components.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == --community && $# == 1 ]] || { echo "community assembly mode required" >&2; exit 64; }
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${PATCHWRIGHT_VERSION:?version required}"
BUILD="${PATCHWRIGHT_BUILD:?build required}"
OUTPUT_PARENT="${PATCHWRIGHT_RELEASE_WORK_ROOT:?work root required}"
mkdir -p "$OUTPUT_PARENT"
WORK_ROOT="$(mktemp -d "$OUTPUT_PARENT/Patchwright-$VERSION-$BUILD.XXXXXX")"
APP_PATH="$WORK_ROOT/Patchwright.app"
mkdir -p "$APP_PATH/Contents/MacOS" "$APP_PATH/Contents/Resources/third-party-licenses/Fake" \
  "$WORK_ROOT/evidence" "$WORK_ROOT/reproducibility"
xcrun clang -arch arm64 -x c -o "$APP_PATH/Contents/MacOS/Patchwright" - <<'C'
int main(void) { return 0; }
C
chmod 755 "$APP_PATH/Contents/MacOS/Patchwright"
cat >"$APP_PATH/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Patchwright</string>
<key>CFBundleIdentifier</key><string>ai.patchwright.app</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$BUILD</string>
<key>LSMinimumSystemVersion</key><string>26.0</string>
</dict></plist>
PLIST
printf '<?xml version="1.0"?><plist version="1.0"><dict/></plist>\n' \
  >"$APP_PATH/Contents/Resources/PrivacyInfo.xcprivacy"
printf '# Third-Party Notices\n\nFake dependency.\n' \
  >"$APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md"
printf 'Fake license\n' >"$APP_PATH/Contents/Resources/third-party-licenses/Fake/LICENSE"
cp "$ROOT_DIR/LICENSE" "$APP_PATH/Contents/Resources/LICENSE.txt"
cp "$ROOT_DIR/NOTICE" "$APP_PATH/Contents/Resources/NOTICE.txt"
cp "$ROOT_DIR/LICENSE" "$WORK_ROOT/reproducibility/LICENSE"
cp "$ROOT_DIR/NOTICE" "$WORK_ROOT/reproducibility/NOTICE"
/usr/bin/codesign --force --sign - "$APP_PATH"
git -C "$ROOT_DIR" archive --format=tar.gz --output="$WORK_ROOT/reproducibility/source.tar.gz" HEAD
COMMIT="$(git -C "$ROOT_DIR" rev-parse HEAD)"
SOURCE_SHA256="$(shasum -a 256 "$WORK_ROOT/reproducibility/source.tar.gz" | awk '{print $1}')"
printf '{"spdxVersion":"SPDX-2.3","dataLicense":"CC0-1.0"}\n' >"$WORK_ROOT/evidence/sbom.spdx.json"
cp "$APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md" "$WORK_ROOT/evidence/third-party-notices.md"
SBOM_SHA256="$(shasum -a 256 "$WORK_ROOT/evidence/sbom.spdx.json" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$WORK_ROOT/evidence/third-party-notices.md" | awk '{print $1}')"
PROJECT_LICENSE_SHA256="$(shasum -a 256 "$WORK_ROOT/reproducibility/LICENSE" | awk '{print $1}')"
PROJECT_NOTICE_SHA256="$(shasum -a 256 "$WORK_ROOT/reproducibility/NOTICE" | awk '{print $1}')"
jq -n --arg app_path "$APP_PATH" --arg version "$VERSION" --arg build "$BUILD" \
  --arg git_commit "$COMMIT" --arg source_archive_sha256 "$SOURCE_SHA256" \
  --arg sbom_sha256 "$SBOM_SHA256" --arg notices_sha256 "$NOTICES_SHA256" \
  --arg project_license_sha256 "$PROJECT_LICENSE_SHA256" \
  --arg project_notice_sha256 "$PROJECT_NOTICE_SHA256" \
  '{schema_version:1,kind:"patchwright.community-assembly",app_path:$app_path,
    version:$version,build:$build,git_commit:$git_commit,dirty:false,
    signing:"ad-hoc",notarized:false,source_archive_sha256:$source_archive_sha256,
    compliance:{sbom_sha256:$sbom_sha256,third_party_notices_sha256:$notices_sha256,
      project_license_sha256:$project_license_sha256,project_notice_sha256:$project_notice_sha256}}' \
  >"$WORK_ROOT/evidence/community-assembly.json"
(
  cd "$WORK_ROOT"
  find . -type f ! -path './evidence/SHA256SUMS' -print0 | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do
        printf '%s  %s\n' "$(shasum -a 256 "$file" | awk '{print $1}')" "${file#./}"
      done
) >"$WORK_ROOT/evidence/SHA256SUMS"
if [[ "${PATCHWRIGHT_FIXTURE_TAMPER:-0}" == 1 ]]; then
  printf 'tampered\n' >>"$APP_PATH/Contents/MacOS/Patchwright"
fi
printf 'PATCHWRIGHT_RELEASE_ROOT=%s\nPATCHWRIGHT_APP_PATH=%s\n' "$WORK_ROOT" "$APP_PATH"
printf 'PATCHWRIGHT_COMMUNITY_ASSEMBLY=%s\n' "$WORK_ROOT/evidence/community-assembly.json"
SH
chmod +x "$FIXTURE_REPO/script/package_community_release.sh" "$FIXTURE_REPO/script/build_release_components.sh"

git -C "$FIXTURE_REPO" init -q
git -C "$FIXTURE_REPO" config user.name Fixture
git -C "$FIXTURE_REPO" config user.email fixture@example.invalid
git -C "$FIXTURE_REPO" add script LICENSE NOTICE
git -C "$FIXTURE_REPO" commit -qm fixture
git -C "$FIXTURE_REPO" tag v0.2.0-community.1

if "$FIXTURE_REPO/script/package_community_release.sh" \
  --app "$TMP_ROOT/unrelated.app" --output "$OUTPUT_DIR/unrelated" \
  --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/unrelated.out" 2>&1; then
  fail "packager still accepts an arbitrary app path"
fi
grep -Fq 'usage: package_community_release.sh' "$TMP_ROOT/unrelated.out" \
  || fail "arbitrary app rejection did not use the strict interface"

PATCHWRIGHT_RELEASE_WORK_ROOT="$TMP_ROOT/builds" \
  "$FIXTURE_REPO/script/package_community_release.sh" \
  --output "$OUTPUT_DIR" --version 0.2.0 --build 3 --tag v0.2.0-community.1

ARCHIVE="$OUTPUT_DIR/Patchwright-0.2.0-community.1-macos-arm64.zip"
CHECKSUM="$ARCHIVE.sha256"
MANIFEST="$OUTPUT_DIR/Patchwright-0.2.0-community.1-manifest.json"
SBOM="$OUTPUT_DIR/Patchwright-0.2.0-community.1-sbom.spdx.json"
NOTICES="$OUTPUT_DIR/Patchwright-0.2.0-community.1-third-party-notices.md"
PROJECT_LICENSE="$OUTPUT_DIR/Patchwright-0.2.0-community.1-LICENSE.txt"
PROJECT_NOTICE="$OUTPUT_DIR/Patchwright-0.2.0-community.1-NOTICE.txt"
for output in "$ARCHIVE" "$CHECKSUM" "$MANIFEST" "$SBOM" "$NOTICES" \
  "$PROJECT_LICENSE" "$PROJECT_NOTICE"; do
  [[ -f "$output" ]] || fail "packager did not emit $(basename "$output")"
done
(cd "$OUTPUT_DIR" && shasum -a 256 -c "$(basename "$CHECKSUM")")

EXPANDED="$TMP_ROOT/expanded"
mkdir -p "$EXPANDED"
/usr/bin/ditto -x -k "$ARCHIVE" "$EXPANDED"
EXPANDED_APP="$EXPANDED/Patchwright.app"
[[ -f "$EXPANDED_APP/Contents/Resources/PrivacyInfo.xcprivacy" ]] \
  || fail "community archive omitted the privacy manifest"
[[ -f "$EXPANDED_APP/Contents/Resources/THIRD_PARTY_NOTICES.md" ]] \
  || fail "community archive omitted third-party notices"
[[ -f "$EXPANDED_APP/Contents/Resources/third-party-licenses/Fake/LICENSE" ]] \
  || fail "community archive omitted the third-party license tree"
cmp "$FIXTURE_REPO/LICENSE" "$EXPANDED_APP/Contents/Resources/LICENSE.txt" \
  || fail "community archive omitted the exact Apache project license"
cmp "$FIXTURE_REPO/NOTICE" "$EXPANDED_APP/Contents/Resources/NOTICE.txt" \
  || fail "community archive omitted the exact project notice"
cmp "$FIXTURE_REPO/LICENSE" "$PROJECT_LICENSE" \
  || fail "community release asset omitted the exact Apache project license"
cmp "$FIXTURE_REPO/NOTICE" "$PROJECT_NOTICE" \
  || fail "community release asset omitted the exact project notice"
/usr/bin/codesign --verify --deep --strict "$EXPANDED_APP"

COMMIT="$(git -C "$FIXTURE_REPO" rev-parse HEAD)"
jq -e --arg commit "$COMMIT" \
  '.schema_version == 1 and .kind == "patchwright.community-prerelease" and
   .version == "0.2.0" and .build == "3" and .tag == "v0.2.0-community.1" and
   .git_commit == $commit and .signing == "ad-hoc" and .notarized == false and
   .minimum_macos == "26.0" and .architecture == "arm64" and
   (.source_archive_sha256 | length) == 64 and (.sbom_sha256 | length) == 64 and
   (.third_party_notices_sha256 | length) == 64 and
   (.project_license_sha256 | length) == 64 and (.project_notice_sha256 | length) == 64' \
  "$MANIFEST" >/dev/null || fail "manifest did not preserve source and compliance bindings"

if PATCHWRIGHT_RELEASE_WORK_ROOT="$TMP_ROOT/tampered-builds" PATCHWRIGHT_FIXTURE_TAMPER=1 \
  "$FIXTURE_REPO/script/package_community_release.sh" \
  --output "$OUTPUT_DIR/tampered" --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/tampered.out" 2>&1; then
  fail "packager accepted component bytes changed after assembly"
fi
grep -Fq 'community assembly checksums failed' "$TMP_ROOT/tampered.out" \
  || fail "tampered assembly rejection was not explicit"

printf 'dirty\n' >"$FIXTURE_REPO/dirty.txt"
if PATCHWRIGHT_RELEASE_WORK_ROOT="$TMP_ROOT/dirty-builds" \
  "$FIXTURE_REPO/script/package_community_release.sh" \
  --output "$OUTPUT_DIR/dirty" --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/dirty.out" 2>&1; then
  fail "packager accepted a dirty release repository"
fi
grep -Fq 'community release worktree must be clean' "$TMP_ROOT/dirty.out" \
  || fail "dirty-tree rejection was not explicit"

git -C "$FIXTURE_REPO" add dirty.txt
git -C "$FIXTURE_REPO" commit -qm newer
if PATCHWRIGHT_RELEASE_WORK_ROOT="$TMP_ROOT/tag-builds" \
  "$FIXTURE_REPO/script/package_community_release.sh" \
  --output "$OUTPUT_DIR/tag-mismatch" --version 0.2.0 --build 3 --tag v0.2.0-community.1 \
  >"$TMP_ROOT/tag-mismatch.out" 2>&1; then
  fail "packager accepted a release tag that did not resolve to HEAD"
fi
grep -Fq 'community release tag must resolve to HEAD' "$TMP_ROOT/tag-mismatch.out" \
  || fail "tag mismatch rejection was not explicit"

echo "Patchwright community release contract passed"
