#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-promotion-contract.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  echo "promotion contract failed: $*" >&2
  exit 1
}

[[ -x "$ROOT_DIR/script/verify_release_evidence.py" ]] \
  || fail "missing executable script/verify_release_evidence.py"
[[ -x "$ROOT_DIR/script/promote_release.sh" ]] \
  || fail "missing executable script/promote_release.sh"

if PATCHWRIGHT_REPO_VERIFIED=1 \
  PATCHWRIGHT_CODEX_VERIFIED=1 \
  PATCHWRIGHT_GITHUB_VERIFIED=1 \
  PATCHWRIGHT_CLEAN_MACHINE_VERIFIED=1 \
  "$ROOT_DIR/script/release_readiness.sh" --app missing.app --json "$TMP_ROOT/readiness.json" \
    >"$TMP_ROOT/legacy.out" 2>&1; then
  fail "legacy environment booleans and --app evidence were accepted"
fi
grep -Fq 'legacy release evidence is unsupported' "$TMP_ROOT/legacy.out" \
  || fail "legacy evidence rejection was not explicit"

python3 - "$ROOT_DIR" "$TMP_ROOT" <<'PY'
from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import plistlib
import stat
import subprocess
import sys


root = Path(sys.argv[1])
temporary = Path(sys.argv[2])
verifier = root / "script" / "verify_release_evidence.py"
now = "2026-07-15T13:00:00Z"
created = "2026-07-15T12:00:00Z"
completed = "2026-07-15T12:10:00Z"
DMG_BYTES = b"Patchwright signed notarized fixture DMG\n"


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def component_digest(path: Path) -> str:
    digest = hashlib.sha256()
    if path.is_file():
        return sha(path)
    for entry in sorted(path.rglob("*"), key=lambda item: item.relative_to(path).as_posix()):
        relative = entry.relative_to(path).as_posix().encode("utf-8")
        mode = stat.S_IMODE(entry.lstat().st_mode)
        if entry.is_symlink():
            kind, payload = b"L", os.readlink(entry).encode("utf-8")
        elif entry.is_file():
            kind, payload = b"F", hashlib.sha256(entry.read_bytes()).digest()
        elif entry.is_dir():
            kind, payload = b"D", b""
        else:
            raise SystemExit(f"unsupported fixture component entry: {entry}")
        digest.update(kind + b"\0" + relative + b"\0" + str(mode).encode("ascii") + b"\0" + payload)
    return digest.hexdigest()


