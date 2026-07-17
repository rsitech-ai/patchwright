#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-candidate-contract.XXXXXX")"
TMP_ROOT="$(cd "$TMP_ROOT" && pwd -P)"
trap '/usr/bin/trash "$TMP_ROOT" >/dev/null 2>&1 || true' EXIT
REPO="$TMP_ROOT/repo"
RELEASE="$TMP_ROOT/release"
EVIDENCE="$RELEASE/evidence"
mkdir -p "$REPO" "$RELEASE/Patchwright.app/Contents/MacOS" "$RELEASE/Patchwright.app/Contents/Helpers" "$EVIDENCE" "$RELEASE/reproducibility"
mkdir -p "$EVIDENCE/third-party-licenses/Sparkle"
git -C "$REPO" init -q
git -C "$REPO" config user.name Fixture
git -C "$REPO" config user.email fixture@example.invalid
printf 'fixture\n' >"$REPO/README.md"
git -C "$REPO" add README.md
git -C "$REPO" commit -qm fixture
git -C "$REPO" tag v0.1.0
COMMIT="$(git -C "$REPO" rev-parse HEAD)"
git -C "$REPO" archive --format=tar.gz --output="$RELEASE/reproducibility/source.tar.gz" "$COMMIT"
SOURCE_DIGEST="$(shasum -a 256 "$RELEASE/reproducibility/source.tar.gz" | awk '{print $1}')"
printf 'signed notarized dmg fixture\n' >"$RELEASE/Patchwright-0.1.0.dmg"
printf 'signed app fixture\n' >"$RELEASE/Patchwright.app/Contents/MacOS/Patchwright"
printf 'signed engine fixture\n' >"$RELEASE/Patchwright.app/Contents/Helpers/patchwright-engine"
printf 'signed relay fixture\n' >"$RELEASE/Patchwright.app/Contents/Helpers/patchwright-relay"
DIGEST="$(shasum -a 256 "$RELEASE/Patchwright-0.1.0.dmg" | awk '{print $1}')"
printf '%s  Patchwright-0.1.0.dmg\n' "$DIGEST" >"$RELEASE/Patchwright-0.1.0.dmg.sha256"
printf '<rss/>\n' >"$RELEASE/appcast.xml"
printf '# Third-Party Notices\n' >"$EVIDENCE/third-party-notices.md"
printf 'MIT fixture\n' >"$EVIDENCE/third-party-licenses/Sparkle/LICENSE"
python3 - "$ROOT_DIR/script" "$RELEASE" "$EVIDENCE/sbom.spdx.json" <<'PY'
import json
from pathlib import Path
import sys

sys.path.insert(0, sys.argv[1])
from generate_candidate_evidence import component_digest

release = Path(sys.argv[2])
app = release / "Patchwright.app"
components = {
    "Patchwright.app": component_digest(app, "Patchwright.app"),
    "patchwright-engine": component_digest(app / "Contents/Helpers/patchwright-engine", "patchwright-engine"),
    "patchwright-relay": component_digest(app / "Contents/Helpers/patchwright-relay", "patchwright-relay"),
}
document = {
    "spdxVersion": "SPDX-2.3",
    "dataLicense": "CC0-1.0",
    "packages": [{"name": "Sparkle", "licenseDeclared": "MIT"}],
    "files": [
        {"fileName": name, "checksums": [{"algorithm": "SHA256", "checksumValue": digest}]}
        for name, digest in components.items()
    ],
}
Path(sys.argv[3]).write_text(json.dumps(document) + "\n", encoding="utf-8")
PY
jq -n '{dirty:false,candidate:true}' >"$EVIDENCE/assembly.json"
jq -n '{dirty:false}' >"$EVIDENCE/build-metadata.json"
jq -n '{schema_version:1,clean:true,findings:[]}' >"$EVIDENCE/secret-scan.json"
jq -n '{schema_version:1,links:[]}' >"$EVIDENCE/SYMLINKS.json"
jq -n '{schema_version:1,kind:"app",status:"Accepted",request_id:"app-id",stapled:true,stapler_validated:true,log_summary:{log_sha256:("a"*64),issue_count:0,error_count:0,warning_count:0,info_count:0,warning_policy:"reject"}}' >"$EVIDENCE/notary-app.json"
jq -n --arg digest "$DIGEST" '{schema_version:1,kind:"dmg",status:"Accepted",request_id:"dmg-id",stapled:true,stapler_validated:true,final_sha256:$digest,log_summary:{log_sha256:("b"*64),issue_count:0,error_count:0,warning_count:0,info_count:0,warning_policy:"reject"}}' >"$EVIDENCE/notary-dmg.json"
jq -n '{schema_version:1,status:"pass",checks:{dmg_signature:true,dmg_ticket:true,dmg_gatekeeper:true,app_signature:true,app_ticket:true,app_gatekeeper:true,bundle_layout:true,team_id:true,hardened_runtime:true,entitlements:true}}' >"$EVIDENCE/distribution.json"

