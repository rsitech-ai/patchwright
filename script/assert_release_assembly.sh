#!/usr/bin/env bash
set -euo pipefail

ASSEMBLY_JSON="${1:-}"
[[ -f "$ASSEMBLY_JSON" && ! -L "$ASSEMBLY_JSON" ]] || {
  echo "release assembly evidence is missing or symlinked" >&2
  exit 65
}
if ! jq -e '.dirty == false and .candidate == true' "$ASSEMBLY_JSON" >/dev/null 2>&1; then
  echo "release assembly is not a clean candidate" >&2
  exit 65
fi
