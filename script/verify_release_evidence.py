#!/usr/bin/env python3
"""Verify a frozen Patchwright candidate and digest-bound release gates."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path, PurePosixPath
from typing import Any

MAX_JSON_BYTES = 1024 * 1024
MAX_AGE = timedelta(hours=168)
CLOCK_SKEW = timedelta(minutes=5)
HEX64 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$")
BUILD = re.compile(r"^[1-9][0-9]*$")
IDENTITY_KEYS = ("artifact_filename", "artifact_sha256", "git_commit", "tag", "version", "build")
GATE_CHECKS = {
    "repo": {"source_verify", "clean_source", "tag_binding"},
    "secret_scan": {"tracked", "all_refs", "candidate_root", "no_findings"},
    "compliance": {"spdx_2_3", "dependency_licenses", "post_signing_component_hashes"},
    "codex": {"signed_in_runtime", "task_start", "resume", "approval", "cancel"},
    "github": {"authorized_sandbox", "app_identity", "delivery", "exact_sha_approval", "merge", "kill_switch"},
    "clean_machine": {"checksum", "dmg_signature", "dmg_ticket", "dmg_gatekeeper", "app_signature", "app_ticket", "app_gatekeeper", "first_launch", "relaunch"},
}


class VerificationError(Exception):
    pass


def duplicate_free(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise VerificationError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def regular_file(path: Path, label: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except FileNotFoundError as error:
        raise VerificationError(f"missing {label}: {path}") from error
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        raise VerificationError(f"{label} must be a regular non-symlink file: {path}")
    return metadata


def read_bytes(path: Path, label: str, limit: int | None = None) -> bytes:
    before = regular_file(path, label)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        after = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise VerificationError(f"{label} changed while opening: {path}")
        if limit is not None and after.st_size > limit:
            raise VerificationError(f"{label} exceeds {limit} bytes: {path}")
        chunks: list[bytes] = []
        remaining = after.st_size
        while remaining:
            chunk = os.read(descriptor, min(65536, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def load_json(path: Path, label: str) -> dict[str, Any]:
    raw = read_bytes(path, label, MAX_JSON_BYTES)
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=duplicate_free)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"malformed {label}: {path}: {error}") from error
    if not isinstance(value, dict):
        raise VerificationError(f"{label} root must be an object: {path}")
    return value


def digest(path: Path, label: str) -> str:
    return hashlib.sha256(read_bytes(path, label)).hexdigest()


def parse_time(value: Any, label: str, now: datetime) -> datetime:
    if not isinstance(value, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value):
        raise VerificationError(f"{label} must be RFC3339 UTC seconds")
    parsed = datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    if parsed > now + CLOCK_SKEW:
        raise VerificationError(f"{label} is more than five minutes in the future")
    if now - parsed > MAX_AGE:
        raise VerificationError(f"{label} is older than 168 hours")
    return parsed


def relative_path(value: Any, label: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "//" in value:
        raise VerificationError(f"invalid relative path for {label}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise VerificationError(f"unsafe relative path for {label}: {value}")
    return path


def resolve_relative(root: Path, value: Any, label: str) -> Path:
    relative = relative_path(value, label)
    target = root.joinpath(*relative.parts)
    try:
        target.parent.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (FileNotFoundError, ValueError) as error:
        raise VerificationError(f"{label} escapes candidate root: {value}") from error
    return target


def require_identity(value: dict[str, Any], label: str) -> dict[str, str]:
    identity: dict[str, str] = {}
    for key in IDENTITY_KEYS:
        item = value.get(key)
        if not isinstance(item, str):
            raise VerificationError(f"{label}.{key} must be a string")
        identity[key] = item
    if not HEX64.fullmatch(identity["artifact_sha256"]):
        raise VerificationError(f"{label}.artifact_sha256 is not canonical SHA-256")
    if not COMMIT.fullmatch(identity["git_commit"]):
        raise VerificationError(f"{label}.git_commit is not canonical")
    if not VERSION.fullmatch(identity["version"]):
        raise VerificationError(f"{label}.version is not semantic")
    if not BUILD.fullmatch(identity["build"]):
        raise VerificationError(f"{label}.build must be a positive decimal string")
    if identity["tag"] != f"v{identity['version']}":
        raise VerificationError(f"{label}.tag must equal v<version>")
    if identity["artifact_filename"] != f"Patchwright-{identity['version']}.dmg":
        raise VerificationError(f"{label}.artifact_filename mismatch")
    return identity


def compare_identity(expected: dict[str, str], actual: dict[str, str], label: str) -> None:
    for key in IDENTITY_KEYS:
        if actual[key] != expected[key]:
            raise VerificationError(f"{label}.{key} does not match candidate")


def verify_gate(path: Path, expected_gate: str, identity: dict[str, str], now: datetime, candidate_time: datetime) -> tuple[dict[str, Any], str]:
    before = regular_file(path, f"{expected_gate} gate")
    gate = load_json(path, f"{expected_gate} gate")
    if gate.get("schema_version") != 1 or gate.get("gate") != expected_gate or gate.get("status") != "pass":
        raise VerificationError(f"invalid {expected_gate} gate envelope")
    compare_identity(identity, require_identity(gate, f"{expected_gate} gate"), f"{expected_gate} gate")
    completed = parse_time(gate.get("completed_at"), f"{expected_gate}.completed_at", now)
    if expected_gate in {"codex", "github", "clean_machine"} and completed + CLOCK_SKEW < candidate_time:
        raise VerificationError(f"{expected_gate} gate predates candidate")
    checks = gate.get("checks")
    required = GATE_CHECKS[expected_gate]
    if not isinstance(checks, dict) or set(checks) != required or any(checks[name] is not True for name in required):
        raise VerificationError(f"{expected_gate} gate required checks are not exactly true")
    if expected_gate == "clean_machine":
        guest = gate.get("guest")
        if not isinstance(guest, dict) or guest.get("gatekeeper_enabled") is not True or guest.get("architecture") != "arm64":
            raise VerificationError("clean_machine guest evidence is invalid")
    gate_digest = digest(path, f"{expected_gate} gate")
    after = regular_file(path, f"{expected_gate} gate")
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns):
        raise VerificationError(f"{expected_gate} gate changed during validation")
    return gate, gate_digest


def verify_candidate(candidate_path: Path, repo: Path, now: datetime) -> tuple[dict[str, Any], Path, dict[str, str], datetime, dict[str, Path]]:
    candidate = load_json(candidate_path, "candidate manifest")
    if candidate.get("schema_version") != 1 or candidate.get("kind") != "patchwright.notarized-candidate" or candidate.get("product") != "Patchwright":
        raise VerificationError("invalid candidate envelope")
    identity = require_identity(candidate, "candidate")
    created = parse_time(candidate.get("created_at"), "candidate.created_at", now)
    release_root = candidate_path.parent.parent.resolve(strict=True)
    if candidate_path.resolve(strict=True) != release_root / "evidence" / "notarized-candidate.json":
        raise VerificationError("candidate manifest must be evidence/notarized-candidate.json")
    artifact = resolve_relative(release_root, candidate.get("artifact_path"), "artifact_path")
    if artifact.name != identity["artifact_filename"]:
        raise VerificationError("candidate artifact path mismatch")
    artifact_stat = regular_file(artifact, "candidate artifact")
    if candidate.get("artifact_size") != artifact_stat.st_size or digest(artifact, "candidate artifact") != identity["artifact_sha256"]:
        raise VerificationError("candidate artifact digest or size mismatch")
    signing = candidate.get("signing")
    notarization = candidate.get("notarization")
    gatekeeper = candidate.get("gatekeeper")
    if not isinstance(signing, dict) or signing.get("identity_class") != "Developer ID Application" or signing.get("hardened_runtime") is not True or signing.get("secure_timestamp") is not True:
        raise VerificationError("candidate signing evidence is invalid")
    if not isinstance(notarization, dict) or any(not isinstance(notarization.get(kind), dict) or notarization[kind].get("status") != "Accepted" or notarization[kind].get("stapled") is not True for kind in ("app", "dmg")):
        raise VerificationError("candidate notarization evidence is invalid")
    if gatekeeper != {"app": True, "dmg": True}:
        raise VerificationError("candidate Gatekeeper evidence is invalid")

    assets = candidate.get("assets")
    if not isinstance(assets, list) or not assets:
        raise VerificationError("candidate assets must be a nonempty array")
    asset_paths: dict[str, Path] = {}
    asset_relatives: dict[str, str] = {}
    asset_hashes: dict[str, str] = {}
    asset_sizes: dict[str, int] = {}
    names: set[str] = set()
    for index, item in enumerate(assets):
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise VerificationError(f"candidate asset {index} is invalid")
        name = item["name"]
        relative = relative_path(item.get("path"), f"assets[{index}].path")
        path = resolve_relative(release_root, str(relative), f"assets[{index}].path")
        metadata = regular_file(path, f"asset {name}")
        if name != path.name:
            raise VerificationError(f"candidate asset name must equal path basename: {name}")
        if name in names or str(path) in {str(existing) for existing in asset_paths.values()}:
            raise VerificationError("duplicate candidate asset name or path")
        names.add(name)
        asset_paths[name] = path
        asset_relatives[name] = str(relative)
        asset_hashes[name] = digest(path, f"asset {name}")
        asset_sizes[name] = metadata.st_size
        if item.get("sha256") != asset_hashes[name] or item.get("size") != asset_sizes[name]:
            raise VerificationError(f"candidate asset digest or size mismatch: {name}")
    if asset_paths.get(identity["artifact_filename"]) != artifact:
        raise VerificationError("public DMG asset must be the canonical candidate artifact")

    evidence = candidate.get("evidence")
    if not isinstance(evidence, dict):
        raise VerificationError("candidate evidence map is missing")
    evidence_paths = {key: resolve_relative(release_root, value, f"evidence.{key}") for key, value in evidence.items()}
    for key, path in evidence_paths.items():
        regular_file(path, f"candidate evidence {key}")

    assembly = load_json(evidence_paths["assembly"], "assembly evidence")
    compare_identity(identity, require_identity(assembly, "assembly evidence"), "assembly evidence")
    if assembly.get("schema_version") != 1 or assembly.get("dirty") is not False or assembly.get("candidate") is not True:
        raise VerificationError("assembly evidence is not a clean candidate")
    build_metadata = load_json(evidence_paths["build_metadata"], "build metadata")
    compare_identity(identity, require_identity(build_metadata, "build metadata"), "build metadata")
    secret_scan = load_json(evidence_paths["secret_scan"], "secret scan evidence")
    if secret_scan.get("schema_version") != 1 or secret_scan.get("clean") is not True or secret_scan.get("findings") != []:
        raise VerificationError("secret scan evidence is not clean")
    for kind, evidence_key in (("app", "notary_app"), ("dmg", "notary_dmg")):
        notary = load_json(evidence_paths[evidence_key], f"{kind} notary evidence")
        if notary.get("schema_version") != 1 or notary.get("kind") != kind or notary.get("status") != "Accepted" or notary.get("stapled") is not True or notary.get("stapler_validated") is not True or not isinstance(notary.get("request_id"), str) or not notary["request_id"]:
            raise VerificationError(f"{kind} notary evidence is not accepted and stapled")
        if kind == "dmg" and notary.get("final_sha256") != identity["artifact_sha256"]:
            raise VerificationError("DMG notary evidence does not bind the final artifact")
    distribution = load_json(evidence_paths["distribution"], "distribution evidence")
    compare_identity(identity, require_identity(distribution, "distribution evidence"), "distribution evidence")
    distribution_checks = {"dmg_signature", "dmg_ticket", "dmg_gatekeeper", "app_signature", "app_ticket", "app_gatekeeper", "bundle_layout", "team_id", "hardened_runtime", "entitlements"}
    if distribution.get("schema_version") != 1 or distribution.get("status") != "pass" or not isinstance(distribution.get("checks"), dict) or set(distribution["checks"]) != distribution_checks or any(distribution["checks"].get(check) is not True for check in distribution_checks):
        raise VerificationError("distribution evidence is not an exact pass")

    symlink_manifest = load_json(evidence_paths["symlinks"], "symlink manifest")
    links = symlink_manifest.get("links")
    if symlink_manifest.get("schema_version") != 1 or not isinstance(links, list):
        raise VerificationError("symlink manifest is invalid")
    expected_links: dict[str, str] = {}
    for item in links:
        if not isinstance(item, dict) or set(item) != {"path", "target"} or not isinstance(item["target"], str):
            raise VerificationError("symlink manifest entry is invalid")
        link_path = str(relative_path(item["path"], "symlink path"))
        if link_path in expected_links or Path(item["target"]).is_absolute():
            raise VerificationError("symlink manifest contains duplicate or absolute entry")
        expected_links[link_path] = item["target"]
    actual_links: dict[str, str] = {}
    for path in release_root.rglob("*"):
        if path.is_symlink():
            actual_links[path.relative_to(release_root).as_posix()] = os.readlink(path)
            try:
                path.resolve(strict=True).relative_to(release_root)
            except (OSError, ValueError) as error:
                raise VerificationError(f"candidate symlink escapes or dangles: {path}") from error
    if actual_links != expected_links:
        raise VerificationError("release symlinks do not match the recorded manifest")

    checksum_path = evidence_paths.get("checksums")
    if checksum_path is None:
        raise VerificationError("candidate checksums evidence is missing")
    checksum_lines = read_bytes(checksum_path, "candidate checksums", MAX_JSON_BYTES).decode("utf-8").splitlines()
    checksums: dict[str, str] = {}
    for line in checksum_lines:
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if not match or match.group(2) in checksums:
            raise VerificationError("malformed or duplicate SHA256SUMS entry")
        checksums[match.group(2)] = match.group(1)
    actual: dict[str, str] = {}
    for path in sorted(release_root.rglob("*")):
        metadata = path.lstat()
        if path == checksum_path:
            continue
        if stat.S_ISREG(metadata.st_mode):
            relative = path.relative_to(release_root).as_posix()
            actual[relative] = digest(path, f"frozen candidate file {relative}")
        elif not (stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode)):
            raise VerificationError(f"unsupported candidate file type: {path}")
    if checksums != actual:
        raise VerificationError("SHA256SUMS does not exactly cover the frozen candidate")

    repo_path = repo.resolve(strict=True)
    if not (repo_path / ".git").exists():
        raise VerificationError("repo is not a Git repository")
    try:
        tagged = subprocess.check_output(["git", "-C", str(repo_path), "rev-parse", f"refs/tags/{identity['tag']}^{{commit}}"], text=True, stderr=subprocess.DEVNULL).strip()
    except subprocess.CalledProcessError as error:
        raise VerificationError("candidate tag is absent") from error
    if tagged != identity["git_commit"]:
        raise VerificationError("candidate tag does not resolve to candidate commit")
    candidate["_asset_relatives"] = asset_relatives
    candidate["_asset_hashes"] = asset_hashes
    candidate["_asset_sizes"] = asset_sizes
    return candidate, release_root, identity, created, asset_paths


def parse_now(value: str | None) -> datetime:
    if value is None:
        return datetime.now(timezone.utc).replace(microsecond=0)
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value):
        raise VerificationError("--now must be RFC3339 UTC seconds")
    return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def write_json(path: Path, value: Any) -> None:
    data = (json.dumps(value, indent=2, sort_keys=True, separators=(",", ": ")) + "\n").encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        os.write(descriptor, data)
    finally:
        os.close(descriptor)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    subcommands = result.add_subparsers(dest="command", required=True)
    for name in ("candidate", "promotion"):
        command = subcommands.add_parser(name)
        command.add_argument("--candidate", required=True, type=Path)
        command.add_argument("--repo", required=True, type=Path)
        command.add_argument("--now")
        if name == "promotion":
            command.add_argument("--codex", required=True, type=Path)
            command.add_argument("--github", required=True, type=Path)
            command.add_argument("--clean-machine", required=True, type=Path)
            command.add_argument("--output-dir", required=True, type=Path)
    return result


def main() -> int:
    arguments = parser().parse_args()
    temporary_output: Path | None = None
    try:
        now = parse_now(arguments.now)
        candidate, release_root, identity, created, asset_paths = verify_candidate(arguments.candidate, arguments.repo, now)
        if arguments.command == "candidate":
            print(f"PATCHWRIGHT_STATUS=notarized-candidate\nPATCHWRIGHT_ARTIFACT_SHA256={identity['artifact_sha256']}")
            return 0
        evidence_map = candidate["evidence"]
        package_gate_paths = {
            "repo": resolve_relative(release_root, evidence_map["repo_gate"], "repo_gate"),
            "secret_scan": resolve_relative(release_root, evidence_map["secret_scan_gate"], "secret_scan_gate"),
            "compliance": resolve_relative(release_root, evidence_map["compliance_gate"], "compliance_gate"),
        }
        external_gate_paths = {"codex": arguments.codex, "github": arguments.github, "clean_machine": arguments.clean_machine}
        gate_summaries: dict[str, Any] = {}
        evidence_hashes: dict[str, str] = {}
        for name, path in {**package_gate_paths, **external_gate_paths}.items():
            gate, gate_hash = verify_gate(path, name, identity, now, created)
            gate_summaries[name] = {"status": gate["status"], "completed_at": gate["completed_at"], "checks": gate["checks"]}
            evidence_hashes[name] = gate_hash
        requested_output = arguments.output_dir
        if requested_output.exists() or requested_output.is_symlink():
            raise VerificationError("output directory must not already exist")
        output_parent = requested_output.parent.resolve(strict=True)
        resolved_output = output_parent / requested_output.name
        try:
            resolved_output.relative_to(release_root)
            raise VerificationError("output directory must be outside candidate root")
        except ValueError:
            pass
        temporary_output = Path(tempfile.mkdtemp(prefix=f".{requested_output.name}.", dir=output_parent))
        os.chmod(temporary_output, 0o700)
        output = temporary_output
        public_signing = {key: candidate["signing"][key] for key in ("identity_class", "team_id", "hardened_runtime", "secure_timestamp")}
        public_notarization = {
            kind: {key: candidate["notarization"][kind][key] for key in ("status", "request_id", "stapled")}
            for kind in ("app", "dmg")
        }
        release_evidence = {
            "schema_version": 1, "kind": "patchwright.release-evidence", "identity": identity,
            "signing": public_signing, "notarization": public_notarization, "gatekeeper": candidate["gatekeeper"],
            "gates": gate_summaries,
        }
        evidence_output = output / "release-evidence.json"
        write_json(evidence_output, release_evidence)
        public_names = [identity["artifact_filename"], f"{identity['artifact_filename']}.sha256", "appcast.xml", "sbom.spdx.json", "third-party-notices.md"]
        media = {".dmg": "application/x-apple-diskimage", ".xml": "application/xml", ".json": "application/json", ".md": "text/markdown", ".sha256": "text/plain"}
        assets: list[dict[str, Any]] = []
        for name in public_names:
            path = asset_paths.get(name)
            if path is None:
                raise VerificationError(f"required public asset is missing: {name}")
            current_hash = digest(path, f"release asset {name}")
            current_size = regular_file(path, f"release asset {name}").st_size
            if current_hash != candidate["_asset_hashes"][name] or current_size != candidate["_asset_sizes"][name]:
                raise VerificationError(f"release asset changed after candidate validation: {name}")
            assets.append({"name": name, "path": candidate["_asset_relatives"][name], "sha256": current_hash, "size": current_size, "media_type": media.get(path.suffix, "application/octet-stream")})
        assets.append({"name": evidence_output.name, "path": evidence_output.name, "sha256": digest(evidence_output, "release evidence"), "size": evidence_output.stat().st_size, "media_type": "application/json"})
        asset_output = output / "release-assets.json"
        write_json(asset_output, {"schema_version": 1, "kind": "patchwright.release-assets", "identity": identity, "assets": assets})
        readiness_output = output / "promotion-readiness.json"
        write_json(readiness_output, {"schema_version": 1, "kind": "patchwright.promotion-readiness", "ready": True, "identity": identity, "evidence_sha256": evidence_hashes, "release_assets_sha256": digest(asset_output, "release asset manifest")})
        os.rename(temporary_output, resolved_output)
        temporary_output = None
        print(f"PATCHWRIGHT_PROMOTION_READINESS={resolved_output / readiness_output.name}\nPATCHWRIGHT_RELEASE_ASSET_MANIFEST={resolved_output / asset_output.name}\nPATCHWRIGHT_STATUS=release-assets-ready")
        return 0
    except (VerificationError, OSError, KeyError, subprocess.SubprocessError) as error:
        if temporary_output is not None:
            shutil.rmtree(temporary_output, ignore_errors=True)
        print(f"release evidence rejected: {error}", file=sys.stderr)
        return 65


if __name__ == "__main__":
    raise SystemExit(main())
