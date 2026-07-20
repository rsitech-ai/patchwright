#!/usr/bin/env python3
"""Generate deterministic SPDX and third-party-notice release evidence."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import stat
import sys
from pathlib import Path
from typing import Any


class ComplianceError(ValueError):
    pass


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ComplianceError(f"invalid JSON metadata: {path.name}: {error}") from error
    if not isinstance(value, dict):
        raise ComplianceError(f"metadata root must be an object: {path.name}")
    return value


def require_text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ComplianceError(f"missing or invalid {label}")
    return value.strip()


def normalize_license(value: Any, label: str) -> str:
    license_expression = require_text(value, f"declared license for {label}")
    if license_expression == "MIT/Apache-2.0":
        return "MIT OR Apache-2.0"
    return license_expression


def rust_packages(metadata: dict[str, Any]) -> list[dict[str, str]]:
    raw_packages = metadata.get("packages")
    resolve = metadata.get("resolve")
    if not isinstance(raw_packages, list) or not isinstance(resolve, dict):
        raise ComplianceError("Cargo metadata must contain packages and resolve")
    raw_nodes = resolve.get("nodes")
    if not isinstance(raw_nodes, list):
        raise ComplianceError("Cargo metadata resolve must contain nodes")
    resolved_ids = {
        require_text(node.get("id"), "Cargo resolve node id")
        for node in raw_nodes
        if isinstance(node, dict)
    }
    if len(resolved_ids) != len(raw_nodes):
        raise ComplianceError("Cargo resolve nodes must be unique objects")
    workspace_members = metadata.get("workspace_members")
    workspace_ids = (
        {require_text(item, "Cargo workspace member") for item in workspace_members}
        if isinstance(workspace_members, list)
        else set()
    )
    packages: list[dict[str, str]] = []
    seen_ids: set[str] = set()
    for raw in raw_packages:
        if not isinstance(raw, dict):
            raise ComplianceError("Cargo package entries must be objects")
        package_id = require_text(raw.get("id"), "Cargo package id")
        if package_id not in resolved_ids:
            continue
        if package_id in seen_ids:
            raise ComplianceError(f"duplicate Cargo package id: {package_id}")
        seen_ids.add(package_id)
        name = require_text(raw.get("name"), "Cargo package name")
        source = raw.get("source")
        if source is not None and not isinstance(source, str):
            raise ComplianceError(f"invalid Cargo source for {name}")
        first_party = package_id in workspace_ids or (not workspace_ids and source is None)
        manifest_path = require_text(raw.get("manifest_path"), f"Cargo manifest path for {name}")
        license_file = raw.get("license_file")
        if license_file is not None and not isinstance(license_file, str):
            raise ComplianceError(f"invalid Cargo license_file for {name}")
        packages.append(
            {
                "name": name,
                "version": require_text(raw.get("version"), f"Cargo version for {name}"),
                "license": normalize_license(raw.get("license"), name),
                "source": source or "workspace",
                "ecosystem": "Rust",
                "first_party": "true" if first_party else "false",
                "package_root": str(Path(manifest_path).parent),
                "license_file": license_file or "",
            }
        )
    missing = resolved_ids - seen_ids
    if missing:
        raise ComplianceError("Cargo resolve references packages absent from metadata")
    return packages


def known_swift_license(identity: str, url: str, raw: dict[str, Any]) -> str:
    if raw.get("license") is not None:
        return normalize_license(raw.get("license"), identity)
    normalized_url = url.lower().removesuffix(".git").rstrip("/")
    if identity.lower() == "sparkle" and normalized_url == "https://github.com/sparkle-project/sparkle":
        return "MIT"
    raise ComplianceError(f"missing declared license mapping for Swift dependency {identity}")


def swift_packages(metadata: dict[str, Any]) -> list[dict[str, str]]:
    root_name = require_text(metadata.get("name"), "Swift root package name")
    root_version = metadata.get("version")
    if not isinstance(root_version, str) or not root_version.strip():
        root_version = "unspecified"
    packages = [
        {
            "name": root_name,
            "version": root_version,
            "license": "MIT OR Apache-2.0",
            "source": "workspace",
            "ecosystem": "Swift",
            "first_party": "true",
        }
    ]
    seen: set[tuple[str, str]] = set()

    def visit(raw: Any) -> None:
        if not isinstance(raw, dict):
            raise ComplianceError("Swift dependency entries must be objects")
        identity = require_text(raw.get("identity"), "Swift dependency identity")
        name = require_text(raw.get("name"), f"Swift dependency name for {identity}")
        version = require_text(raw.get("version"), f"Swift dependency version for {identity}")
        url = require_text(raw.get("url"), f"Swift dependency URL for {identity}")
        package_path = require_text(raw.get("path"), f"Swift dependency path for {identity}")
        key = (identity.lower(), version)
        if key not in seen:
            seen.add(key)
            packages.append(
                {
                    "name": name,
                    "version": version,
                    "license": known_swift_license(identity, url, raw),
                    "source": url,
                    "ecosystem": "Swift",
                    "first_party": "false",
                    "package_root": package_path,
                    "license_file": "",
                }
            )
        dependencies = raw.get("dependencies")
        if not isinstance(dependencies, list):
            raise ComplianceError(f"Swift dependency list is malformed for {identity}")
        for dependency in dependencies:
            visit(dependency)

    root_dependencies = metadata.get("dependencies")
    if not isinstance(root_dependencies, list):
        raise ComplianceError("Swift metadata must contain dependencies")
    for dependency in root_dependencies:
        visit(dependency)
    return packages


LICENSE_FILE_PATTERN = re.compile(
    r"^(?:license|licence|copying|notice|copyright|unlicense|authors)(?:[._-].*)?$", re.IGNORECASE
)


def package_directory_name(item: dict[str, str]) -> str:
    values = (item["ecosystem"], item["name"], item["version"])
    parts = [re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-") for value in values]
    if not all(parts):
        raise ComplianceError(f"cannot create license directory name for {item['name']}")
    return "-".join(parts)


def collect_override_files(item: dict[str, str], override_root: Path) -> list[tuple[Path, Path]]:
    directory = override_root / package_directory_name(item)
    provenance_path = directory / "SOURCE.json"
    if directory.is_symlink() or not directory.is_dir() or provenance_path.is_symlink() or not provenance_path.is_file():
        raise ComplianceError(f"no distributable license or notice text found for {item['name']}")
    provenance = read_json(provenance_path)
    expected_identity = {
        "ecosystem": item["ecosystem"],
        "package": item["name"],
        "version": item["version"],
    }
    if any(provenance.get(key) != value for key, value in expected_identity.items()):
        raise ComplianceError(f"license override identity mismatch for {item['name']}")
    require_text(provenance.get("upstream_commit"), f"override commit for {item['name']}")
    require_text(provenance.get("upstream_url"), f"override URL for {item['name']}")
    expected_files = provenance.get("files")
    if not isinstance(expected_files, dict) or not expected_files:
        raise ComplianceError(f"license override file manifest is missing for {item['name']}")
    collected: list[tuple[Path, Path]] = []
    for relative_text, expected_sha256 in sorted(expected_files.items()):
        relative = Path(require_text(relative_text, f"override file path for {item['name']}"))
        if relative.is_absolute() or ".." in relative.parts:
            raise ComplianceError(f"license override path is unsafe for {item['name']}")
        source = directory / relative
        if source.is_symlink() or not source.is_file():
            raise ComplianceError(f"license override file is missing for {item['name']}")
        expected_digest = require_text(expected_sha256, f"override SHA-256 for {item['name']}")
        actual_digest = hashlib.sha256(source.read_bytes()).hexdigest()
        if not re.fullmatch(r"[0-9a-f]{64}", expected_digest) or actual_digest != expected_digest:
            raise ComplianceError(f"license override digest mismatch for {item['name']}")
        collected.append((source, relative))
    actual_files = {
        path.relative_to(directory).as_posix()
        for path in directory.rglob("*")
        if path.is_file() and path != provenance_path
    }
    if actual_files != {relative.as_posix() for _, relative in collected}:
        raise ComplianceError(f"license override contains unmanifested files for {item['name']}")
    collected.append((provenance_path, Path("SOURCE.json")))
    return collected


def collect_license_files(item: dict[str, str], override_root: Path | None) -> list[tuple[Path, Path]]:
    root = Path(item["package_root"])
    if root.is_symlink() or not root.is_dir():
        raise ComplianceError(f"dependency source directory is missing or symlinked for {item['name']}")
    resolved_root = root.resolve()
    candidates: set[Path] = set()
    declared = item.get("license_file", "")
    if declared:
        declared_path = Path(declared)
        if not declared_path.is_absolute():
            declared_path = root / declared_path
        candidates.add(declared_path)
    for child in root.iterdir():
        if child.is_file() and LICENSE_FILE_PATTERN.match(child.name):
            candidates.add(child)
        elif child.is_dir() and not child.is_symlink() and child.name.casefold() in {"license", "licenses"}:
            for nested in child.rglob("*"):
                if nested.is_file():
                    candidates.add(nested)
    collected: list[tuple[Path, Path]] = []
    for candidate in sorted(candidates, key=lambda path: str(path)):
        if candidate.is_symlink() or not candidate.is_file():
            raise ComplianceError(f"license text is missing, symlinked, or not a file for {item['name']}")
        resolved = candidate.resolve()
        try:
            relative = resolved.relative_to(resolved_root)
        except ValueError as error:
            raise ComplianceError(f"license text escapes dependency source for {item['name']}") from error
        if not candidate.read_bytes():
            raise ComplianceError(f"license text is empty for {item['name']}")
        collected.append((candidate, relative))
    if not collected:
        if override_root is None:
            raise ComplianceError(f"no distributable license or notice text found for {item['name']}")
        return collect_override_files(item, override_root)
    return collected


def digest_component(path: Path) -> str:
    digest = hashlib.sha256()
    if path.is_symlink():
        raise ComplianceError(f"component path must not be a symlink: {path.name}")
    if path.is_file():
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()
    if not path.is_dir():
        raise ComplianceError(f"component path is missing or unsupported: {path.name}")
    entries = sorted(path.rglob("*"), key=lambda item: item.relative_to(path).as_posix())
    for entry in entries:
        relative = entry.relative_to(path).as_posix().encode("utf-8")
        mode = stat.S_IMODE(entry.lstat().st_mode)
        if entry.is_symlink():
            kind = b"L"
            payload = os.readlink(entry).encode("utf-8")
        elif entry.is_file():
            kind = b"F"
            payload = hashlib.sha256(entry.read_bytes()).digest()
        elif entry.is_dir():
            kind = b"D"
            payload = b""
        else:
            raise ComplianceError(f"unsupported component entry: {entry.name}")
        digest.update(kind + b"\0" + relative + b"\0" + str(mode).encode("ascii") + b"\0" + payload)
    return digest.hexdigest()


def spdx_id(prefix: str, *values: str) -> str:
    readable = re.sub(r"[^A-Za-z0-9.-]+", "-", values[0]).strip("-") or "item"
    suffix = hashlib.sha256("\0".join(values).encode("utf-8")).hexdigest()[:12]
    return f"SPDXRef-{prefix}-{readable}-{suffix}"


def created_timestamp() -> str:
    raw_epoch = os.environ.get("SOURCE_DATE_EPOCH", "0")
    try:
        epoch = int(raw_epoch)
    except ValueError as error:
        raise ComplianceError("SOURCE_DATE_EPOCH must be an integer") from error
    if epoch < 0:
        raise ComplianceError("SOURCE_DATE_EPOCH must not be negative")
    return dt.datetime.fromtimestamp(epoch, tz=dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def build_documents(
    cargo: dict[str, Any],
    swift: dict[str, Any],
    components: list[tuple[str, Path]],
    license_overrides: Path | None,
) -> tuple[dict[str, Any], str, list[tuple[Path, Path]]]:
    raw_packages = rust_packages(cargo) + swift_packages(swift)
    package_keys: set[tuple[str, str, str]] = set()
    package_rows: list[dict[str, Any]] = []
    notices: list[dict[str, str]] = []
    bundled_licenses: list[tuple[Path, Path]] = []
    for item in sorted(raw_packages, key=lambda row: (row["name"], row["version"], row["ecosystem"])):
        key = (item["name"], item["version"], item["ecosystem"])
        if key in package_keys:
            continue
        package_keys.add(key)
        package_id = spdx_id("Package", *key)
        package_rows.append(
            {
                "SPDXID": package_id,
                "name": item["name"],
                "versionInfo": item["version"],
                "downloadLocation": item["source"] if item["source"] != "workspace" else "NOASSERTION",
                "filesAnalyzed": False,
                "licenseConcluded": item["license"],
                "licenseDeclared": item["license"],
                "copyrightText": "NOASSERTION",
                "supplier": "NOASSERTION",
            }
        )
        if item["first_party"] == "false":
            license_files = collect_license_files(item, license_overrides)
            destination_root = Path("third-party-licenses") / package_directory_name(item)
            notice = dict(item)
            notice["license_paths"] = ", ".join(
                (destination_root / relative).as_posix() for _, relative in license_files
            )
            notices.append(notice)
            bundled_licenses.extend((source, destination_root / relative) for source, relative in license_files)

    files: list[dict[str, Any]] = []
    for name, path in sorted(components, key=lambda component: component[0]):
        checksum = digest_component(path)
        files.append(
            {
                "SPDXID": spdx_id("File", name, checksum),
                "fileName": name,
                "checksums": [{"algorithm": "SHA256", "checksumValue": checksum}],
                "licenseConcluded": "MIT OR Apache-2.0",
                "licenseInfoInFiles": ["NOASSERTION"],
                "copyrightText": "NOASSERTION",
            }
        )

    seed = json.dumps({"packages": package_rows, "files": files}, sort_keys=True, separators=(",", ":"))
    namespace_digest = hashlib.sha256(seed.encode("utf-8")).hexdigest()
    relationships = [
        {"spdxElementId": "SPDXRef-DOCUMENT", "relationshipType": "DESCRIBES", "relatedSpdxElement": item["SPDXID"]}
        for item in package_rows + files
    ]
    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "Patchwright release SBOM",
        "documentNamespace": f"https://github.com/rsitech-ai/patchwright/spdx/{namespace_digest}",
        "creationInfo": {"created": created_timestamp(), "creators": ["Tool: Patchwright compliance generator"]},
        "packages": package_rows,
        "files": files,
        "relationships": relationships,
    }

    notice_lines = [
        "# Third-Party Notices",
        "",
        "This file is generated from the exact locked Rust and Swift dependency metadata used for this release.",
        "The declared license expressions below are metadata, not a replacement for each dependency's license text.",
        "",
        "| Package | Version | Declared license | Ecosystem | Source | Bundled license and notice files |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for item in sorted(notices, key=lambda row: (row["name"].casefold(), row["version"], row["ecosystem"])):
        cells = [
            item[key].replace("|", "\\|")
            for key in ("name", "version", "license", "ecosystem", "source", "license_paths")
        ]
        notice_lines.append(f"| {' | '.join(cells)} |")
    notice_lines.extend(
        [
            "",
            "Patchwright does not bundle Codex. It discovers a separately installed, signed-in Codex executable at runtime.",
            "",
        ]
    )
    return document, "\n".join(notice_lines), bundled_licenses


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-metadata", required=True, type=Path)
    parser.add_argument("--swift-metadata", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--engine", required=True, type=Path)
    parser.add_argument("--relay", required=True, type=Path)
    parser.add_argument("--license-overrides", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        document, notices, bundled_licenses = build_documents(
            read_json(args.cargo_metadata),
            read_json(args.swift_metadata),
            [("Patchwright.app", args.app), ("patchwright-engine", args.engine), ("patchwright-relay", args.relay)],
            args.license_overrides,
        )
        args.output_dir.mkdir(parents=True, exist_ok=True)
        (args.output_dir / "sbom.spdx.json").write_text(
            json.dumps(document, indent=2, sort_keys=False, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        (args.output_dir / "third-party-notices.md").write_text(notices, encoding="utf-8")
        license_root = args.output_dir / "third-party-licenses"
        if license_root.is_symlink():
            raise ComplianceError("third-party license output directory must not be a symlink")
        if license_root.exists():
            shutil.rmtree(license_root)
        for source, destination in bundled_licenses:
            output = args.output_dir / destination
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(source.read_bytes())
    except (ComplianceError, OSError) as error:
        print(f"compliance generation failed: {error}", file=sys.stderr)
        return 65
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
