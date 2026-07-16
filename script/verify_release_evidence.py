#!/usr/bin/env python3
"""Verify a frozen Patchwright candidate and digest-bound release gates."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
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
    return gate, digest(path, f"{expected_gate} gate")


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
    names: set[str] = set()
    for index, item in enumerate(assets):
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise VerificationError(f"candidate asset {index} is invalid")
        name = item["name"]
        path = resolve_relative(release_root, item.get("path"), f"assets[{index}].path")
        metadata = regular_file(path, f"asset {name}")
        if name in names or str(path) in {str(existing) for existing in asset_paths.values()}:
            raise VerificationError("duplicate candidate asset name or path")
        names.add(name)
        asset_paths[name] = path
        if item.get("sha256") != digest(path, f"asset {name}") or item.get("size") != metadata.st_size:
            raise VerificationError(f"candidate asset digest or size mismatch: {name}")

    evidence = candidate.get("evidence")
    if not isinstance(evidence, dict):
        raise VerificationError("candidate evidence map is missing")
    evidence_paths = {key: resolve_relative(release_root, value, f"evidence.{key}") for key, value in evidence.items()}
    for key, path in evidence_paths.items():
        regular_file(path, f"candidate evidence {key}")

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
        output = arguments.output_dir
        if output.exists() and (output.is_symlink() or not output.is_dir() or any(output.iterdir())):
            raise VerificationError("output directory must be a new or empty regular directory")
        output_parent = output.parent.resolve(strict=True)
        resolved_output = output_parent / output.name
        try:
            resolved_output.relative_to(release_root)
            raise VerificationError("output directory must be outside candidate root")
        except ValueError:
            pass
        output.mkdir(mode=0o700, exist_ok=True)
        release_evidence = {
            "schema_version": 1, "kind": "patchwright.release-evidence", "identity": identity,
            "signing": candidate["signing"], "notarization": candidate["notarization"], "gatekeeper": candidate["gatekeeper"],
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
            assets.append({"name": name, "path": str(path), "sha256": digest(path, f"release asset {name}"), "size": path.stat().st_size, "media_type": media.get(path.suffix, "application/octet-stream")})
        assets.append({"name": evidence_output.name, "path": str(evidence_output), "sha256": digest(evidence_output, "release evidence"), "size": evidence_output.stat().st_size, "media_type": "application/json"})
        asset_output = output / "release-assets.json"
        write_json(asset_output, {"schema_version": 1, "kind": "patchwright.release-assets", "identity": identity, "assets": assets})
        readiness_output = output / "promotion-readiness.json"
        write_json(readiness_output, {"schema_version": 1, "kind": "patchwright.promotion-readiness", "ready": True, "identity": identity, "evidence_sha256": evidence_hashes, "release_assets_sha256": digest(asset_output, "release asset manifest")})
        print(f"PATCHWRIGHT_PROMOTION_READINESS={readiness_output}\nPATCHWRIGHT_RELEASE_ASSET_MANIFEST={asset_output}\nPATCHWRIGHT_STATUS=release-assets-ready")
        return 0
    except (VerificationError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"release evidence rejected: {error}", file=sys.stderr)
        return 65


if __name__ == "__main__":
    raise SystemExit(main())