def initialize_repo(path: Path) -> str:
    path.mkdir(parents=True)
    subprocess.run(["git", "-C", str(path), "init", "-q"], check=True)
    subprocess.run(["git", "-C", str(path), "config", "user.name", "Fixture"], check=True)
    subprocess.run(["git", "-C", str(path), "config", "user.email", "fixture@example.invalid"], check=True)
    (path / "README.md").write_text("fixture\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(path), "add", "README.md"], check=True)
    environment = dict(os.environ, GIT_AUTHOR_DATE="2026-07-15T11:00:00Z", GIT_COMMITTER_DATE="2026-07-15T11:00:00Z")
    subprocess.run(["git", "-C", str(path), "commit", "-qm", "fixture"], check=True, env=environment)
    commit = subprocess.check_output(["git", "-C", str(path), "rev-parse", "HEAD"], text=True).strip()
    subprocess.run(["git", "-C", str(path), "tag", "v0.1.0"], check=True)
    return commit


def identity(commit: str) -> dict[str, object]:
    return {
        "artifact_filename": "Patchwright-0.1.0.dmg",
        "artifact_sha256": "",
        "git_commit": commit,
        "tag": "v0.1.0",
        "version": "0.1.0",
        "build": "1",
    }


def gate(name: str, commit: str, digest: str, source_digest: str, checks: list[str]) -> dict[str, object]:
    value = identity(commit)
    value["artifact_sha256"] = digest
    value["source_archive_path"] = "reproducibility/source.tar.gz"
    value["source_archive_sha256"] = source_digest
    value.update({
        "schema_version": 1,
        "gate": name,
        "status": "pass",
        "completed_at": completed,
        "checks": {check: True for check in checks},
    })
    return value


def appcast_content(archive_signature: str) -> str:
    return (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" version="2.0">'
        '<channel><item><title>Patchwright 0.1.0</title>'
        '<sparkle:version>1</sparkle:version>'
        '<sparkle:shortVersionString>0.1.0</sparkle:shortVersionString>'
        '<enclosure url="https://github.com/rsitech-ai/patchwright/releases/download/v0.1.0/Patchwright-0.1.0.dmg" '
        f'length="{len(DMG_BYTES)}" type="application/octet-stream" '
        f'sparkle:edSignature="{archive_signature}"/>'
        '</item></channel></rss>\n'
    )


signature_root = temporary / "signature-fixture"
signature_root.mkdir()
signature_dmg = signature_root / "Patchwright-0.1.0.dmg"
signature_template = signature_root / "appcast-template.xml"
signature_dmg.write_bytes(DMG_BYTES)
signature_template.write_text(appcast_content("__ARCHIVE_SIGNATURE__"), encoding="utf-8")
swift_environment = os.environ.copy()
swift_environment.pop("SDKROOT", None)
signature_result = subprocess.run(
    ["swift", str(root / "Tests/PackagingTests/generate_ed25519_fixture.swift"), str(signature_dmg), str(signature_template)],
    check=True, env=swift_environment, stdout=subprocess.PIPE, text=True,
)
SIGNATURES = json.loads(signature_result.stdout)
FIXTURE_APPCAST_CONTENT = appcast_content(SIGNATURES["archive_signature"])
FIXTURE_APPCAST = (
    FIXTURE_APPCAST_CONTENT
    + f'<!-- sparkle-signatures:\nedSignature: {SIGNATURES["feed_signature"]}\nlength: {len(FIXTURE_APPCAST_CONTENT.encode("utf-8"))}\n-->\n'
)


def create_fixture(base: Path) -> tuple[Path, Path, Path]:
    repo = base / "repo"
    release = base / "candidate"
    evidence = release / "evidence"
    external = base / "external"
    commit = initialize_repo(repo)
    evidence.mkdir(parents=True)
    external.mkdir()
    reproducibility = release / "reproducibility"
    reproducibility.mkdir()
    source_archive = reproducibility / "source.tar.gz"
    with source_archive.open("wb") as handle:
        subprocess.run(["git", "-C", str(repo), "archive", "--format=tar.gz", commit], check=True, stdout=handle)
    source_digest = sha(source_archive)

    app = release / "Patchwright.app"
    engine = app / "Contents" / "Helpers" / "patchwright-engine"
    relay = app / "Contents" / "Helpers" / "patchwright-relay"
    engine.parent.mkdir(parents=True)
    (app / "Contents" / "MacOS").mkdir()
    (app / "Contents" / "MacOS" / "Patchwright").write_bytes(b"signed app executable\n")
    engine.write_bytes(b"signed engine\n")
    relay.write_bytes(b"signed relay\n")
    with (app / "Contents" / "Info.plist").open("wb") as handle:
        plistlib.dump({
            "CFBundleIdentifier": "ai.patchwright.app",
            "SUPublicEDKey": SIGNATURES["public_key"],
            "SUVerifyUpdateBeforeExtraction": True,
            "SURequireSignedFeed": True,
        }, handle)

    dmg = release / "Patchwright-0.1.0.dmg"
    dmg.write_bytes(DMG_BYTES)
    digest = sha(dmg)
    (release / "Patchwright-0.1.0.dmg.sha256").write_text(
        f"{digest}  Patchwright-0.1.0.dmg\n", encoding="utf-8"
    )
    (release / "appcast.xml").write_text(FIXTURE_APPCAST, encoding="utf-8")
    component_hashes = {
        "Patchwright.app": component_digest(app),
        "patchwright-engine": component_digest(engine),
        "patchwright-relay": component_digest(relay),
    }
    write_json(evidence / "sbom.spdx.json", {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "packages": [{"name": "Sparkle", "versionInfo": "2.9.2", "licenseDeclared": "MIT"}],
        "files": [
            {"fileName": name, "checksums": [{"algorithm": "SHA256", "checksumValue": value}]}
            for name, value in component_hashes.items()
        ],
    })
    (evidence / "third-party-notices.md").write_text("# Third-Party Notices\n\nSparkle 2.9.2 — MIT\n", encoding="utf-8")
    (evidence / "third-party-licenses").mkdir()
    (evidence / "third-party-licenses" / "Sparkle-LICENSE").write_text("MIT fixture\n", encoding="utf-8")

    common = identity(commit)
    common["artifact_sha256"] = digest
    common["source_archive_path"] = "reproducibility/source.tar.gz"
    common["source_archive_sha256"] = source_digest
    write_json(evidence / "assembly.json", {
        "schema_version": 1, **common, "dirty": False, "candidate": True,
        "compliance": {
            "sbom_sha256": sha(evidence / "sbom.spdx.json"),
            "third_party_notices_sha256": sha(evidence / "third-party-notices.md"),
            "post_signing_components": component_hashes,
        },
    })
    write_json(evidence / "build-metadata.json", {
        "schema_version": 1, **common, "bundle_identifier": "ai.patchwright.app",
        "team_id": "ABCDE12345", "architecture": "arm64", "minimum_macos": "26.0",
        "source_date_epoch": 1752577200, "dirty": False,
    })
    write_json(evidence / "SYMLINKS.json", {"schema_version": 1, "links": []})
    write_json(evidence / "secret-scan.json", {
        "schema_version": 1, "clean": True,
        "scanned": {"tracked_files": 1, "history_blobs": 1, "artifact_files": 12},
        "excluded_artifacts": [
            {"reason": "checksum-manifest-circularity", "locator_sha256": "4" * 64},
            {"reason": "self-output", "locator_sha256": "5" * 64},
        ],
        "findings": [],
    })
    write_json(evidence / "notary-app.json", {
        "schema_version": 1, "kind": "app", "submission_sha256": "6" * 64,
        "status": "Accepted", "request_id": "11111111-1111-1111-1111-111111111111",
        "stapled": True, "stapler_validated": True, "completed_at": completed,
        "log_summary": {"log_sha256": "8" * 64, "issue_count": 0, "error_count": 0,
                        "warning_count": 0, "info_count": 0, "warning_policy": "reject"},
    })
    write_json(evidence / "notary-dmg.json", {
        "schema_version": 1, "kind": "dmg", "submission_sha256": "7" * 64,
        "final_sha256": digest, "status": "Accepted",
        "request_id": "22222222-2222-2222-2222-222222222222",
        "stapled": True, "stapler_validated": True, "completed_at": completed,
        "log_summary": {"log_sha256": "9" * 64, "issue_count": 0, "error_count": 0,
                        "warning_count": 0, "info_count": 0, "warning_policy": "reject"},
    })
    distribution_checks = [
        "dmg_signature", "dmg_ticket", "dmg_gatekeeper", "app_signature", "app_ticket",
        "app_gatekeeper", "bundle_layout", "team_id", "hardened_runtime", "entitlements",
    ]
    write_json(evidence / "distribution.json", {
        "schema_version": 1, **common, "status": "pass",
        "checks": {name: True for name in distribution_checks},
    })

    gate_checks = {
        "repo": ["source_verify", "clean_source", "tag_binding"],
        "secret_scan": ["tracked", "all_refs", "candidate_root", "no_findings"],
        "compliance": ["spdx_2_3", "dependency_licenses", "post_signing_component_hashes"],
        "codex": ["signed_in_runtime", "task_start", "resume", "approval", "cancel"],
        "github": ["authorized_sandbox", "app_identity", "delivery", "exact_sha_approval", "merge", "kill_switch"],
        "clean_machine": ["checksum", "dmg_signature", "dmg_ticket", "dmg_gatekeeper", "app_signature", "app_ticket", "app_gatekeeper", "first_launch", "relaunch"],
    }
    write_json(evidence / "repo.json", gate("repo", commit, digest, source_digest, gate_checks["repo"]))
    write_json(evidence / "secret-scan-gate.json", gate("secret_scan", commit, digest, source_digest, gate_checks["secret_scan"]))
    write_json(evidence / "compliance-gate.json", gate("compliance", commit, digest, source_digest, gate_checks["compliance"]))
    codex = gate("codex", commit, digest, source_digest, gate_checks["codex"])
    github = gate("github", commit, digest, source_digest, gate_checks["github"])
    clean_checks = [
        "checksum", "dmg_signature", "dmg_ticket", "dmg_gatekeeper", "app_signature",
        "app_ticket", "app_gatekeeper", "first_launch", "relaunch",
        "missing_integration_guidance", "codex_thread_resume", "github_ingestion_without_gh",
        "offline_state", "expired_token_state", "revoked_installation_state",
        "missing_permission_state", "approval_delivery", "stale_head_rejection",
        "exact_sha_merge", "queue_advancement", "migration", "update_relaunch",
        "uninstall_data_retention", "explicit_data_removal",
    ]
    clean_evidence: dict[str, object] = {}
    for name in clean_checks:
        check_path = external / "clean-machine-checks" / f"{name}.txt"
        check_path.parent.mkdir(exist_ok=True)
        check_path.write_text(f"{name}: pass\n", encoding="utf-8")
        clean_evidence[name] = {
            "path": f"clean-machine-checks/{name}.txt",
            "sha256": sha(check_path),
        }
    manifest = {
        "schema_version": 1,
        "kind": "patchwright.clean-machine-evidence-manifest",
        "checks": clean_evidence,
    }
    manifest_path = external / "clean-machine-evidence-manifest.json"
    write_json(manifest_path, manifest)
    clean_machine = gate("clean_machine", commit, digest, source_digest, clean_checks)
    clean_machine["schema_version"] = 2
    clean_machine["reviewer"] = {
        "name": "Independent Fixture Reviewer",
        "identity": "reviewer@example.invalid",
        "independent": True,
    }
    clean_machine["checks"] = {
        name: {"status": "pass", "evidence": evidence}
        for name, evidence in clean_evidence.items()
    }
    clean_machine["evidence_manifest"] = {
        "path": manifest_path.name,
        "sha256": sha(manifest_path),
    }
    clean_machine["guest"] = {
        "product_version": "26.5", "build_version": "25F71", "architecture": "arm64",
        "gatekeeper_enabled": True, "source": "apple-ipsw:26.5:25F71:fixture-image-sha256",
    }
    write_json(external / "codex.json", codex)
    write_json(external / "github.json", github)
    write_json(external / "clean-machine.json", clean_machine)

    asset_paths = [
        "Patchwright-0.1.0.dmg", "Patchwright-0.1.0.dmg.sha256", "appcast.xml",
        "evidence/sbom.spdx.json", "evidence/third-party-notices.md",
    ]
    candidate = {
        "schema_version": 1,
        "kind": "patchwright.notarized-candidate",
        "product": "Patchwright",
        "artifact_filename": "Patchwright-0.1.0.dmg",
        "artifact_path": "Patchwright-0.1.0.dmg",
        "artifact_sha256": digest,
        "artifact_size": dmg.stat().st_size,
        "git_commit": commit,
        "tag": "v0.1.0",
        "version": "0.1.0",
        "build": "1",
        "source_archive_path": "reproducibility/source.tar.gz",
        "source_archive_sha256": source_digest,
        "bundle_identifier": "ai.patchwright.app",
        "created_at": created,
        "signing": {
            "identity_class": "Developer ID Application", "team_id": "ABCDE12345",
            "hardened_runtime": True, "secure_timestamp": True,
        },
        "notarization": {
            "app": {"status": "Accepted", "request_id": "11111111-1111-1111-1111-111111111111", "stapled": True},
            "dmg": {"status": "Accepted", "request_id": "22222222-2222-2222-2222-222222222222", "stapled": True},
        },
        "gatekeeper": {"app": True, "dmg": True},
        "assets": [
            {"name": Path(relative).name, "path": relative, "sha256": sha(release / relative), "size": (release / relative).stat().st_size}
            for relative in asset_paths
        ],
        "evidence": {
            "assembly": "evidence/assembly.json",
            "build_metadata": "evidence/build-metadata.json",
            "checksums": "evidence/SHA256SUMS",
            "symlinks": "evidence/SYMLINKS.json",
            "secret_scan": "evidence/secret-scan.json",
            "repo_gate": "evidence/repo.json",
            "secret_scan_gate": "evidence/secret-scan-gate.json",
            "compliance_gate": "evidence/compliance-gate.json",
            "notary_app": "evidence/notary-app.json",
            "notary_dmg": "evidence/notary-dmg.json",
            "distribution": "evidence/distribution.json",
        },
    }
    write_json(evidence / "notarized-candidate.json", candidate)
    entries = []
    for path in sorted(release.rglob("*"), key=lambda item: item.relative_to(release).as_posix()):
        if path.is_file() and path != evidence / "SHA256SUMS":
            entries.append(f"{sha(path)}  {path.relative_to(release).as_posix()}")
    (evidence / "SHA256SUMS").write_text("\n".join(entries) + "\n", encoding="utf-8")
    return repo, evidence / "notarized-candidate.json", external


repo, candidate, external = create_fixture(temporary / "happy")
output = temporary / "promotion"
command = [
    sys.executable, str(verifier), "promotion",
    "--candidate", str(candidate), "--repo", str(repo),
    "--codex", str(external / "codex.json"),
    "--github", str(external / "github.json"),
    "--clean-machine", str(external / "clean-machine.json"),
    "--output-dir", str(output), "--now", now,
]
result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
if result.returncode != 0:
    raise SystemExit(f"promotion happy path failed: {result.stderr}")
if "PATCHWRIGHT_STATUS=promoted-release" not in result.stdout:
    raise SystemExit("promotion happy path did not emit promoted-release status")
for name in ("release-evidence.json", "release-assets.json", "promotion-manifest.json", "promotion-readiness.json"):
    if not (output / name).is_file():
        raise SystemExit(f"promotion output missing: {name}")
readiness = json.loads((output / "promotion-readiness.json").read_text(encoding="utf-8"))
if readiness.get("ready") is not True:
    raise SystemExit("promotion readiness did not report ready=true")
promotion_manifest = json.loads((output / "promotion-manifest.json").read_text(encoding="utf-8"))
if (promotion_manifest.get("candidate_sha256") != sha(candidate)
        or promotion_manifest.get("release_assets_sha256") != sha(output / "release-assets.json")
        or promotion_manifest.get("release_evidence_sha256") != sha(output / "release-evidence.json")
        or readiness.get("promotion_manifest_sha256") != sha(output / "promotion-manifest.json")):
    raise SystemExit("promotion manifest did not bind candidate and publication outputs")


def promotion_command(repo: Path, candidate: Path, external: Path, output: Path) -> list[str]:
    return [
        sys.executable, str(verifier), "promotion",
        "--candidate", str(candidate), "--repo", str(repo),
        "--codex", str(external / "codex.json"),
        "--github", str(external / "github.json"),
        "--clean-machine", str(external / "clean-machine.json"),
        "--output-dir", str(output), "--now", now,
    ]


def refreeze(candidate: Path) -> None:
    release = candidate.parent.parent
    checksum = release / "evidence" / "SHA256SUMS"
    entries = []
    for path in sorted(release.rglob("*"), key=lambda item: item.relative_to(release).as_posix()):
        if path.is_file() and not path.is_symlink() and path != checksum:
            entries.append(f"{sha(path)}  {path.relative_to(release).as_posix()}")
    checksum.write_text("\n".join(entries) + "\n", encoding="utf-8")


def require_rejected(name: str, mutate) -> None:
    base = temporary / f"reject-{name}"
    case_repo, case_candidate, case_external = create_fixture(base)
    case_output = base / "promotion"
    command = promotion_command(case_repo, case_candidate, case_external, case_output)
    mutate(case_repo, case_candidate, case_external, case_output, command)
    # ponytail: ceiling=must exceed verify_ed25519.swift cold-start (inner timeout=30); raise if Swift helper stays cold-path slow
    rejected = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=60)
    if rejected.returncode == 0:
        raise SystemExit(f"promotion unexpectedly accepted {name}")
    if "release evidence rejected:" not in rejected.stderr and "usage:" not in rejected.stderr:
        raise SystemExit(f"promotion rejection for {name} was not explicit: {rejected.stderr}")


