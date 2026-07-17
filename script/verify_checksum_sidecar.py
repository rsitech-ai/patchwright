#!/usr/bin/env python3
"""Read-only, no-follow verification for a portable SHA-256 sidecar."""

from __future__ import annotations

import argparse
import hashlib
import os
import stat
import sys
from pathlib import Path


class ChecksumError(ValueError):
    pass


def read_regular(path: Path, label: str, limit: int | None = None) -> bytes:
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        raise ChecksumError(f"missing {label}: {path}") from error
    if not stat.S_ISREG(before.st_mode) or path.is_symlink():
        raise ChecksumError(f"{label} must be a regular non-symlink file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        opened = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
            raise ChecksumError(f"{label} changed while opening")
        if limit is not None and opened.st_size > limit:
            raise ChecksumError(f"{label} exceeds {limit} bytes")
        chunks: list[bytes] = []
        remaining = opened.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        after = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
        ):
            raise ChecksumError(f"{label} changed during verification")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--sidecar", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        artifact = read_regular(arguments.artifact, "artifact")
        sidecar = read_regular(arguments.sidecar, "checksum sidecar", 512)
        digest = hashlib.sha256(artifact).hexdigest()
        expected = f"{digest}  {arguments.artifact.name}\n".encode("ascii")
        if sidecar != expected:
            raise ChecksumError("checksum sidecar digest does not match artifact")
    except (ChecksumError, OSError) as error:
        print(f"distribution verification failed: {error}", file=sys.stderr)
        return 65
    print(f"checksum verified: {arguments.artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
