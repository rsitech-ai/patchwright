#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?release root required}"
for required in \
  assembly.json \
  build-metadata.json \
  sbom.spdx.json \
  third-party-notices.md \
  secret-scan.json \
  SHA256SUMS; do
  [[ -f "$ROOT/evidence/$required" && ! -L "$ROOT/evidence/$required" && -s "$ROOT/evidence/$required" ]] \
    || { echo "missing or invalid compliance evidence: $required" >&2; exit 65; }
done
jq -e '.spdxVersion == "SPDX-2.3" and .dataLicense == "CC0-1.0" and (.packages | type == "array") and (.files | type == "array")' \
  "$ROOT/evidence/sbom.spdx.json" >/dev/null \
  || { echo "invalid SPDX compliance evidence" >&2; exit 65; }
jq -e '.schema_version == 1 and .clean == true and .findings == []' \
  "$ROOT/evidence/secret-scan.json" >/dev/null \
  || { echo "publication secret scan is not clean" >&2; exit 65; }
grep -Fq '# Third-Party Notices' "$ROOT/evidence/third-party-notices.md" \
  || { echo "invalid third-party notice evidence" >&2; exit 65; }
SBOM_SHA256="$(shasum -a 256 "$ROOT/evidence/sbom.spdx.json" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$ROOT/evidence/third-party-notices.md" | awk '{print $1}')"
SECRET_SCAN_SHA256="$(shasum -a 256 "$ROOT/evidence/secret-scan.json" | awk '{print $1}')"
jq -e \
  --arg sbom_sha256 "$SBOM_SHA256" \
  --arg notices_sha256 "$NOTICES_SHA256" \
  --arg secret_scan_sha256 "$SECRET_SCAN_SHA256" \
  '.compliance.sbom_sha256 == $sbom_sha256 and .compliance.third_party_notices_sha256 == $notices_sha256 and .compliance.secret_scan_sha256 == $secret_scan_sha256' \
  "$ROOT/evidence/assembly.json" >/dev/null \
  || { echo "assembly metadata is not bound to compliance evidence" >&2; exit 65; }
for required in sbom.spdx.json third-party-notices.md secret-scan.json; do
  grep -Fq "evidence/$required" "$ROOT/evidence/SHA256SUMS" \
    || { echo "SHA256SUMS does not cover $required" >&2; exit 65; }
done
if rg -n --hidden -g '!SHA256SUMS' -e 'gh[op]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY|sk-[A-Za-z0-9]{20,}' "$ROOT"; then
  echo "credential-shaped material found in release root" >&2
  exit 65
fi
(
  cd "$ROOT"
  shasum -a 256 -c evidence/SHA256SUMS
)
echo "reproducibility bundle verified: $ROOT"
