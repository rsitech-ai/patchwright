#!/usr/bin/env bash
set -euo pipefail

for legacy in \
  PATCHWRIGHT_REPO_VERIFIED \
  PATCHWRIGHT_CODEX_VERIFIED \
  PATCHWRIGHT_GITHUB_VERIFIED \
  PATCHWRIGHT_CLEAN_MACHINE_VERIFIED; do
  if [[ -n "${!legacy+x}" ]]; then
    echo "legacy release evidence is unsupported: $legacy" >&2
    exit 64
  fi
done
for argument in "$@"; do
  if [[ "$argument" == --app || "$argument" == --dmg ]]; then
    echo "legacy release evidence is unsupported: $argument" >&2
    exit 64
  fi
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT_DIR/script/verify_release_evidence.py" promotion "$@"