require_rejected("missing-candidate", lambda _r, candidate, _e, _o, command: command.__setitem__(command.index(str(candidate)), str(candidate.with_name("missing.json"))))
require_rejected("malformed-candidate", lambda _r, candidate, _e, _o, _c: candidate.write_text("{broken\n", encoding="utf-8"))


def duplicate_candidate(_repo, candidate, _external, _output, _command):
    candidate.write_text('{"schema_version":1,"schema_version":1}\n', encoding="utf-8")


require_rejected("duplicate-json-key", duplicate_candidate)


def malformed_signing_team_id(_repo, candidate, _external, _output, _command):
    value = json.loads(candidate.read_text(encoding="utf-8"))
    value["signing"]["team_id"] = "NOT-A-TEAM-ID"
    write_json(candidate, value)
    refreeze(candidate)


require_rejected("malformed-signing-team-id", malformed_signing_team_id)


def symlink_candidate(_repo, candidate, _external, _output, _command):
    target = candidate.with_name("candidate-target.json")
    candidate.rename(target)
    candidate.symlink_to(target.name)


require_rejected("symlink-candidate", symlink_candidate)


def fifo_candidate(_repo, candidate, _external, _output, _command):
    candidate.unlink()
    os.mkfifo(candidate)


require_rejected("fifo-candidate", fifo_candidate)


