#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${PATCHWRIGHT_VERSION:-0.1.0}"
BUILD="${PATCHWRIGHT_BUILD:-1}"
OUTPUT_PARENT="${PATCHWRIGHT_RELEASE_WORK_ROOT:-$HOME/.patchwright/release-work}"
ALLOW_DIRTY="${PATCHWRIGHT_ALLOW_DIRTY:-0}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9]+)*$ ]] || { echo "invalid PATCHWRIGHT_VERSION" >&2; exit 64; }
[[ "$BUILD" =~ ^[1-9][0-9]*$ ]] || { echo "invalid PATCHWRIGHT_BUILD" >&2; exit 64; }

DIRTY=false
[[ -z "$(git -C "$ROOT_DIR" status --porcelain)" ]] || DIRTY=true
if [[ "$DIRTY" == true && "$ALLOW_DIRTY" != 1 ]]; then
  echo "release build refused: working tree is dirty" >&2
  exit 65
fi

mkdir -p "$OUTPUT_PARENT"
WORK_ROOT="$(mktemp -d "$OUTPUT_PARENT/Patchwright-$VERSION-$BUILD.XXXXXX")"
APP_PATH="$WORK_ROOT/Patchwright.app"
mkdir -p "$APP_PATH/Contents/MacOS" "$APP_PATH/Contents/Helpers" "$APP_PATH/Contents/Resources" "$WORK_ROOT/reproducibility" "$WORK_ROOT/evidence"

cd "$ROOT_DIR"
swift build -c release -Xswiftc -warnings-as-errors
cargo build --locked --release -p patchwright-engine -p patchwright-relay
SWIFT_BIN="$(swift build -c release --show-bin-path)/Patchwright"
cp "$SWIFT_BIN" "$APP_PATH/Contents/MacOS/Patchwright"
cp "$ROOT_DIR/target/release/patchwright-engine" "$APP_PATH/Contents/Helpers/patchwright-engine"
cp "$ROOT_DIR/target/release/patchwright-relay" "$APP_PATH/Contents/Helpers/patchwright-relay"
cp "$ROOT_DIR/Packaging/Info.plist" "$APP_PATH/Contents/Info.plist"
cp "$ROOT_DIR/Packaging/PrivacyInfo.xcprivacy" "$APP_PATH/Contents/Resources/PrivacyInfo.xcprivacy"
cp "$ROOT_DIR/Packaging/THIRD_PARTY_NOTICES.md" "$APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP_PATH/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD" "$APP_PATH/Contents/Info.plist"
chmod 755 "$APP_PATH/Contents/MacOS/Patchwright" "$APP_PATH/Contents/Helpers/patchwright-engine" "$APP_PATH/Contents/Helpers/patchwright-relay"
/usr/bin/xattr -cr "$APP_PATH"

cp "$ROOT_DIR/Cargo.lock" "$ROOT_DIR/Cargo.toml" "$ROOT_DIR/Package.swift" "$WORK_ROOT/reproducibility/"
cp -R "$ROOT_DIR/Packaging" "$ROOT_DIR/script" "$WORK_ROOT/reproducibility/"
cp "$ROOT_DIR/README.md" "$ROOT_DIR/LICENSE" "$WORK_ROOT/reproducibility/" 2>/dev/null || true
git -C "$ROOT_DIR" archive --format=tar.gz --output="$WORK_ROOT/reproducibility/source.tar.gz" HEAD
cargo metadata --locked --format-version 1 >"$WORK_ROOT/reproducibility/cargo-metadata.json"
swift package show-dependencies --format json >"$WORK_ROOT/reproducibility/swift-dependencies.json"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT_DIR" show -s --format=%ct HEAD)}"
generate_compliance() {
  python3 "$ROOT_DIR/script/generate_release_compliance.py" \
    --cargo-metadata "$WORK_ROOT/reproducibility/cargo-metadata.json" \
    --swift-metadata "$WORK_ROOT/reproducibility/swift-dependencies.json" \
    --output-dir "$WORK_ROOT/evidence" \
    --app "$APP_PATH" \
    --engine "$APP_PATH/Contents/Helpers/patchwright-engine" \
    --relay "$APP_PATH/Contents/Helpers/patchwright-relay"
}
generate_compliance
cp "$WORK_ROOT/evidence/third-party-notices.md" "$APP_PATH/Contents/Resources/THIRD_PARTY_NOTICES.md"
# Regenerate once after embedding the deterministic notice so the app tree hash is exact.
generate_compliance

"$ROOT_DIR/script/validate_bundle.sh" "$APP_PATH"
"$ROOT_DIR/script/scan_publication_secrets.sh" \
  --repo "$ROOT_DIR" \
  --artifact-root "$WORK_ROOT" \
  --output "$WORK_ROOT/evidence/secret-scan.json"

SBOM_SHA256="$(shasum -a 256 "$WORK_ROOT/evidence/sbom.spdx.json" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$WORK_ROOT/evidence/third-party-notices.md" | awk '{print $1}')"
SECRET_SCAN_SHA256="$(shasum -a 256 "$WORK_ROOT/evidence/secret-scan.json" | awk '{print $1}')"

jq -n \
  --arg app "$APP_PATH" \
  --arg root "$WORK_ROOT" \
  --arg version "$VERSION" \
  --arg build "$BUILD" \
  --arg sbom_sha256 "$SBOM_SHA256" \
  --arg notices_sha256 "$NOTICES_SHA256" \
  --arg secret_scan_sha256 "$SECRET_SCAN_SHA256" \
  --argjson dirty "$DIRTY" \
  '{app_path:$app,release_root:$root,version:$version,build:$build,dirty:$dirty,candidate:($dirty|not),compliance:{sbom_sha256:$sbom_sha256,third_party_notices_sha256:$notices_sha256,secret_scan_sha256:$secret_scan_sha256}}' \
  >"$WORK_ROOT/evidence/assembly.json"
"$ROOT_DIR/script/generate_release_metadata.sh" "$APP_PATH" "$WORK_ROOT"

printf 'PATCHWRIGHT_RELEASE_ROOT=%s\nPATCHWRIGHT_APP_PATH=%s\n' "$WORK_ROOT" "$APP_PATH"
