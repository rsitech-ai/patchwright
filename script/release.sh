#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSEMBLY_OUTPUT="$("$ROOT_DIR/script/build_release_components.sh")"
RELEASE_ROOT="$(printf '%s\n' "$ASSEMBLY_OUTPUT" | sed -n 's/^PATCHWRIGHT_RELEASE_ROOT=//p')"
APP_PATH="$(printf '%s\n' "$ASSEMBLY_OUTPUT" | sed -n 's/^PATCHWRIGHT_APP_PATH=//p')"
[[ -d "$APP_PATH" && -d "$RELEASE_ROOT" ]] || { echo "release assembly did not return valid paths" >&2; exit 65; }
"$ROOT_DIR/script/assert_release_assembly.sh" "$RELEASE_ROOT/evidence/assembly.json"

PATCHWRIGHT_REPO_VERIFIED=1 "$ROOT_DIR/script/sign_release.sh" "$APP_PATH"
PATCHWRIGHT_REPO_VERIFIED=1 "$ROOT_DIR/script/notarize_release.sh" "$APP_PATH" "$RELEASE_ROOT/evidence"
DMG_PATH="$RELEASE_ROOT/Patchwright-${PATCHWRIGHT_VERSION:-0.1.0}.dmg"
"$ROOT_DIR/script/create_dmg.sh" "$APP_PATH" "$DMG_PATH"
"$ROOT_DIR/script/notarize_release.sh" "$DMG_PATH" "$RELEASE_ROOT/evidence"
"$ROOT_DIR/script/verify_distribution.sh" "$DMG_PATH"
"$ROOT_DIR/script/generate_release_metadata.sh" "$APP_PATH" "$RELEASE_ROOT"
"$ROOT_DIR/script/verify_reproducibility_bundle.sh" "$RELEASE_ROOT"
PATCHWRIGHT_REPO_VERIFIED=1 "$ROOT_DIR/script/release_readiness.sh" --app "$APP_PATH" --dmg "$DMG_PATH" --json "$RELEASE_ROOT/evidence/readiness.json"
echo "PATCHWRIGHT_DMG_PATH=$DMG_PATH"
