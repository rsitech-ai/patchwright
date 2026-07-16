#!/usr/bin/env bash
set -euo pipefail

DMG_PATH="${1:?notarized DMG required}"
EVIDENCE_DIR="${2:?evidence directory required}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ "${PATCHWRIGHT_CLEAN_MACHINE:-0}" == 1 ]] || {
  echo "blocked:external — run only in the documented clean macOS 26+ VM with PATCHWRIGHT_CLEAN_MACHINE=1" >&2
  exit 78
}
mkdir -p "$EVIDENCE_DIR"
"$ROOT_DIR/script/verify_distribution.sh" "$DMG_PATH" | tee "$EVIDENCE_DIR/distribution.txt"
{
  sw_vers
  uname -a
  sysctl -n machdep.cpu.brand_string 2>/dev/null || true
  system_profiler SPHardwareDataType | sed -n '1,40p'
  command -v git || true
  command -v gh || true
  command -v codex || true
} >"$EVIDENCE_DIR/machine.txt"
if [[ -e "$HOME/.patchwright/patchwright.sqlite3" || -e "$HOME/.patchwright/engine.sock" ]]; then
  echo "clean-machine probe refused: prior Patchwright state exists" >&2
  exit 65
fi
MOUNT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-clean-mount.XXXXXX")"
INSTALL_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/patchwright-clean-install.XXXXXX")"
trap '/usr/bin/hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true; rm -rf "$INSTALL_ROOT"; rmdir "$MOUNT" 2>/dev/null || true' EXIT
/usr/bin/hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$DMG_PATH"
/usr/bin/ditto "$MOUNT/Patchwright.app" "$INSTALL_ROOT/Patchwright.app"
/usr/sbin/spctl --assess --type execute --verbose=4 "$INSTALL_ROOT/Patchwright.app" 2>&1 | tee "$EVIDENCE_DIR/gatekeeper.txt"
/usr/bin/open -n "$INSTALL_ROOT/Patchwright.app"
for _ in {1..100}; do pgrep -x Patchwright >/dev/null && break; sleep 0.1; done
pgrep -x Patchwright >/dev/null || { echo "first launch failed" >&2; exit 65; }
pkill -x Patchwright
for _ in {1..100}; do ! pgrep -x Patchwright >/dev/null && break; sleep 0.1; done
/usr/bin/open -n "$INSTALL_ROOT/Patchwright.app"
for _ in {1..100}; do pgrep -x Patchwright >/dev/null && break; sleep 0.1; done
pgrep -x Patchwright >/dev/null || { echo "relaunch failed" >&2; exit 65; }
pkill -x Patchwright
echo "clean-machine base install/launch/relaunch probe passed; continue the account-state matrix in docs/clean-machine-test-plan.md" | tee "$EVIDENCE_DIR/result.txt"
