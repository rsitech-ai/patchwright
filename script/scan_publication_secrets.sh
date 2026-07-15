#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --repo PATH --artifact-root PATH --output PATH" >&2
  exit 64
}

REPO=""
ARTIFACT_ROOT=""
OUTPUT=""
while (($#)); do
  case "$1" in
    --repo) [[ $# -ge 2 ]] || usage; REPO="$2"; shift 2 ;;
    --artifact-root) [[ $# -ge 2 ]] || usage; ARTIFACT_ROOT="$2"; shift 2 ;;
    --output) [[ $# -ge 2 ]] || usage; OUTPUT="$2"; shift 2 ;;
    *) usage ;;
  esac
done

[[ -n "$REPO" && -n "$ARTIFACT_ROOT" && -n "$OUTPUT" ]] || usage
[[ -d "$REPO/.git" || -f "$REPO/.git" ]] || { echo "secret scan failed: repository is not a Git worktree" >&2; exit 65; }
[[ -d "$ARTIFACT_ROOT" && ! -L "$ARTIFACT_ROOT" ]] || { echo "secret scan failed: artifact root is missing or symlinked" >&2; exit 65; }
[[ ! -e "$OUTPUT" || ( -f "$OUTPUT" && ! -L "$OUTPUT" ) ]] || { echo "secret scan failed: output must be a regular file" >&2; exit 65; }
mkdir -p "$(dirname "$OUTPUT")"

python3 - "$REPO" "$ARTIFACT_ROOT" "$OUTPUT" <<'PY'
from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path


repo = Path(sys.argv[1]).resolve()
artifact_root = Path(sys.argv[2]).resolve()
output = Path(sys.argv[3]).resolve()


def pattern(*parts: bytes) -> bytes:
    return b"".join(parts)


rules = [
    ("github-personal-access-token", re.compile(pattern(b"github", b"_pat_", b"[A-Za-z0-9_]{20,}"))),
    ("github-legacy-token", re.compile(pattern(b"gh", b"[pousr]", b"_[A-Za-z0-9]{30,}"))),
    (
        "pem-private-key",
        re.compile(
            pattern(
                b"-----BEGIN ",
                b"(?:RSA |EC |OPENSSH )?",
                b"PRIVATE KEY-----\\s+",
                b"[A-Za-z0-9+/=\\r\\n]{40,}",
                b"-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
            )
        ),
    ),
    ("openai-api-key", re.compile(pattern(b"sk", b"-(?:proj-|svcacct-)?", b"[A-Za-z0-9_-]{20,}"))),
    (
        "assigned-webhook-secret",
        re.compile(pattern(b"(?i)(?:webhook|client)[_-]?secret", b"[\"']?\\s*[:=]\\s*[\"']", b"[A-Za-z0-9_./+=-]{20,}")),
    ),
    ("slack-webhook", re.compile(pattern(b"https://hooks", b"\\.slack\\.com/services/", b"[A-Za-z0-9/_-]{20,}"))),
    ("discord-webhook", re.compile(pattern(b"https://discord", b"(?:app)?\\.com/api/webhooks/", b"[0-9]+/[A-Za-z0-9._-]{20,}"))),
]

findings: set[tuple[str, str, str]] = set()
counts = {"tracked_files": 0, "history_blobs": 0, "artifact_files": 0}


def scan_bytes(scope: str, locator: str, payload: bytes) -> None:
    locator_hash = hashlib.sha256(f"{scope}\0{locator}".encode("utf-8", "surrogateescape")).hexdigest()
    for rule_name, expression in rules:
        if expression.search(payload):
            findings.add((scope, locator_hash, rule_name))


def git(*args: str, input_data: bytes | None = None) -> bytes:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        input=input_data,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Git command failed: {' '.join(args)}")
    return result.stdout


try:
    tracked = [item for item in git("ls-files", "-z").split(b"\0") if item]
    for raw_relative in sorted(tracked):
        relative = raw_relative.decode("utf-8", "surrogateescape")
        path = repo / relative
        if path.is_symlink():
            payload = os.readlink(path).encode("utf-8", "surrogateescape")
        elif path.is_file():
            payload = path.read_bytes()
        else:
            payload = b""
        scan_bytes("tracked", relative, payload)
        counts["tracked_files"] += 1

    object_lines = git("rev-list", "--objects", "--all").splitlines()
    object_ids = sorted({line.split(b" ", 1)[0].decode("ascii") for line in object_lines if line})
    for object_id in object_ids:
        if git("cat-file", "-t", object_id).strip() != b"blob":
            continue
        scan_bytes("history", object_id, git("cat-file", "blob", object_id))
        counts["history_blobs"] += 1

    for path in sorted(artifact_root.rglob("*"), key=lambda item: item.relative_to(artifact_root).as_posix()):
        if path.resolve() == output:
            continue
        if path.is_symlink():
            payload = os.readlink(path).encode("utf-8", "surrogateescape")
        elif path.is_file():
            payload = path.read_bytes()
        else:
            continue
        relative = path.relative_to(artifact_root).as_posix()
        scan_bytes("artifact", relative, payload)
        counts["artifact_files"] += 1
except (OSError, RuntimeError, UnicodeError) as error:
    print(f"secret scan failed: {error}", file=sys.stderr)
    raise SystemExit(65)

finding_rows = [
    {"scope": scope, "locator_sha256": locator_hash, "rule": rule_name}
    for scope, locator_hash, rule_name in sorted(findings)
]
result = {
    "schema_version": 1,
    "clean": not finding_rows,
    "scanned": counts,
    "findings": finding_rows,
}
temporary = output.with_name(output.name + ".tmp")
temporary.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(temporary, output)
if finding_rows:
    print(f"secret scan rejected publication material: {len(finding_rows)} redacted finding(s)", file=sys.stderr)
    raise SystemExit(65)
print(
    "secret scan passed: "
    f"{counts['tracked_files']} tracked files, {counts['history_blobs']} history blobs, "
    f"{counts['artifact_files']} artifact files"
)
PY
