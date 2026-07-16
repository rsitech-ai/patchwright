#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${PATCHWRIGHT_VERSION:-0.1.0}"
BUILD="${PATCHWRIGHT_BUILD:-1}"
SPARKLE_ACCOUNT="ai.patchwright.app.release-v1"
SIGNING_KEYCHAIN="${PATCHWRIGHT_SIGNING_KEYCHAIN:-}"
APPCAST_STAGE=""
KEYCHAIN_SEARCH_LIST_CHANGED=0
ORIGINAL_KEYCHAINS=()

restore_keychain_search_list() {
  if [[ "$KEYCHAIN_SEARCH_LIST_CHANGED" == 1 && "${#ORIGINAL_KEYCHAINS[@]}" -gt 0 ]]; then
    security list-keychains -d user -s "${ORIGINAL_KEYCHAINS[@]}"
  fi
}

cleanup() {
  if [[ -n "$APPCAST_STAGE" && -e "$APPCAST_STAGE" ]]; then
    /usr/bin/trash "$APPCAST_STAGE" >/dev/null 2>&1 || true
  fi
  restore_keychain_search_list
}
trap cleanup EXIT

if [[ -n "$SIGNING_KEYCHAIN" ]]; then
  keychain_parent="$(cd "$(dirname "$SIGNING_KEYCHAIN")" 2>/dev/null && pwd -P || true)"
  canonical_keychain="$keychain_parent/$(basename "$SIGNING_KEYCHAIN")"
  keychain_mode="$(stat -f '%Lp' "$SIGNING_KEYCHAIN" 2>/dev/null || true)"
  keychain_owner="$(stat -f '%u' "$SIGNING_KEYCHAIN" 2>/dev/null || true)"
  if [[ "$SIGNING_KEYCHAIN" != /* || ! -f "$SIGNING_KEYCHAIN" || -L "$SIGNING_KEYCHAIN" \
      || "$canonical_keychain" != "$SIGNING_KEYCHAIN" || "$keychain_owner" != "$(id -u)" \
      || ! "$keychain_mode" =~ ^[0-7]{3,4}$ || $((8#$keychain_mode & 077)) -ne 0 ]]; then
    echo "blocked:external — PATCHWRIGHT_SIGNING_KEYCHAIN must be an owner-only absolute keychain file" >&2
    exit 78
  fi
  while IFS= read -r keychain_line; do
    keychain_line="${keychain_line#"${keychain_line%%[![:space:]]*}"}"
    keychain_line="${keychain_line#\"}"
    keychain_line="${keychain_line%\"}"
    [[ -z "$keychain_line" || "$keychain_line" == "$SIGNING_KEYCHAIN" ]] \
      || ORIGINAL_KEYCHAINS+=("$keychain_line")
  done < <(security list-keychains -d user)
  [[ "${#ORIGINAL_KEYCHAINS[@]}" -gt 0 ]] \
    || { echo "blocked:external — no existing user Keychain search list to preserve" >&2; exit 78; }
  security list-keychains -d user -s "$SIGNING_KEYCHAIN" "${ORIGINAL_KEYCHAINS[@]}"
  KEYCHAIN_SEARCH_LIST_CHANGED=1
fi
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && "$BUILD" =~ ^[1-9][0-9]*$ ]] || { echo "invalid release version or build" >&2; exit 64; }
COMMIT="$(git -C "$ROOT_DIR" rev-parse HEAD)"
TAG_COMMIT="$(git -C "$ROOT_DIR" rev-parse "refs/tags/v$VERSION^{commit}" 2>/dev/null || true)"
[[ "$TAG_COMMIT" == "$COMMIT" ]] || { echo "release tag v$VERSION must resolve to HEAD" >&2; exit 65; }
[[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] || { echo "release worktree must be clean" >&2; exit 65; }
"$ROOT_DIR/script/verify.sh"

ASSEMBLY_OUTPUT="$(PATCHWRIGHT_VERSION="$VERSION" PATCHWRIGHT_BUILD="$BUILD" "$ROOT_DIR/script/build_release_components.sh")"
RELEASE_ROOT="$(printf '%s\n' "$ASSEMBLY_OUTPUT" | sed -n 's/^PATCHWRIGHT_RELEASE_ROOT=//p')"
APP_PATH="$(printf '%s\n' "$ASSEMBLY_OUTPUT" | sed -n 's/^PATCHWRIGHT_APP_PATH=//p')"
PRIVATE_EVIDENCE="$RELEASE_ROOT.private"
mkdir -p "$RELEASE_ROOT/evidence" "$PRIVATE_EVIDENCE"
"$ROOT_DIR/script/assert_release_assembly.sh" "$RELEASE_ROOT/evidence/assembly.json"
"$ROOT_DIR/script/sign_release.sh" "$APP_PATH"
"$ROOT_DIR/script/notarize_release.sh" "$APP_PATH" "$RELEASE_ROOT/evidence/notary-app.json" "$PRIVATE_EVIDENCE" app
DMG_PATH="$RELEASE_ROOT/Patchwright-$VERSION.dmg"
"$ROOT_DIR/script/create_dmg.sh" "$APP_PATH" "$DMG_PATH"
"$ROOT_DIR/script/notarize_release.sh" "$DMG_PATH" "$RELEASE_ROOT/evidence/notary-dmg.json" "$PRIVATE_EVIDENCE" dmg
"$ROOT_DIR/script/verify_distribution.sh" "$DMG_PATH"
ARTIFACT_SHA256="$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')"
printf '%s  %s\n' "$ARTIFACT_SHA256" "$(basename "$DMG_PATH")" >"$DMG_PATH.sha256"
TEAM_ID="$(/usr/bin/codesign -dv --verbose=4 "$APP_PATH" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
[[ "$TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || { echo "signed app has no valid TeamIdentifier" >&2; exit 65; }

SPARKLE_BIN="$ROOT_DIR/.build/artifacts/sparkle/Sparkle/bin"
APPCAST_STAGE="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-appcast.XXXXXX")"
/usr/bin/ditto "$DMG_PATH" "$APPCAST_STAGE/$(basename "$DMG_PATH")"
"$SPARKLE_BIN/generate_appcast" --account "$SPARKLE_ACCOUNT" \
  --download-url-prefix "https://github.com/s1korrrr/patchwright/releases/download/v$VERSION" \
  --link "https://github.com/s1korrrr/patchwright/releases/tag/v$VERSION" --versions "$BUILD" --maximum-deltas 0 \
  -o "$APPCAST_STAGE/appcast.xml" "$APPCAST_STAGE"
APPCAST_PATH="$RELEASE_ROOT/appcast.xml"
/usr/bin/ditto "$APPCAST_STAGE/appcast.xml" "$APPCAST_PATH"
"$SPARKLE_BIN/sign_update" --account "$SPARKLE_ACCOUNT" --verify "$APPCAST_PATH"

# Recompute compliance from the final signed and stapled app bytes.
python3 "$ROOT_DIR/script/generate_release_compliance.py" \
  --cargo-metadata "$RELEASE_ROOT/reproducibility/cargo-metadata.json" \
  --swift-metadata "$RELEASE_ROOT/reproducibility/swift-dependencies.json" \
  --output-dir "$RELEASE_ROOT/evidence" \
  --app "$APP_PATH" \
  --engine "$APP_PATH/Contents/Helpers/patchwright-engine" \
  --relay "$APP_PATH/Contents/Helpers/patchwright-relay" \
  --license-overrides "$ROOT_DIR/Packaging/ThirdPartyLicenseOverrides"
PATCHWRIGHT_ARTIFACT_SHA256="$ARTIFACT_SHA256" "$ROOT_DIR/script/generate_release_metadata.sh" --phase metadata --app "$APP_PATH" --output-root "$RELEASE_ROOT"
jq -n \
  --arg artifact_filename "$(basename "$DMG_PATH")" --arg artifact_sha256 "$ARTIFACT_SHA256" \
  --arg git_commit "$COMMIT" --arg tag "v$VERSION" --arg version "$VERSION" --arg build "$BUILD" \
  '{schema_version:1,artifact_filename:$artifact_filename,artifact_sha256:$artifact_sha256,git_commit:$git_commit,tag:$tag,version:$version,build:$build,status:"pass",checks:{dmg_signature:true,dmg_ticket:true,dmg_gatekeeper:true,app_signature:true,app_ticket:true,app_gatekeeper:true,bundle_layout:true,team_id:true,hardened_runtime:true,entitlements:true}}' \
  >"$RELEASE_ROOT/evidence/distribution.json"
# Preliminary scan proves the inputs used to create package gate envelopes.
"$ROOT_DIR/script/scan_publication_secrets.sh" --repo "$ROOT_DIR" --artifact-root "$RELEASE_ROOT" --output "$RELEASE_ROOT/evidence/secret-scan.json"
python3 "$ROOT_DIR/script/generate_candidate_evidence.py" \
  --release-root "$RELEASE_ROOT" --repo "$ROOT_DIR" --app "$APP_PATH" --dmg "$DMG_PATH" \
  --version "$VERSION" --build "$BUILD" --team-id "$TEAM_ID"
# Final scan covers the candidate and all package-generated gate documents.
"$ROOT_DIR/script/scan_publication_secrets.sh" --repo "$ROOT_DIR" --artifact-root "$RELEASE_ROOT" --output "$RELEASE_ROOT/evidence/secret-scan.json"
"$ROOT_DIR/script/generate_release_metadata.sh" --phase checksums --output-root "$RELEASE_ROOT"
CANDIDATE_MANIFEST="$RELEASE_ROOT/evidence/notarized-candidate.json"
"$ROOT_DIR/script/verify_release_evidence.py" candidate --candidate "$CANDIDATE_MANIFEST" --repo "$ROOT_DIR"
printf 'PATCHWRIGHT_RELEASE_ROOT=%s\nPATCHWRIGHT_APP_PATH=%s\nPATCHWRIGHT_DMG_PATH=%s\n' "$RELEASE_ROOT" "$APP_PATH" "$DMG_PATH"
printf 'PATCHWRIGHT_CANDIDATE_MANIFEST=%s\nPATCHWRIGHT_ARTIFACT_SHA256=%s\nPATCHWRIGHT_STATUS=notarized-candidate\n' "$CANDIDATE_MANIFEST" "$ARTIFACT_SHA256"