def artifact_digest_mismatch(_repo, candidate, _external, _output, _command):
    value = json.loads(candidate.read_text(encoding="utf-8"))
    artifact = candidate.parent.parent / value["artifact_path"]
    artifact.write_bytes(artifact.read_bytes() + b"tampered")


require_rejected("artifact-digest-mismatch", artifact_digest_mismatch)


def dirty_build_metadata(_repo, candidate, _external, _output, _command):
    path = candidate.parent / "build-metadata.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value["dirty"] = True
    write_json(path, value)
    refreeze(candidate)


require_rejected("dirty-build-metadata", dirty_build_metadata)


def dirty_release_repo(repo, _candidate, _external, _output, _command):
    (repo / "README.md").write_text("dirty fixture\n", encoding="utf-8")


require_rejected("dirty-release-repository", dirty_release_repo)


def source_archive_digest_mismatch(_repo, candidate, _external, _output, _command):
    archive = candidate.parent.parent / "reproducibility/source.tar.gz"
    archive.write_bytes(archive.read_bytes() + b"tamper\n")
    refreeze(candidate)


require_rejected("source-archive-digest-mismatch", source_archive_digest_mismatch)


def gate_digest_mismatch(_repo, _candidate, external, _output, _command):
    gate_path = external / "github.json"
    value = json.loads(gate_path.read_text(encoding="utf-8"))
    value["artifact_sha256"] = "f" * 64
    write_json(gate_path, value)


