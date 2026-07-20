#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR=""
VERSION=""
BUILD=""
TAG=""
STAGE=""

cleanup() {
  if [[ -n "$STAGE" && -e "$STAGE" ]]; then
    /usr/bin/trash "$STAGE" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

fail() {
  echo "community release failed: $*" >&2
  exit 65
}

usage() {
  cat >&2 <<'EOF'
usage: package_community_release.sh \
  --output /absolute/path/output \
  --version X.Y.Z \
  --build N \
  --tag vX.Y.Z-community.N
EOF
  exit 64
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) [[ $# -ge 2 ]] || usage; OUTPUT_DIR="$2"; shift 2 ;;
    --version) [[ $# -ge 2 ]] || usage; VERSION="$2"; shift 2 ;;
    --build) [[ $# -ge 2 ]] || usage; BUILD="$2"; shift 2 ;;
    --tag) [[ $# -ge 2 ]] || usage; TAG="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[[ "$OUTPUT_DIR" == /* ]] || fail "--output must be an absolute directory path"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid community release version"
[[ "$BUILD" =~ ^[1-9][0-9]*$ ]] || fail "invalid community release build"
ESCAPED_VERSION="${VERSION//./\\.}"
[[ "$TAG" =~ ^v${ESCAPED_VERSION}-community\.[1-9][0-9]*$ ]] \
  || fail "community release tag must match v$VERSION-community.N"

COMMIT="$(git -C "$ROOT_DIR" rev-parse HEAD)"
[[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] \
  || fail "community release worktree must be clean"
TAG_COMMIT="$(git -C "$ROOT_DIR" rev-parse "refs/tags/$TAG^{commit}" 2>/dev/null || true)"
[[ "$TAG_COMMIT" == "$COMMIT" ]] || fail "community release tag must resolve to HEAD"

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-community-package.XXXXXX")"
ASSEMBLY_LOG="$STAGE/community-assembly.log"
BUILD_WORK_ROOT="${PATCHWRIGHT_RELEASE_WORK_ROOT:-$STAGE/builds}"
mkdir -p "$BUILD_WORK_ROOT"
PATCHWRIGHT_VERSION="$VERSION" PATCHWRIGHT_BUILD="$BUILD" \
  PATCHWRIGHT_RELEASE_WORK_ROOT="$BUILD_WORK_ROOT" \
  "$ROOT_DIR/script/build_release_components.sh" --community | tee "$ASSEMBLY_LOG"

RELEASE_ROOT="$(sed -n 's/^PATCHWRIGHT_RELEASE_ROOT=//p' "$ASSEMBLY_LOG" | tail -1)"
APP_PATH="$(sed -n 's/^PATCHWRIGHT_APP_PATH=//p' "$ASSEMBLY_LOG" | tail -1)"
ASSEMBLY="$(sed -n 's/^PATCHWRIGHT_COMMUNITY_ASSEMBLY=//p' "$ASSEMBLY_LOG" | tail -1)"
[[ -d "$RELEASE_ROOT" && ! -L "$RELEASE_ROOT" ]] || fail "community assembly root is invalid"
[[ -d "$APP_PATH" && ! -L "$APP_PATH" ]] || fail "community assembly app is invalid"
[[ -f "$ASSEMBLY" && ! -L "$ASSEMBLY" ]] || fail "community assembly evidence is missing"
RELEASE_REAL="$(cd "$RELEASE_ROOT" && pwd -P)"
APP_REAL="$(cd "$APP_PATH" && pwd -P)"
ASSEMBLY_REAL="$(cd "$(dirname "$ASSEMBLY")" && pwd -P)/$(basename "$ASSEMBLY")"
[[ "$APP_REAL" == "$RELEASE_REAL/"* && "$ASSEMBLY_REAL" == "$RELEASE_REAL/"* ]] \
  || fail "community assembly paths escape the release root"

# Recheck source identity after the build, then bind every assembled byte before
# making the public archive. This closes both source drift and post-build swaps.
[[ "$(git -C "$ROOT_DIR" rev-parse HEAD)" == "$COMMIT" ]] \
  || fail "community release source changed during assembly"
[[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] \
  || fail "community release worktree changed during assembly"
[[ "$(git -C "$ROOT_DIR" rev-parse "refs/tags/$TAG^{commit}" 2>/dev/null || true)" == "$COMMIT" ]] \
  || fail "community release tag changed during assembly"
[[ -f "$RELEASE_ROOT/evidence/SHA256SUMS" ]] || fail "community assembly checksums are missing"
if ! (cd "$RELEASE_ROOT" && shasum -a 256 -c evidence/SHA256SUMS >/dev/null); then
  fail "community assembly checksums failed"
fi

SOURCE_ARCHIVE="$RELEASE_ROOT/reproducibility/source.tar.gz"
SBOM_SOURCE="$RELEASE_ROOT/evidence/sbom.spdx.json"
NOTICES_SOURCE="$RELEASE_ROOT/evidence/third-party-notices.md"
for required in "$SOURCE_ARCHIVE" "$SBOM_SOURCE" "$NOTICES_SOURCE" \
  "$APP_PATH/Contents/Resources/PrivacyInfo.xcprivacy" \
  "$APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md"; do
  [[ -f "$required" && ! -L "$required" ]] || fail "required assembled file is missing: $required"
done
[[ -d "$APP_PATH/Contents/Resources/third-party-licenses" ]] \
  || fail "third-party license tree is missing"
find "$APP_PATH/Contents/Resources/third-party-licenses" -type f -print -quit | grep -q . \
  || fail "third-party license tree is empty"

INFO_PLIST="$APP_PATH/Contents/Info.plist"
EXECUTABLE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$INFO_PLIST" 2>/dev/null || true)"
APP_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INFO_PLIST" 2>/dev/null || true)"
APP_BUILD="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$INFO_PLIST" 2>/dev/null || true)"
MINIMUM_MACOS="$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$INFO_PLIST" 2>/dev/null || true)"
[[ -n "$EXECUTABLE_NAME" && -f "$APP_PATH/Contents/MacOS/$EXECUTABLE_NAME" ]] \
  || fail "app bundle executable is missing"
[[ "$APP_VERSION" == "$VERSION" ]] || fail "app version does not match --version"
[[ "$APP_BUILD" == "$BUILD" ]] || fail "app build does not match --build"
[[ -n "$MINIMUM_MACOS" ]] || fail "app minimum macOS version is missing"

/usr/bin/codesign --verify --deep --strict "$APP_PATH"
SIGNING_DETAILS="$(/usr/bin/codesign -dv --verbose=4 "$APP_PATH" 2>&1)"
grep -Fq 'Signature=adhoc' <<<"$SIGNING_DETAILS" \
  || fail "community app must use an ad-hoc signature"
TEAM_ID="$(sed -n 's/^TeamIdentifier=//p' <<<"$SIGNING_DETAILS")"
[[ -z "$TEAM_ID" || "$TEAM_ID" == "not set" ]] \
  || fail "community app must not carry a Developer ID team identifier"

ARCHS="$(/usr/bin/lipo -archs "$APP_PATH/Contents/MacOS/$EXECUTABLE_NAME")"
case "$ARCHS" in
  arm64) ARCHITECTURE="arm64" ;;
  x86_64) ARCHITECTURE="x86_64" ;;
  'x86_64 arm64'|'arm64 x86_64') ARCHITECTURE="universal2" ;;
  *) fail "unsupported app architecture: $ARCHS" ;;
esac

SOURCE_SHA256="$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print $1}')"
SBOM_SHA256="$(shasum -a 256 "$SBOM_SOURCE" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$NOTICES_SOURCE" | awk '{print $1}')"
ASSEMBLY_SHA256="$(shasum -a 256 "$ASSEMBLY" | awk '{print $1}')"
jq -e --arg app_path "$APP_PATH" --arg version "$VERSION" --arg build "$BUILD" \
  --arg git_commit "$COMMIT" --arg source_sha256 "$SOURCE_SHA256" \
  --arg sbom_sha256 "$SBOM_SHA256" --arg notices_sha256 "$NOTICES_SHA256" \
  '.schema_version == 1 and .kind == "patchwright.community-assembly" and
   .app_path == $app_path and .version == $version and .build == $build and
   .git_commit == $git_commit and .dirty == false and .signing == "ad-hoc" and
   .notarized == false and .source_archive_sha256 == $source_sha256 and
   .compliance.sbom_sha256 == $sbom_sha256 and
   .compliance.third_party_notices_sha256 == $notices_sha256' \
  "$ASSEMBLY" >/dev/null || fail "community assembly evidence does not match assembled bytes"

RELEASE_SUFFIX="${TAG#v$VERSION-}"
ARCHIVE_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-macos-$ARCHITECTURE.zip"
MANIFEST_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-manifest.json"
CHECKSUM_NAME="$ARCHIVE_NAME.sha256"
SBOM_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-sbom.spdx.json"
NOTICES_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-third-party-notices.md"
mkdir -p "$OUTPUT_DIR"
for path in "$OUTPUT_DIR/$ARCHIVE_NAME" "$OUTPUT_DIR/$CHECKSUM_NAME" \
  "$OUTPUT_DIR/$MANIFEST_NAME" "$OUTPUT_DIR/$SBOM_NAME" "$OUTPUT_DIR/$NOTICES_NAME"; do
  [[ ! -e "$path" ]] || fail "refusing to overwrite existing release output: $path"
done

/usr/bin/ditto -c -k --keepParent --sequesterRsrc "$APP_PATH" "$STAGE/$ARCHIVE_NAME"
ARCHIVE_SHA256="$(shasum -a 256 "$STAGE/$ARCHIVE_NAME" | awk '{print $1}')"
printf '%s  %s\n' "$ARCHIVE_SHA256" "$ARCHIVE_NAME" >"$STAGE/$CHECKSUM_NAME"
cp "$SBOM_SOURCE" "$STAGE/$SBOM_NAME"
cp "$NOTICES_SOURCE" "$STAGE/$NOTICES_NAME"
CREATED_AT="$(git -C "$ROOT_DIR" show -s --format=%cI "$COMMIT")"
jq -n \
  --arg version "$VERSION" --arg build "$BUILD" --arg tag "$TAG" \
  --arg git_commit "$COMMIT" --arg archive "$ARCHIVE_NAME" \
  --arg archive_sha256 "$ARCHIVE_SHA256" --arg minimum_macos "$MINIMUM_MACOS" \
  --arg architecture "$ARCHITECTURE" --arg created_at "$CREATED_AT" \
  --arg source_archive_sha256 "$SOURCE_SHA256" --arg sbom "$SBOM_NAME" \
  --arg sbom_sha256 "$SBOM_SHA256" --arg notices "$NOTICES_NAME" \
  --arg third_party_notices_sha256 "$NOTICES_SHA256" \
  --arg community_assembly_sha256 "$ASSEMBLY_SHA256" \
  '{schema_version:1,kind:"patchwright.community-prerelease",version:$version,build:$build,
    tag:$tag,git_commit:$git_commit,archive:$archive,archive_sha256:$archive_sha256,
    platform:"macOS",minimum_macos:$minimum_macos,architecture:$architecture,
    signing:"ad-hoc",notarized:false,created_at:$created_at,
    source_archive_sha256:$source_archive_sha256,sbom:$sbom,sbom_sha256:$sbom_sha256,
    third_party_notices:$notices,third_party_notices_sha256:$third_party_notices_sha256,
    community_assembly_sha256:$community_assembly_sha256,
    install_warning:"This community build is not Developer ID signed or Apple notarized. macOS Gatekeeper may block it; build from source if you require a locally trusted copy."}' \
  >"$STAGE/$MANIFEST_NAME"

for asset in "$ARCHIVE_NAME" "$CHECKSUM_NAME" "$MANIFEST_NAME" "$SBOM_NAME" "$NOTICES_NAME"; do
  /bin/mv "$STAGE/$asset" "$OUTPUT_DIR/$asset"
done
printf 'PATCHWRIGHT_COMMUNITY_ARCHIVE=%s\n' "$OUTPUT_DIR/$ARCHIVE_NAME"
printf 'PATCHWRIGHT_COMMUNITY_CHECKSUM=%s\n' "$OUTPUT_DIR/$CHECKSUM_NAME"
printf 'PATCHWRIGHT_COMMUNITY_MANIFEST=%s\n' "$OUTPUT_DIR/$MANIFEST_NAME"
printf 'PATCHWRIGHT_COMMUNITY_SBOM=%s\n' "$OUTPUT_DIR/$SBOM_NAME"
printf 'PATCHWRIGHT_COMMUNITY_NOTICES=%s\n' "$OUTPUT_DIR/$NOTICES_NAME"
printf 'PATCHWRIGHT_STATUS=community-prerelease-not-notarized\n'
