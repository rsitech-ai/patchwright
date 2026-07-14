#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?release root required}"
[[ -s "$ROOT/evidence/build-metadata.json" ]] || { echo "missing build metadata" >&2; exit 65; }
[[ -s "$ROOT/evidence/SHA256SUMS" ]] || { echo "missing SHA256SUMS" >&2; exit 65; }
if rg -n --hidden -g '!SHA256SUMS' -e 'gh[op]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY|sk-[A-Za-z0-9]{20,}' "$ROOT"; then
  echo "credential-shaped material found in release root" >&2
  exit 65
fi
(
  cd "$ROOT"
  shasum -a 256 -c evidence/SHA256SUMS
)
echo "reproducibility bundle verified: $ROOT"