require_rejected("gate-digest-mismatch", gate_digest_mismatch)


def false_gate_check(_repo, _candidate, external, _output, _command):
    gate_path = external / "codex.json"
    value = json.loads(gate_path.read_text(encoding="utf-8"))
    value["checks"]["cancel"] = False
    write_json(gate_path, value)


require_rejected("false-required-check", false_gate_check)


def stale_gate(_repo, _candidate, external, _output, _command):
    gate_path = external / "clean-machine.json"
    value = json.loads(gate_path.read_text(encoding="utf-8"))
    value["completed_at"] = "2026-07-01T00:00:00Z"
    write_json(gate_path, value)


require_rejected("stale-gate", stale_gate)


def missing_clean_reviewer(_repo, _candidate, external, _output, _command):
    path = external / "clean-machine.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value.pop("reviewer")
    write_json(path, value)


require_rejected("missing-clean-machine-reviewer", missing_clean_reviewer)


def tampered_clean_check(_repo, _candidate, external, _output, _command):
    (external / "clean-machine-checks/first_launch.txt").write_text("tampered\n", encoding="utf-8")


require_rejected("tampered-clean-machine-check", tampered_clean_check)


def tampered_clean_manifest(_repo, _candidate, external, _output, _command):
    path = external / "clean-machine-evidence-manifest.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value["checks"].pop("relaunch")
    write_json(path, value)


