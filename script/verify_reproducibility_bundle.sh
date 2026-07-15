#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?release root required}"
for required in \
  assembly.json \
  build-metadata.json \
  SYMLINKS.json \
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
jq -e '
  ([.excluded_artifacts[].reason] | sort) == ["checksum-manifest-circularity", "self-output"] and
  ([.excluded_artifacts[] | has("locator_sha256")] | all)
' "$ROOT/evidence/secret-scan.json" >/dev/null \
  || { echo "publication secret scan exclusions are invalid" >&2; exit 65; }
grep -Fq '# Third-Party Notices' "$ROOT/evidence/third-party-notices.md" \
  || { echo "invalid third-party notice evidence" >&2; exit 65; }
[[ -d "$ROOT/evidence/third-party-licenses" && ! -L "$ROOT/evidence/third-party-licenses" ]] \
  || { echo "missing third-party license texts" >&2; exit 65; }
find "$ROOT/evidence/third-party-licenses" -type l -print -quit | grep -q . \
  && { echo "third-party license texts must not be symlinks" >&2; exit 65; }
find "$ROOT/evidence/third-party-licenses" -type f -size +0c -print -quit | grep -q . \
  || { echo "third-party license texts are empty" >&2; exit 65; }
"$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/generate_symlink_manifest.py" \
  --root "$ROOT" \
  --verify "$ROOT/evidence/SYMLINKS.json"
SBOM_SHA256="$(shasum -a 256 "$ROOT/evidence/sbom.spdx.json" | awk '{print $1}')"
NOTICES_SHA256="$(shasum -a 256 "$ROOT/evidence/third-party-notices.md" | awk '{print $1}')"
jq -e \
  --arg sbom_sha256 "$SBOM_SHA256" \
  --arg notices_sha256 "$NOTICES_SHA256" \
  '.compliance.sbom_sha256 == $sbom_sha256 and .compliance.third_party_notices_sha256 == $notices_sha256 and .compliance.secret_scan_binding == "evidence/SHA256SUMS"' \
  "$ROOT/evidence/assembly.json" >/dev/null \
  || { echo "assembly metadata is not bound to compliance evidence" >&2; exit 65; }
for required in SYMLINKS.json sbom.spdx.json third-party-notices.md secret-scan.json; do
  grep -Fq "evidence/$required" "$ROOT/evidence/SHA256SUMS" \
    || { echo "SHA256SUMS does not cover $required" >&2; exit 65; }
done
if rg --quiet --hidden -g '!SHA256SUMS' -e 'gh[op]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|BEGIN (ENCRYPTED |RSA |EC |OPENSSH )?PRIVATE KEY|sk-[A-Za-z0-9]{20,}' "$ROOT"; then
  echo "credential-shaped material found in release root" >&2
  exit 65
fi
CURRENT_SUMS="$(mktemp "${TMPDIR:-/tmp}/patchwright-current-sums.XXXXXX")"
trap 'rm -f "$CURRENT_SUMS"' EXIT
(
  cd "$ROOT"
  find . -type f ! -path './evidence/SHA256SUMS' -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do shasum -a 256 "$file"; done
) >"$CURRENT_SUMS"
cmp -s "$ROOT/evidence/SHA256SUMS" "$CURRENT_SUMS" \
  || { echo "SHA256SUMS does not exactly match candidate files" >&2; exit 65; }
(
  cd "$ROOT"
  shasum -a 256 -c evidence/SHA256SUMS
)
echo "reproducibility bundle verified: $ROOT"
