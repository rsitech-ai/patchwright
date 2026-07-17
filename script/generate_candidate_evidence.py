#!/usr/bin/env python3
"""Generate public-safe, digest-bound candidate and package gate evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


class CandidateError(ValueError):
    pass


def regular(path: Path, label: str) -> Path:
    try:
        mode = path.lstat().st_mode
    except FileNotFoundError as error:
        raise CandidateError(f"missing {label}: {path}") from error
    if not stat.S_ISREG(mode) or path.is_symlink():
        raise CandidateError(f"{label} must be a regular non-symlink file")
    return path


def load(path: Path, label: str) -> dict[str, Any]:
    regular(path, label)
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CandidateError(f"invalid {label}: {error}") from error
    if not isinstance(value, dict):
        raise CandidateError(f"{label} must be a JSON object")
    return value


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with regular(path, path.name).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def component_digest(path: Path, label: str) -> str:
    if path.is_symlink():
        raise CandidateError(f"{label} must not be a symlink")
    if path.is_file():
        return sha(path)
    if not path.is_dir():
        raise CandidateError(f"missing or unsupported {label}: {path}")
    digest = hashlib.sha256()
    for entry in sorted(path.rglob("*"), key=lambda item: item.relative_to(path).as_posix()):
        relative = entry.relative_to(path).as_posix().encode("utf-8")
        metadata = entry.lstat()
        mode = stat.S_IMODE(metadata.st_mode)
        if stat.S_ISLNK(metadata.st_mode):
            kind, payload = b"L", os.readlink(entry).encode("utf-8")
        elif stat.S_ISREG(metadata.st_mode):
            kind, payload = b"F", bytes.fromhex(sha(entry))
        elif stat.S_ISDIR(metadata.st_mode):
            kind, payload = b"D", b""
        else:
            raise CandidateError(f"unsupported {label} entry: {entry}")
        digest.update(kind + b"\0" + relative + b"\0" + str(mode).encode("ascii") + b"\0" + payload)
    return digest.hexdigest()


def write(path: Path, value: dict[str, Any]) -> None:
    if path.exists() or path.is_symlink():
        if path.is_symlink() or not path.is_file():
            raise CandidateError(f"output is not a regular file: {path}")
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    if temporary.exists() or temporary.is_symlink():
        raise CandidateError(f"temporary output already exists: {temporary}")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)


def identity(args: argparse.Namespace, digest: str, source_digest: str) -> dict[str, str]:
    commit = subprocess.check_output(["git", "-C", str(args.repo), "rev-parse", "HEAD"], text=True).strip()
    tag = f"v{args.version}"
    tagged = subprocess.check_output(["git", "-C", str(args.repo), "rev-parse", f"refs/tags/{tag}^{{commit}}"], text=True, stderr=subprocess.DEVNULL).strip()
    if tagged != commit:
        raise CandidateError(f"{tag} does not resolve to HEAD")
    return {
        "artifact_filename": args.dmg.name,
        "artifact_sha256": digest,
        "git_commit": commit,
        "tag": tag,
        "version": args.version,
        "build": args.build,
        "source_archive_path": "reproducibility/source.tar.gz",
        "source_archive_sha256": source_digest,
    }


def gate(name: str, common: dict[str, str], completed: str, checks: list[str]) -> dict[str, Any]:
    return {"schema_version": 1, "gate": name, "status": "pass", **common, "completed_at": completed, "checks": {item: True for item in checks}}


def require_notary_summary(document: dict[str, Any], label: str) -> None:
    summary = document.get("log_summary")
    required = {"log_sha256", "issue_count", "error_count", "warning_count", "info_count", "warning_policy"}
    if not isinstance(summary, dict) or set(summary) != required:
        raise CandidateError(f"{label} notary evidence has no sanitized log summary")
    counts = [summary.get(name) for name in ("error_count", "warning_count", "info_count")]
    if (
        not re.fullmatch(r"[0-9a-f]{64}", summary.get("log_sha256", ""))
        or any(not isinstance(count, int) or isinstance(count, bool) or count < 0 for count in counts)
        or summary.get("issue_count") != sum(counts)
        or summary.get("error_count") != 0
        or summary.get("warning_policy") not in {"reject", "allow"}
        or (summary.get("warning_policy") == "reject" and summary.get("warning_count") != 0)
    ):
        raise CandidateError(f"{label} notary log summary is invalid")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-root", required=True, type=Path)
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--dmg", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--build", required=True)
    parser.add_argument("--team-id", required=True)
    parser.add_argument("--created-at")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        root = args.release_root.resolve(strict=True)
        evidence = root / "evidence"
        if args.app.name != "Patchwright.app" or args.app.resolve(strict=True).parent != root or args.dmg.resolve(strict=True).parent != root:
            raise CandidateError("app and DMG must be direct candidate-root children")
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version) or not re.fullmatch(r"[1-9][0-9]*", args.build):
            raise CandidateError("invalid version or build")
        if args.dmg.name != f"Patchwright-{args.version}.dmg":
            raise CandidateError("DMG filename does not match version")
        if not re.fullmatch(r"[A-Z0-9]{10}", args.team_id):
            raise CandidateError("invalid Developer ID team identifier")
        created = args.created_at or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        artifact_digest = sha(args.dmg)
        source_archive = regular(root / "reproducibility" / "source.tar.gz", "source archive")
        source_digest = sha(source_archive)
        common = identity(args, artifact_digest, source_digest)

        notary_app = load(evidence / "notary-app.json", "app notary evidence")
        notary_dmg = load(evidence / "notary-dmg.json", "DMG notary evidence")
        require_notary_summary(notary_app, "app")
        require_notary_summary(notary_dmg, "DMG")
        distribution = load(evidence / "distribution.json", "distribution evidence")
        secret = load(evidence / "secret-scan.json", "secret scan evidence")
        sbom = load(evidence / "sbom.spdx.json", "SPDX evidence")
        notices = regular(evidence / "third-party-notices.md", "third-party notices")
        license_root = evidence / "third-party-licenses"
        if any(doc.get("status") != "Accepted" or doc.get("stapled") is not True or doc.get("stapler_validated") is not True for doc in (notary_app, notary_dmg)):
            raise CandidateError("notary evidence is not accepted and stapled")
        if notary_dmg.get("final_sha256") != artifact_digest:
            raise CandidateError("DMG notary evidence does not match final artifact")
        if distribution.get("status") != "pass" or secret.get("clean") is not True or secret.get("findings") != []:
            raise CandidateError("distribution or secret evidence is not a pass")
        packages = sbom.get("packages")
        if sbom.get("spdxVersion") != "SPDX-2.3" or sbom.get("dataLicense") != "CC0-1.0" or not isinstance(packages, list) or not packages or any(not isinstance(package, dict) or not isinstance(package.get("licenseDeclared"), str) or not package["licenseDeclared"].strip() for package in packages) or notices.stat().st_size == 0:
            raise CandidateError("compliance evidence is incomplete")
        if license_root.is_symlink() or not license_root.is_dir():
            raise CandidateError("bundled third-party license directory is missing")
        license_files = list(license_root.rglob("*"))
        if not license_files or any(path.is_symlink() or (not path.is_dir() and not path.is_file()) for path in license_files) or not any(path.is_file() for path in license_files):
            raise CandidateError("bundled third-party license evidence is incomplete")
        recorded_components: dict[str, str] = {}
        for row in sbom.get("files", []):
            if not isinstance(row, dict) or not isinstance(row.get("fileName"), str):
                raise CandidateError("SPDX component evidence is malformed")
            checksums = row.get("checksums")
            if not isinstance(checksums, list) or len(checksums) != 1 or checksums[0].get("algorithm") != "SHA256" or not re.fullmatch(r"[0-9a-f]{64}", checksums[0].get("checksumValue", "")):
                raise CandidateError("SPDX component checksum is malformed")
            if row["fileName"] in recorded_components:
                raise CandidateError("SPDX component evidence contains a duplicate")
            recorded_components[row["fileName"]] = checksums[0]["checksumValue"]
        components = {
            "Patchwright.app": component_digest(args.app, "Patchwright.app"),
            "patchwright-engine": component_digest(args.app / "Contents" / "Helpers" / "patchwright-engine", "patchwright-engine"),
            "patchwright-relay": component_digest(args.app / "Contents" / "Helpers" / "patchwright-relay", "patchwright-relay"),
        }
        if recorded_components != components:
            raise CandidateError("SPDX component hashes do not match the final signed app")

        assembly_path = evidence / "assembly.json"
        assembly = load(assembly_path, "assembly evidence")
        if assembly.get("dirty") is not False or assembly.get("candidate") is not True:
            raise CandidateError("assembly evidence must record a clean candidate")
        assembly.update({
            "schema_version": 1,
            **common,
            "compliance": {
                "sbom_sha256": sha(evidence / "sbom.spdx.json"),
                "third_party_notices_sha256": sha(notices),
                "post_signing_components": components,
                "secret_scan_binding": "evidence/SHA256SUMS",
            },
        })
        write(assembly_path, assembly)
        metadata_path = evidence / "build-metadata.json"
        metadata = load(metadata_path, "build metadata")
        if metadata.get("dirty") is not False:
            raise CandidateError("build metadata must record dirty=false")
        metadata.update({"schema_version": 1, **common})
        write(metadata_path, metadata)
        distribution.update({"schema_version": 1, **common})
        write(evidence / "distribution.json", distribution)

        checks = {
            "repo": ["source_verify", "clean_source", "tag_binding"],
            "secret_scan": ["tracked", "all_refs", "candidate_root", "no_findings"],
            "compliance": ["spdx_2_3", "dependency_licenses", "post_signing_component_hashes"],
        }
        gate_files = {"repo": "repo.json", "secret_scan": "secret-scan-gate.json", "compliance": "compliance-gate.json"}
        for name, names in checks.items():
            write(evidence / gate_files[name], gate(name, common, created, names))

        assets = []
        for path in (args.dmg, root / f"{args.dmg.name}.sha256", root / "appcast.xml", evidence / "sbom.spdx.json", notices):
            regular(path, path.name)
            assets.append({"name": path.name, "path": path.relative_to(root).as_posix(), "sha256": sha(path), "size": path.stat().st_size})
        candidate = {
            "schema_version": 1,
            "kind": "patchwright.notarized-candidate",
            "product": "Patchwright",
            **common,
            "artifact_path": args.dmg.name,
            "artifact_size": args.dmg.stat().st_size,
            "bundle_identifier": "ai.patchwright.app",
            "created_at": created,
            "signing": {"identity_class": "Developer ID Application", "team_id": args.team_id, "hardened_runtime": True, "secure_timestamp": True},
            "notarization": {
                "app": {"status": "Accepted", "request_id": notary_app["request_id"], "stapled": True},
                "dmg": {"status": "Accepted", "request_id": notary_dmg["request_id"], "stapled": True},
            },
            "gatekeeper": {"app": True, "dmg": True},
            "assets": assets,
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
        subprocess.run(
            [
                str(Path(__file__).with_name("verify_release_source.py")),
                "--repo", str(args.repo),
                "--commit", common["git_commit"],
                "--tag", common["tag"],
                "--source-archive", str(source_archive),
                "--source-archive-sha256", source_digest,
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        write(evidence / "notarized-candidate.json", candidate)
    except (CandidateError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"candidate evidence generation failed: {error}", file=sys.stderr)
        return 65
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