require_rejected("tampered-clean-machine-manifest", tampered_clean_manifest)


def escaping_clean_evidence(_repo, _candidate, external, _output, _command):
    path = external / "clean-machine.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value["checks"]["first_launch"]["evidence"]["path"] = "../outside.txt"
    write_json(path, value)


require_rejected("escaping-clean-machine-evidence", escaping_clean_evidence)


def symlink_gate(_repo, _candidate, external, _output, _command):
    gate_path = external / "github.json"
    target = external / "github-target.json"
    gate_path.rename(target)
    gate_path.symlink_to(target.name)


require_rejected("symlink-gate", symlink_gate)


def nonempty_output(_repo, _candidate, _external, output, _command):
    output.mkdir()
    (output / "existing").write_text("occupied\n", encoding="utf-8")


require_rejected("nonempty-output", nonempty_output)


def nested_output(_repo, candidate, _external, _output, command):
    nested = candidate.parent.parent / "promotion"
    command[command.index("--output-dir") + 1] = str(nested)


require_rejected("candidate-nested-output", nested_output)


def decoy_public_dmg(_repo, candidate, _external, _output, _command):
    release = candidate.parent.parent
    (release / "decoy.bin").write_bytes(b"decoy\n")
    value = json.loads(candidate.read_text(encoding="utf-8"))
    for asset in value["assets"]:
        if asset["name"] == value["artifact_filename"]:
            asset["path"] = "decoy.bin"
            asset["sha256"] = sha(release / "decoy.bin")
            asset["size"] = (release / "decoy.bin").stat().st_size
    write_json(candidate, value)
    refreeze(candidate)


require_rejected("decoy-public-dmg", decoy_public_dmg)


