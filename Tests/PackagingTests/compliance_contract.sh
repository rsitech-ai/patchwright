#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-compliance-contract.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  echo "compliance contract failed: $*" >&2
  exit 1
}

mkdir -p "$TMP_ROOT/components" "$TMP_ROOT/out-a" "$TMP_ROOT/out-b"
printf 'app fixture\n' >"$TMP_ROOT/components/Patchwright.app"
printf 'engine fixture\n' >"$TMP_ROOT/components/patchwright-engine"
printf 'relay fixture\n' >"$TMP_ROOT/components/patchwright-relay"

cat >"$TMP_ROOT/cargo.json" <<'JSON'
{
  "packages": [
    {"id":"zeta@2.0.0","name":"zeta","version":"2.0.0","license":"Apache-2.0","source":"registry+https://example.invalid/index","manifest_path":"/fixture/zeta/Cargo.toml"},
    {"id":"patchwright-core@0.1.0","name":"patchwright-core","version":"0.1.0","license":"MIT OR Apache-2.0","source":null,"manifest_path":"/fixture/patchwright-core/Cargo.toml"},
    {"id":"alpha@1.0.0","name":"alpha","version":"1.0.0","license":"MIT","source":"registry+https://example.invalid/index","manifest_path":"/fixture/alpha/Cargo.toml"}
  ],
  "resolve": {"nodes":[{"id":"zeta@2.0.0"},{"id":"patchwright-core@0.1.0"},{"id":"alpha@1.0.0"}]}
}
JSON

cat >"$TMP_ROOT/swift.json" <<'JSON'
{
  "identity":"patchwright",
  "name":"Patchwright",
  "url":"/fixture/Patchwright",
  "version":"unspecified",
  "path":"/fixture/Patchwright",
  "dependencies":[
    {"identity":"sparkle","name":"Sparkle","url":"https://github.com/sparkle-project/Sparkle.git","version":"2.9.2","path":"/fixture/Sparkle","dependencies":[]}
  ]
}
JSON

generate() {
  local output="$1"
  SOURCE_DATE_EPOCH=0 python3 "$ROOT_DIR/script/generate_release_compliance.py" \
    --cargo-metadata "$TMP_ROOT/cargo.json" \
    --swift-metadata "$TMP_ROOT/swift.json" \
    --output-dir "$output" \
    --app "$TMP_ROOT/components/Patchwright.app" \
    --engine "$TMP_ROOT/components/patchwright-engine" \
    --relay "$TMP_ROOT/components/patchwright-relay"
}

generate "$TMP_ROOT/out-a"
generate "$TMP_ROOT/out-b"
cmp "$TMP_ROOT/out-a/sbom.spdx.json" "$TMP_ROOT/out-b/sbom.spdx.json" \
  || fail "SBOM output is not deterministic"
cmp "$TMP_ROOT/out-a/third-party-notices.md" "$TMP_ROOT/out-b/third-party-notices.md" \
  || fail "third-party notices are not deterministic"

jq -e '
  .spdxVersion == "SPDX-2.3" and
  .dataLicense == "CC0-1.0" and
  .creationInfo.created == "1970-01-01T00:00:00Z" and
  ([.packages[].name] == ([.packages[].name] | sort)) and
  ([.packages[] | select(.name == "alpha" and .versionInfo == "1.0.0" and .licenseDeclared == "MIT")] | length == 1) and
  ([.packages[] | select(.name == "Sparkle" and .versionInfo == "2.9.2" and .licenseDeclared == "MIT")] | length == 1) and
  ([.files[].fileName] == ["Patchwright.app", "patchwright-engine", "patchwright-relay"])
' "$TMP_ROOT/out-a/sbom.spdx.json" >/dev/null || fail "SPDX identity, ordering, package, or component contract failed"

grep -Fq '| alpha | 1.0.0 | MIT | Rust |' "$TMP_ROOT/out-a/third-party-notices.md" \
  || fail "Rust dependency notice is missing"
grep -Fq '| Sparkle | 2.9.2 | MIT | Swift |' "$TMP_ROOT/out-a/third-party-notices.md" \
  || fail "Swift dependency notice is missing"
if grep -Fq '| patchwright-core |' "$TMP_ROOT/out-a/third-party-notices.md"; then
  fail "first-party workspace package was included as a third-party notice"
fi

jq '.packages[0].license = null' "$TMP_ROOT/cargo.json" >"$TMP_ROOT/cargo-missing-license.json"
if SOURCE_DATE_EPOCH=0 python3 "$ROOT_DIR/script/generate_release_compliance.py" \
  --cargo-metadata "$TMP_ROOT/cargo-missing-license.json" \
  --swift-metadata "$TMP_ROOT/swift.json" \
  --output-dir "$TMP_ROOT/bad-license" \
  --app "$TMP_ROOT/components/Patchwright.app" \
  --engine "$TMP_ROOT/components/patchwright-engine" \
  --relay "$TMP_ROOT/components/patchwright-relay" >"$TMP_ROOT/bad-license.out" 2>&1; then
  fail "dependency without a declared license was accepted"
fi

printf '{malformed' >"$TMP_ROOT/malformed.json"
if SOURCE_DATE_EPOCH=0 python3 "$ROOT_DIR/script/generate_release_compliance.py" \
  --cargo-metadata "$TMP_ROOT/malformed.json" \
  --swift-metadata "$TMP_ROOT/swift.json" \
  --output-dir "$TMP_ROOT/malformed-out" \
  --app "$TMP_ROOT/components/Patchwright.app" \
  --engine "$TMP_ROOT/components/patchwright-engine" \
  --relay "$TMP_ROOT/components/patchwright-relay" >"$TMP_ROOT/malformed.out" 2>&1; then
  fail "malformed metadata was accepted"
