#!/usr/bin/env python3
"""Generate or verify a deterministic manifest of release-root symlinks."""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any


class SymlinkManifestError(ValueError):
    pass


def build_manifest(root: Path) -> dict[str, Any]:
    if root.is_symlink() or not root.is_dir():
        raise SymlinkManifestError("release root must be a real directory")
    resolved_root = root.resolve(strict=True)
    links: list[dict[str, str]] = []
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        if not path.is_symlink():
            continue
        target = os.readlink(path)
        if Path(target).is_absolute():
            raise SymlinkManifestError(f"absolute symlink is not reproducible: {path.relative_to(root)}")
        try:
            resolved_target = path.resolve(strict=True)
            resolved_target.relative_to(resolved_root)
        except (OSError, ValueError) as error:
            raise SymlinkManifestError(f"symlink is dangling or escapes release root: {path.relative_to(root)}") from error
        links.append({"path": path.relative_to(root).as_posix(), "target": target})
    return {"schema_version": 1, "links": links}


def read_manifest(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise SymlinkManifestError("symlink manifest must be a regular file")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SymlinkManifestError(f"invalid symlink manifest: {error}") from error
    if not isinstance(value, dict):
        raise SymlinkManifestError("symlink manifest root must be an object")
    return value


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path)
    mode.add_argument("--verify", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        actual = build_manifest(args.root)
        if args.verify is not None:
            expected = read_manifest(args.verify)
            if expected != actual:
                raise SymlinkManifestError("release symlinks do not match the recorded manifest")
        else:
            output: Path = args.output
            if output.is_symlink():
                raise SymlinkManifestError("symlink manifest output must not be a symlink")
            output.parent.mkdir(parents=True, exist_ok=True)
            temporary = output.with_name(output.name + ".tmp")
            if temporary.exists() or temporary.is_symlink():
                raise SymlinkManifestError("temporary symlink manifest path already exists")
            temporary.write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            os.replace(temporary, output)
    except (OSError, SymlinkManifestError) as error:
        print(f"symlink manifest failed: {error}", file=sys.stderr)
        return 65
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
