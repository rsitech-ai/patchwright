#!/usr/bin/env python3
"""Verify final release Git state and its recorded source archive digest."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path


class SourceError(ValueError):
    pass


def git(repo: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *arguments],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SourceError(f"Git command failed: {' '.join(arguments)}")
    return result.stdout.strip()


def verified_release_archive_content_digest(path: Path, expected_digest: str) -> str:
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        raise SourceError(f"missing source archive: {path}") from error
    if not stat.S_ISREG(before.st_mode) or path.is_symlink():
        raise SourceError("source archive must be a regular non-symlink file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    archive_digest = hashlib.sha256()
    content_digest = hashlib.sha256()
    try:
        opened = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
            raise SourceError("source archive changed while opening")
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            archive_digest.update(chunk)
        if archive_digest.hexdigest() != expected_digest:
            raise SourceError("source archive digest mismatch")
        os.lseek(descriptor, 0, os.SEEK_SET)
        try:
            with os.fdopen(os.dup(descriptor), "rb") as compressed:
                with gzip.GzipFile(fileobj=compressed, mode="rb") as archive:
                    while True:
                        chunk = archive.read(1024 * 1024)
                        if not chunk:
                            break
                        content_digest.update(chunk)
        except (EOFError, gzip.BadGzipFile) as error:
            raise SourceError("source archive must be a valid gzip archive") from error
        after = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
        ):
            raise SourceError("source archive changed during verification")
    finally:
        os.close(descriptor)
    return content_digest.hexdigest()


def git_archive_content_digest(repo: Path, commit: str) -> str:
    result = hashlib.sha256()
    with tempfile.TemporaryFile() as error_log:
        process = subprocess.Popen(
            ["git", "-C", str(repo), "archive", "--format=tar", commit],
            stdout=subprocess.PIPE,
            stderr=error_log,
        )
        if process.stdout is None:
            process.kill()
            process.wait()
            raise SourceError("Git archive did not provide a content stream")
        try:
            while True:
                chunk = process.stdout.read(1024 * 1024)
                if not chunk:
                    break
                result.update(chunk)
        finally:
            process.stdout.close()
        returncode = process.wait()
        if returncode != 0:
            raise SourceError(f"Git command failed: archive --format=tar {commit}")
    return result.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-archive", required=True, type=Path)
    parser.add_argument("--source-archive-sha256", required=True)
    arguments = parser.parse_args()
    try:
        if not re.fullmatch(r"[0-9a-f]{40}", arguments.commit):
            raise SourceError("candidate commit is not canonical")
        if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", arguments.tag):
            raise SourceError("candidate tag is not canonical")
        if not re.fullmatch(r"[0-9a-f]{64}", arguments.source_archive_sha256):
            raise SourceError("source archive digest is not canonical")
        repo = arguments.repo.resolve(strict=True)
        if git(repo, "rev-parse", "HEAD") != arguments.commit:
            raise SourceError("release HEAD differs from candidate commit")
        if git(repo, "rev-parse", f"refs/tags/{arguments.tag}^{{commit}}") != arguments.commit:
            raise SourceError("release tag differs from candidate commit")
        if subprocess.run(["git", "-C", str(repo), "diff", "--quiet", "--exit-code", arguments.commit, "--"], check=False).returncode != 0:
            raise SourceError("release worktree differs from candidate commit")
        if subprocess.run(["git", "-C", str(repo), "diff", "--cached", "--quiet", "--exit-code", arguments.commit, "--"], check=False).returncode != 0:
            raise SourceError("release index differs from candidate commit")
        if git(repo, "ls-files", "--others", "--exclude-standard", "-z"):
            raise SourceError("release worktree contains untracked files")
        content_digest = verified_release_archive_content_digest(
            arguments.source_archive, arguments.source_archive_sha256
        )
        if content_digest != git_archive_content_digest(repo, arguments.commit):
            raise SourceError("source archive content differs from candidate commit")
    except (SourceError, OSError) as error:
        print(f"release source verification failed: {error}", file=sys.stderr)
        return 65
    print(f"release source verified: {arguments.commit} {arguments.source_archive_sha256}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