def rejected_notary(_repo, candidate, _external, _output, _command):
    path = candidate.parent / "notary-dmg.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value.update({"status": "Rejected", "stapled": False, "stapler_validated": False})
    write_json(path, value)
    refreeze(candidate)


require_rejected("rejected-notary-evidence", rejected_notary)


def missing_notary_log_summary(_repo, candidate, _external, _output, _command):
    path = candidate.parent / "notary-app.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value.pop("log_summary")
    write_json(path, value)
    refreeze(candidate)


require_rejected("missing-notary-log-summary", missing_notary_log_summary)


def dirty_secret_scan(_repo, candidate, _external, _output, _command):
    path = candidate.parent / "secret-scan.json"
    value = json.loads(path.read_text(encoding="utf-8"))
    value["clean"] = False
    value["findings"] = [{"kind": "private-key"}]
    write_json(path, value)
    refreeze(candidate)


require_rejected("dirty-secret-evidence", dirty_secret_scan)


def rebind_asset(candidate: Path, name: str) -> None:
    release = candidate.parent.parent
    value = json.loads(candidate.read_text(encoding="utf-8"))
    for asset in value["assets"]:
        if asset["name"] == name:
            path = release / asset["path"]
            asset["sha256"] = sha(path)
            asset["size"] = path.stat().st_size
            break
    else:
        raise SystemExit(f"fixture asset missing: {name}")
    write_json(candidate, value)
    refreeze(candidate)


def flip_base64(value: str) -> str:
    return ("A" if value[0] != "A" else "B") + value[1:]


def rebind_components(candidate: Path) -> None:
    release = candidate.parent.parent
    app = release / "Patchwright.app"
    components = {
        "Patchwright.app": component_digest(app),
        "patchwright-engine": component_digest(app / "Contents/Helpers/patchwright-engine"),
        "patchwright-relay": component_digest(app / "Contents/Helpers/patchwright-relay"),
    }
    sbom_path = candidate.parent / "sbom.spdx.json"
    sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
    for row in sbom["files"]:
        row["checksums"][0]["checksumValue"] = components[row["fileName"]]
    write_json(sbom_path, sbom)
    assembly_path = candidate.parent / "assembly.json"
    assembly = json.loads(assembly_path.read_text(encoding="utf-8"))
    assembly["compliance"]["sbom_sha256"] = sha(sbom_path)
    assembly["compliance"]["post_signing_components"] = components
    write_json(assembly_path, assembly)
    rebind_asset(candidate, "sbom.spdx.json")


def malformed_sidecar(_repo, candidate, _external, _output, _command):
    name = "Patchwright-0.1.0.dmg.sha256"
    (candidate.parent.parent / name).write_text("not-a-portable-checksum\n", encoding="utf-8")
    rebind_asset(candidate, name)


require_rejected("malformed-portable-checksum", malformed_sidecar)


def malformed_appcast(_repo, candidate, _external, _output, _command):
    (candidate.parent.parent / "appcast.xml").write_text("<rss/>\n", encoding="utf-8")
    rebind_asset(candidate, "appcast.xml")


require_rejected("malformed-appcast", malformed_appcast)


def invalid_feed_signature(_repo, candidate, _external, _output, _command):
    appcast = candidate.parent.parent / "appcast.xml"
    content = appcast.read_text(encoding="utf-8")
    content = content.replace(
        f'edSignature: {SIGNATURES["feed_signature"]}',
        f'edSignature: {flip_base64(SIGNATURES["feed_signature"])}',
        1,
    )
    appcast.write_text(content, encoding="utf-8")
    rebind_asset(candidate, "appcast.xml")


require_rejected("invalid-feed-ed25519-signature", invalid_feed_signature)


def invalid_archive_signature(_repo, candidate, _external, _output, _command):
    appcast = candidate.parent.parent / "appcast.xml"
    content = appcast.read_text(encoding="utf-8")
    content = content.replace(
        f'sparkle:edSignature="{SIGNATURES["archive_signature"]}"',
        f'sparkle:edSignature="{flip_base64(SIGNATURES["archive_signature"])}"',
        1,
    )
    appcast.write_text(content, encoding="utf-8")
    rebind_asset(candidate, "appcast.xml")


require_rejected("invalid-archive-ed25519-signature", invalid_archive_signature)