fi

mkdir -p "$TMP_ROOT/repo" "$TMP_ROOT/artifacts"
git -C "$TMP_ROOT/repo" init -q
git -C "$TMP_ROOT/repo" config user.email fixture@example.invalid
git -C "$TMP_ROOT/repo" config user.name Fixture
printf 'safe fixture\n' >"$TMP_ROOT/repo/README.md"
git -C "$TMP_ROOT/repo" add README.md
git -C "$TMP_ROOT/repo" commit -qm safe

"$ROOT_DIR/script/scan_publication_secrets.sh" \
  --repo "$TMP_ROOT/repo" \
  --artifact-root "$TMP_ROOT/artifacts" \
  --output "$TMP_ROOT/clean-scan.json"
jq -e '.schema_version == 1 and .clean == true and .findings == [] and .scanned.history_blobs >= 1' \
  "$TMP_ROOT/clean-scan.json" >/dev/null || fail "clean scan JSON is invalid"

printf 'github_pat_%s\n' 'abcdefghijklmnopqrstuvwxyz1234567890' >"$TMP_ROOT/artifacts/token.txt"
{
  printf '%s%s\n' '-----BEGIN ' 'PRIVATE KEY-----'
  printf '%s\n' 'YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXphYmNkZWZnaGlqa2xtbm9wcXJzdHV2d3h5eg=='
  printf '%s%s\n' '-----END ' 'PRIVATE KEY-----'
} >"$TMP_ROOT/artifacts/key.pem"
if "$ROOT_DIR/script/scan_publication_secrets.sh" \
  --repo "$TMP_ROOT/repo" \
  --artifact-root "$TMP_ROOT/artifacts" \
  --output "$TMP_ROOT/findings.json" >"$TMP_ROOT/findings.out" 2>&1; then
  fail "credential fixtures were accepted"
fi
jq -e '
  .clean == false and
  ([.findings[].rule] | index("github-personal-access-token") != null) and
  ([.findings[].rule] | index("pem-private-key") != null) and
  ([.findings[] | has("locator_sha256") and (has("path") | not) and (has("object_id") | not)] | all)
' "$TMP_ROOT/findings.json" >/dev/null || fail "secret findings were absent or insufficiently redacted"
if grep -Fq 'abcdefghijklmnopqrstuvwxyz1234567890' "$TMP_ROOT/findings.json"; then
  fail "secret value leaked into scan evidence"
fi
if grep -Fq 'token.txt' "$TMP_ROOT/findings.json"; then
  fail "artifact path leaked into scan evidence"
fi

grep -Fq 'generate_release_compliance.py' "$ROOT_DIR/script/build_release_components.sh" \
  || fail "release assembly does not generate compliance evidence"
grep -Fq 'scan_publication_secrets.sh' "$ROOT_DIR/script/build_release_components.sh" \
  || fail "release assembly does not generate secret-scan evidence"
grep -Fq 'third-party-notices.md' "$ROOT_DIR/script/build_release_components.sh" \
  || fail "release assembly does not embed dependency-derived notices"

VERIFY_ROOT="$TMP_ROOT/verify-root"
mkdir -p "$VERIFY_ROOT/evidence"
cp "$TMP_ROOT/out-a/sbom.spdx.json" "$VERIFY_ROOT/evidence/sbom.spdx.json"
cp "$TMP_ROOT/out-a/third-party-notices.md" "$VERIFY_ROOT/evidence/third-party-notices.md"
cp "$TMP_ROOT/clean-scan.json" "$VERIFY_ROOT/evidence/secret-scan.json"
printf '{"version":"0.1.0"}\n' >"$VERIFY_ROOT/evidence/build-metadata.json"
SBOM_SHA256="$(shasum -a 256 "$VERIFY_ROOT/evidence/sbom.spdx.json" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$VERIFY_ROOT/evidence/third-party-notices.md" | awk '{print $1}')"
SECRET_SCAN_SHA256="$(shasum -a 256 "$VERIFY_ROOT/evidence/secret-scan.json" | awk '{print $1}')"
jq -n \
  --arg sbom_sha256 "$SBOM_SHA256" \
  --arg notices_sha256 "$NOTICES_SHA256" \
  --arg secret_scan_sha256 "$SECRET_SCAN_SHA256" \
  '{compliance:{sbom_sha256:$sbom_sha256,third_party_notices_sha256:$notices_sha256,secret_scan_sha256:$secret_scan_sha256}}' \
  >"$VERIFY_ROOT/evidence/assembly.json"
(
  cd "$VERIFY_ROOT"
  find . -type f ! -path './evidence/SHA256SUMS' -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do shasum -a 256 "$file"; done
) >"$VERIFY_ROOT/evidence/SHA256SUMS"
"$ROOT_DIR/script/verify_reproducibility_bundle.sh" "$VERIFY_ROOT" >/dev/null

jq '.clean = false | .findings = [{"scope":"artifact","locator_sha256":"redacted","rule":"fixture"}]' \
  "$VERIFY_ROOT/evidence/secret-scan.json" >"$VERIFY_ROOT/evidence/secret-scan.tmp"
mv "$VERIFY_ROOT/evidence/secret-scan.tmp" "$VERIFY_ROOT/evidence/secret-scan.json"
(
  cd "$VERIFY_ROOT"
  find . -type f ! -path './evidence/SHA256SUMS' -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do shasum -a 256 "$file"; done
) >"$VERIFY_ROOT/evidence/SHA256SUMS"
if "$ROOT_DIR/script/verify_reproducibility_bundle.sh" "$VERIFY_ROOT" >"$TMP_ROOT/verify-findings.out" 2>&1; then
  fail "reproducibility verification accepted secret findings"
fi

echo "Patchwright compliance contract passed"
