#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH=""
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
  --app /absolute/path/Patchwright.app \
  --output /absolute/path/output \
  --version X.Y.Z \
  --build N \
  --tag vX.Y.Z-community.N
EOF
  exit 64
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) [[ $# -ge 2 ]] || usage; APP_PATH="$2"; shift 2 ;;
    --output) [[ $# -ge 2 ]] || usage; OUTPUT_DIR="$2"; shift 2 ;;
    --version) [[ $# -ge 2 ]] || usage; VERSION="$2"; shift 2 ;;
    --build) [[ $# -ge 2 ]] || usage; BUILD="$2"; shift 2 ;;
    --tag) [[ $# -ge 2 ]] || usage; TAG="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[[ "$APP_PATH" == /* && -d "$APP_PATH" && ! -L "$APP_PATH" ]] \
  || fail "--app must be an absolute, non-symlink app bundle"
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

RELEASE_SUFFIX="${TAG#v$VERSION-}"
ARCHIVE_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-macos-$ARCHITECTURE.zip"
MANIFEST_NAME="Patchwright-$VERSION-$RELEASE_SUFFIX-manifest.json"
CHECKSUM_NAME="$ARCHIVE_NAME.sha256"
mkdir -p "$OUTPUT_DIR"
for path in "$OUTPUT_DIR/$ARCHIVE_NAME" "$OUTPUT_DIR/$CHECKSUM_NAME" "$OUTPUT_DIR/$MANIFEST_NAME"; do
  [[ ! -e "$path" ]] || fail "refusing to overwrite existing release output: $path"
done

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-community-package.XXXXXX")"
/usr/bin/ditto "$APP_PATH" "$STAGE/Patchwright.app"
/usr/bin/ditto -c -k --keepParent --sequesterRsrc "$STAGE/Patchwright.app" "$STAGE/$ARCHIVE_NAME"
ARCHIVE_SHA256="$(shasum -a 256 "$STAGE/$ARCHIVE_NAME" | awk '{print $1}')"
printf '%s  %s\n' "$ARCHIVE_SHA256" "$ARCHIVE_NAME" >"$STAGE/$CHECKSUM_NAME"
CREATED_AT="$(git -C "$ROOT_DIR" show -s --format=%cI "$COMMIT")"
jq -n \
  --arg version "$VERSION" \
  --arg build "$BUILD" \
  --arg tag "$TAG" \
  --arg git_commit "$COMMIT" \
  --arg archive "$ARCHIVE_NAME" \
  --arg archive_sha256 "$ARCHIVE_SHA256" \
  --arg minimum_macos "$MINIMUM_MACOS" \
  --arg architecture "$ARCHITECTURE" \
  --arg created_at "$CREATED_AT" \
  '{schema_version:1,kind:"patchwright.community-prerelease",version:$version,build:$build,
    tag:$tag,git_commit:$git_commit,archive:$archive,archive_sha256:$archive_sha256,
    platform:"macOS",minimum_macos:$minimum_macos,architecture:$architecture,
    signing:"ad-hoc",notarized:false,created_at:$created_at,
    install_warning:"This community build is not Developer ID signed or Apple notarized. macOS Gatekeeper may block it; build from source if you require a locally trusted copy."}' \
  >"$STAGE/$MANIFEST_NAME"

/bin/mv "$STAGE/$ARCHIVE_NAME" "$OUTPUT_DIR/$ARCHIVE_NAME"
/bin/mv "$STAGE/$CHECKSUM_NAME" "$OUTPUT_DIR/$CHECKSUM_NAME"
/bin/mv "$STAGE/$MANIFEST_NAME" "$OUTPUT_DIR/$MANIFEST_NAME"
printf 'PATCHWRIGHT_COMMUNITY_ARCHIVE=%s\n' "$OUTPUT_DIR/$ARCHIVE_NAME"
printf 'PATCHWRIGHT_COMMUNITY_CHECKSUM=%s\n' "$OUTPUT_DIR/$CHECKSUM_NAME"
printf 'PATCHWRIGHT_COMMUNITY_MANIFEST=%s\n' "$OUTPUT_DIR/$MANIFEST_NAME"
printf 'PATCHWRIGHT_STATUS=community-prerelease-not-notarized\n'