"$ROOT_DIR/script/generate_candidate_evidence.py" \
  --release-root "$RELEASE" --repo "$REPO" --app "$RELEASE/Patchwright.app" \
  --dmg "$RELEASE/Patchwright-0.1.0.dmg" --version 0.1.0 --build 1 \
  --team-id ABCDE12345 --created-at 2026-07-16T10:00:00Z

jq -e --arg commit "$COMMIT" --arg digest "$DIGEST" '
  .kind == "patchwright.notarized-candidate" and
  .git_commit == $commit and .artifact_sha256 == $digest and
  .signing.team_id == "ABCDE12345" and
  .evidence.secret_scan_gate == "evidence/secret-scan-gate.json"
' "$EVIDENCE/notarized-candidate.json" >/dev/null
jq -e --arg source_digest "$SOURCE_DIGEST" '
  .source_archive_path == "reproducibility/source.tar.gz" and
  .source_archive_sha256 == $source_digest
' "$EVIDENCE/notarized-candidate.json" >/dev/null \
  || { echo "candidate evidence did not bind the source archive digest" >&2; exit 1; }
jq -e --arg source_digest "$SOURCE_DIGEST" '
  .dirty == false and .source_archive_sha256 == $source_digest
' "$EVIDENCE/build-metadata.json" >/dev/null \
  || { echo "build metadata did not bind clean state and source archive digest" >&2; exit 1; }
jq -e --arg commit "$COMMIT" --arg digest "$DIGEST" '
  .git_commit == $commit and .artifact_sha256 == $digest and
  .status == "pass" and ([.checks[]] | all)
' "$EVIDENCE/distribution.json" >/dev/null
SBOM_DIGEST="$(shasum -a 256 "$EVIDENCE/sbom.spdx.json" | awk '{print $1}')"
NOTICES_DIGEST="$(shasum -a 256 "$EVIDENCE/third-party-notices.md" | awk '{print $1}')"
jq -e --arg sbom "$SBOM_DIGEST" --arg notices "$NOTICES_DIGEST" '
  .compliance.sbom_sha256 == $sbom and
  .compliance.third_party_notices_sha256 == $notices and
  (.compliance.post_signing_components | keys | sort) == ["Patchwright.app", "patchwright-engine", "patchwright-relay"]
' "$EVIDENCE/assembly.json" >/dev/null
for gate in repo.json secret-scan-gate.json compliance-gate.json; do
  jq -e --arg digest "$DIGEST" --arg source_digest "$SOURCE_DIGEST" \
    '.status == "pass" and .artifact_sha256 == $digest and .source_archive_sha256 == $source_digest' \
    "$EVIDENCE/$gate" >/dev/null \
    || { echo "$gate did not bind the source archive digest" >&2; exit 1; }
done

jq '.dirty = true' "$EVIDENCE/build-metadata.json" >"$EVIDENCE/build-metadata.tmp"
mv "$EVIDENCE/build-metadata.tmp" "$EVIDENCE/build-metadata.json"
if "$ROOT_DIR/script/generate_candidate_evidence.py" \
  --release-root "$RELEASE" --repo "$REPO" --app "$RELEASE/Patchwright.app" \
  --dmg "$RELEASE/Patchwright-0.1.0.dmg" --version 0.1.0 --build 1 \
  --team-id ABCDE12345 --created-at 2026-07-16T10:00:00Z >"$TMP_ROOT/dirty-metadata.out" 2>&1; then
  echo "candidate evidence accepted dirty build metadata" >&2
  exit 1
fi
grep -Fq 'build metadata must record dirty=false' "$TMP_ROOT/dirty-metadata.out" \
  || { echo "dirty build metadata rejection was not explicit" >&2; exit 1; }
echo "Patchwright candidate evidence contract passed"
