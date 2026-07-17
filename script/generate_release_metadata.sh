#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PHASE=""
OUTPUT_ROOT=""
APP_PATH=""
if [[ "${1:-}" == --phase ]]; then
  PHASE="${2:?phase required}"; shift 2
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --output-root) OUTPUT_ROOT="${2:?output root required}"; shift 2 ;;
      --app) APP_PATH="${2:?app path required}"; shift 2 ;;
      *) echo "unknown metadata argument: $1" >&2; exit 64 ;;
    esac
  done
else
  APP_PATH="${1:?app path required}"
  OUTPUT_ROOT="${2:?output root required}"
  PHASE=all
fi
[[ -d "$OUTPUT_ROOT" && ! -L "$OUTPUT_ROOT" ]] || { echo "output root must be a real directory" >&2; exit 65; }
mkdir -p "$OUTPUT_ROOT/evidence"

write_metadata() {
  [[ -d "$APP_PATH" ]] || { echo "app path required for metadata phase" >&2; exit 64; }
  local version build commit dirty source_archive source_archive_sha256
  version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP_PATH/Contents/Info.plist")
  build=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP_PATH/Contents/Info.plist")
  commit=$(git -C "$ROOT_DIR" rev-parse HEAD)
  dirty=false; [[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] || dirty=true
  source_archive="$OUTPUT_ROOT/reproducibility/source.tar.gz"
  [[ -f "$source_archive" && ! -L "$source_archive" ]] \
    || { echo "source archive must be a regular non-symlink file" >&2; exit 65; }
  source_archive_sha256="$(shasum -a 256 "$source_archive" | awk '{print $1}')"
  jq -n --arg version "$version" --arg build "$build" --arg git_commit "$commit" \
    --arg tag "v$version" --arg artifact_filename "Patchwright-$version.dmg" \
    --arg artifact_sha256 "${PATCHWRIGHT_ARTIFACT_SHA256:-}" --argjson dirty "$dirty" \
    --arg source_archive_path "reproducibility/source.tar.gz" --arg source_archive_sha256 "$source_archive_sha256" \
    '{schema_version:1,version:$version,build:$build,git_commit:$git_commit,tag:$tag,artifact_filename:$artifact_filename,artifact_sha256:$artifact_sha256,dirty:$dirty,source_archive_path:$source_archive_path,source_archive_sha256:$source_archive_sha256,architecture:"arm64",minimum_macos:"26.0"}' \
    >"$OUTPUT_ROOT/evidence/build-metadata.json"
  "$ROOT_DIR/script/generate_symlink_manifest.py" --root "$OUTPUT_ROOT" --output "$OUTPUT_ROOT/evidence/SYMLINKS.json"
}

freeze_checksums() {
  local unsupported
  unsupported=$(find "$OUTPUT_ROOT" -mindepth 1 ! -type d ! -type f ! -type l -print -quit)
  [[ -z "$unsupported" ]] || { echo "unsupported candidate file type: ${unsupported#"$OUTPUT_ROOT/"}" >&2; exit 65; }
  local temporary="$OUTPUT_ROOT/evidence/SHA256SUMS.tmp"
  [[ ! -e "$temporary" && ! -L "$temporary" ]] || { echo "checksum temporary path exists" >&2; exit 65; }
  (
    cd "$OUTPUT_ROOT"
    find . -type f ! -path './evidence/SHA256SUMS' ! -path './evidence/SHA256SUMS.tmp' -print0 \
      | LC_ALL=C sort -z \
      | while IFS= read -r -d '' file; do
          digest=$(shasum -a 256 "$file" | awk '{print $1}')
          printf '%s  %s\n' "$digest" "${file#./}"
        done
  ) >"$temporary"
  mv "$temporary" "$OUTPUT_ROOT/evidence/SHA256SUMS"
}

case "$PHASE" in
  metadata) write_metadata ;;
  checksums) freeze_checksums ;;
  all) write_metadata; freeze_checksums ;;
  *) echo "phase must be metadata or checksums" >&2; exit 64 ;;
esac
