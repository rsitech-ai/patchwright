#!/usr/bin/env python3
"""Validate an Apple notarization log and emit only sanitized counts/digest."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import sys
from pathlib import Path
from typing import Any


class NotaryLogError(ValueError):
    pass


def duplicate_free(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise NotaryLogError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_log(path: Path) -> bytes:
    try:
        before = path.lstat()
    except FileNotFoundError as error:
        raise NotaryLogError(f"notarization log is missing: {path}") from error
    if not stat.S_ISREG(before.st_mode) or path.is_symlink():
        raise NotaryLogError("notarization log must be a regular non-symlink file")
    if before.st_size > 16 * 1024 * 1024:
        raise NotaryLogError("notarization log exceeds 16 MiB")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        opened = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino):
            raise NotaryLogError("notarization log changed while opening")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(descriptor, 65536)
            if not chunk:
                break
            chunks.append(chunk)
        after = os.fstat(descriptor)
        if (opened.st_size, opened.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise NotaryLogError("notarization log changed during validation")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--warning-policy", choices=("reject", "allow"), default="reject")
    arguments = parser.parse_args()
    try:
        raw = read_log(arguments.log)
        try:
            document = json.loads(raw.decode("utf-8"), object_pairs_hook=duplicate_free)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise NotaryLogError(f"malformed notarization log: {error}") from error
        if not isinstance(document, dict) or not isinstance(document.get("issues"), list):
            raise NotaryLogError("notarization log must contain an issues array")
        counts = {"error": 0, "warning": 0, "info": 0}
        for index, issue in enumerate(document["issues"]):
            if not isinstance(issue, dict) or not isinstance(issue.get("severity"), str):
                raise NotaryLogError(f"notarization issue {index} has no severity")
            severity = issue["severity"].lower()
            if severity in {"notice", "informational"}:
                severity = "info"
            if severity not in counts:
                raise NotaryLogError(f"notarization issue {index} has unknown severity")
            counts[severity] += 1
        if counts["error"]:
            raise NotaryLogError(f"notarization log contains {counts['error']} error issue(s)")
        if arguments.warning_policy == "reject" and counts["warning"]:
            raise NotaryLogError(
                f"notarization warning policy rejected {counts['warning']} warning issue(s)"
            )
        summary = {
            "log_sha256": hashlib.sha256(raw).hexdigest(),
            "issue_count": sum(counts.values()),
            "error_count": counts["error"],
            "warning_count": counts["warning"],
            "info_count": counts["info"],
            "warning_policy": arguments.warning_policy,
        }
        print(json.dumps(summary, sort_keys=True, separators=(",", ":")))
    except (NotaryLogError, OSError) as error:
        print(f"notarization log rejected: {error}", file=sys.stderr)
        return 65
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