def wrong_sparkle_public_key(_repo, candidate, _external, _output, _command):
    info_path = candidate.parent.parent / "Patchwright.app/Contents/Info.plist"
    with info_path.open("rb") as handle:
        info = plistlib.load(handle)
    info["SUPublicEDKey"] = base64.b64encode(bytes([0xA5]) * 32).decode("ascii")
    with info_path.open("wb") as handle:
        plistlib.dump(info, handle)
    rebind_components(candidate)


require_rejected("wrong-sparkle-public-key", wrong_sparkle_public_key)


def rebind_tampered_dmg(_repo, candidate, external, _output, _command):
    release = candidate.parent.parent
    dmg = release / "Patchwright-0.1.0.dmg"
    mutated = bytearray(dmg.read_bytes())
    mutated[0] ^= 1
    dmg.write_bytes(mutated)
    new_digest = sha(dmg)
    sidecar = release / "Patchwright-0.1.0.dmg.sha256"
    sidecar.write_text(f"{new_digest}  Patchwright-0.1.0.dmg\n", encoding="utf-8")
    identity_paths = [
        candidate,
        candidate.parent / "assembly.json",
        candidate.parent / "build-metadata.json",
        candidate.parent / "distribution.json",
        candidate.parent / "repo.json",
        candidate.parent / "secret-scan-gate.json",
        candidate.parent / "compliance-gate.json",
        external / "codex.json",
        external / "github.json",
        external / "clean-machine.json",
    ]
    for path in identity_paths:
        value = json.loads(path.read_text(encoding="utf-8"))
        value["artifact_sha256"] = new_digest
        if path == candidate:
            value["artifact_size"] = dmg.stat().st_size
            for asset in value["assets"]:
                asset_path = release / asset["path"]
                if asset["name"] in {"Patchwright-0.1.0.dmg", "Patchwright-0.1.0.dmg.sha256"}:
                    asset["sha256"] = sha(asset_path)
                    asset["size"] = asset_path.stat().st_size
        write_json(path, value)
    notary_path = candidate.parent / "notary-dmg.json"
    notary = json.loads(notary_path.read_text(encoding="utf-8"))
    notary["final_sha256"] = new_digest
    write_json(notary_path, notary)
    refreeze(candidate)


require_rejected("rebound-tampered-dmg-signature", rebind_tampered_dmg)


def invalid_spdx(_repo, candidate, _external, _output, _command):
    path = candidate.parent / "sbom.spdx.json"
    write_json(path, {"spdxVersion": "SPDX-0.0", "dataLicense": "NONE"})
    rebind_asset(candidate, "sbom.spdx.json")


require_rejected("invalid-spdx", invalid_spdx)


def forged_component_hashes(_repo, candidate, _external, _output, _command):
    sbom_path = candidate.parent / "sbom.spdx.json"
    assembly_path = candidate.parent / "assembly.json"
    sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
    forged = {row["fileName"]: "f" * 64 for row in sbom["files"]}
    for row in sbom["files"]:
        row["checksums"][0]["checksumValue"] = forged[row["fileName"]]
    write_json(sbom_path, sbom)
    assembly = json.loads(assembly_path.read_text(encoding="utf-8"))
    assembly["compliance"]["sbom_sha256"] = sha(sbom_path)
    assembly["compliance"]["post_signing_components"] = forged
    write_json(assembly_path, assembly)
    rebind_asset(candidate, "sbom.spdx.json")


require_rejected("forged-post-signing-component-hashes", forged_component_hashes)


leak_repo, leak_candidate, leak_external = create_fixture(temporary / "redaction")
leak_value = json.loads(leak_candidate.read_text(encoding="utf-8"))
leak_value["signing"]["keychain_path"] = "/Users/alice/Library/Keychains/login.keychain-db"
leak_value["notarization"]["dmg"]["raw_log"] = "PRIVATE-NOTARY-LOG"
write_json(leak_candidate, leak_value)
refreeze(leak_candidate)
leak_output = temporary / "redaction-output"
leak_result = subprocess.run(promotion_command(leak_repo, leak_candidate, leak_external, leak_output), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
if leak_result.returncode != 0:
    raise SystemExit(f"allowlisted redaction fixture failed: {leak_result.stderr}")
published = (leak_output / "release-evidence.json").read_text(encoding="utf-8") + (leak_output / "release-assets.json").read_text(encoding="utf-8")
if "/Users/" in published or "PRIVATE-NOTARY-LOG" in published:
    raise SystemExit("public promotion outputs leaked unvalidated local fields")

print("promotion matrix: 29 safety categories passed")
PY

echo "Patchwright promotion contract passed"
