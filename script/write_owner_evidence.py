#!/usr/bin/env python3
"""Atomically create JSON evidence in a canonical owner-only directory."""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
import uuid
from pathlib import Path
from typing import Any


class EvidenceError(ValueError):
    pass


def duplicate_free(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise EvidenceError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def owner_directory(path: Path) -> int:
    if not path.is_absolute() or ".." in path.parts:
        raise EvidenceError("evidence directory must be an absolute canonical path")
    try:
        direct = path.lstat()
    except FileNotFoundError:
        direct = None
    if direct is not None and stat.S_ISLNK(direct.st_mode):
        raise EvidenceError("evidence directory must not be a symlink")
    resolved = path.resolve(strict=False)
    if resolved != path:
        raise EvidenceError("evidence directory must be an absolute canonical path")
    if direct is None:
        parent = path.parent
        try:
            parent_metadata = parent.lstat()
        except FileNotFoundError as error:
            raise EvidenceError("evidence directory parent must already exist") from error
        if stat.S_ISLNK(parent_metadata.st_mode) or not stat.S_ISDIR(parent_metadata.st_mode):
            raise EvidenceError("evidence directory parent must be a real directory")
        if parent_metadata.st_uid != os.getuid() or stat.S_IMODE(parent_metadata.st_mode) != 0o700:
            raise EvidenceError("evidence directory parent must be owner-only mode 700")
        parent_descriptor = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0))
        try:
            os.mkdir(path.name, 0o700, dir_fd=parent_descriptor)
        finally:
            os.close(parent_descriptor)
        direct = path.lstat()
    if not stat.S_ISDIR(direct.st_mode) or path.is_symlink():
        raise EvidenceError("evidence directory must be a real directory")
    if direct.st_uid != os.getuid():
        raise EvidenceError("evidence directory must be owned by the current user")
    if stat.S_IMODE(direct.st_mode) != 0o700:
        raise EvidenceError("evidence directory must have mode 700")
    return os.open(path, os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--name", required=True)
    arguments = parser.parse_args()
    directory_descriptor: int | None = None
    temporary_name: str | None = None
    try:
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,199}\.json", arguments.name):
            raise EvidenceError("evidence filename is invalid")
        payload = sys.stdin.buffer.read(4 * 1024 * 1024 + 1)
        if len(payload) > 4 * 1024 * 1024:
            raise EvidenceError("evidence JSON exceeds 4 MiB")
        try:
            value = json.loads(payload.decode("utf-8"), object_pairs_hook=duplicate_free)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvidenceError(f"evidence JSON is malformed: {error}") from error
        if not isinstance(value, dict):
            raise EvidenceError("evidence JSON root must be an object")
        encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
        directory_descriptor = owner_directory(arguments.directory)
        temporary_name = f".{arguments.name}.{uuid.uuid4().hex}.tmp"
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(temporary_name, flags, 0o600, dir_fd=directory_descriptor)
        try:
            os.fchmod(descriptor, 0o600)
            offset = 0
            while offset < len(encoded):
                offset += os.write(descriptor, encoded[offset:])
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        try:
            os.link(
                temporary_name,
                arguments.name,
                src_dir_fd=directory_descriptor,
                dst_dir_fd=directory_descriptor,
                follow_symlinks=False,
            )
        except FileExistsError as error:
            raise EvidenceError("evidence file already exists") from error
        os.unlink(temporary_name, dir_fd=directory_descriptor)
        temporary_name = None
        os.fsync(directory_descriptor)
    except (EvidenceError, OSError) as error:
        print(f"evidence creation failed: {error}", file=sys.stderr)
        return 65
    finally:
        if directory_descriptor is not None:
            if temporary_name is not None:
                try:
                    os.unlink(temporary_name, dir_fd=directory_descriptor)
                except FileNotFoundError:
                    pass
            os.close(directory_descriptor)
    print(str(arguments.directory / arguments.name))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
