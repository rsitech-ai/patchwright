#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?app path required}"
OUTPUT_ROOT="${2:?output root required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$OUTPUT_ROOT/evidence"
VERSION=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_PATH/Contents/Info.plist")
BUILD=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP_PATH/Contents/Info.plist")
COMMIT=$(git -C "$ROOT_DIR" rev-parse HEAD)
DIRTY=false
[[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] || DIRTY=true

jq -n \
  --arg version "$VERSION" \
  --arg build "$BUILD" \
  --arg commit "$COMMIT" \
  --arg swift "$(swift --version | head -n 1)" \
  --arg rust "$(rustc --version)" \
  --arg cargo "$(cargo --version)" \
  --argjson dirty "$DIRTY" \
  '{version:$version,build:$build,git_commit:$commit,dirty:$dirty,swift:$swift,rust:$rust,cargo:$cargo,architecture:"arm64",minimum_macos:"26.0"}' \
  >"$OUTPUT_ROOT/evidence/build-metadata.json"

"$ROOT_DIR/script/generate_symlink_manifest.py" \
  --root "$OUTPUT_ROOT" \
  --output "$OUTPUT_ROOT/evidence/SYMLINKS.json"

(
  cd "$OUTPUT_ROOT"
  find . -type f ! -path './evidence/SHA256SUMS' -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do shasum -a 256 "$file"; done
) >"$OUTPUT_ROOT/evidence/SHA256SUMS"
