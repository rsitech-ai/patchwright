#!/usr/bin/env bash
set -euo pipefail

APP_PATH=""
DMG_PATH=""
JSON_PATH=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) APP_PATH="${2:?}"; shift 2 ;;
    --dmg) DMG_PATH="${2:?}"; shift 2 ;;
    --json) JSON_PATH="${2:?}"; shift 2 ;;
    *) echo "usage: $0 --app PATH [--dmg PATH] --json PATH" >&2; exit 64 ;;
  esac
done
[[ -n "$APP_PATH" && -n "$JSON_PATH" ]] || { echo "app and json paths are required" >&2; exit 64; }
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
repo_ready=false
codex_integration=false
github_integration=false
bundle_valid=false
developer_id=false
hardened_runtime=false
package_ready=false
notarized=false
gatekeeper=false
clean_machine=false
[[ "${PATCHWRIGHT_REPO_VERIFIED:-0}" == 1 ]] && repo_ready=true
[[ "${PATCHWRIGHT_CODEX_VERIFIED:-0}" == 1 ]] && codex_integration=true
[[ "${PATCHWRIGHT_GITHUB_VERIFIED:-0}" == 1 ]] && github_integration=true
if "$ROOT_DIR/script/validate_bundle.sh" "$APP_PATH" >/dev/null 2>&1; then bundle_valid=true; fi
if "$ROOT_DIR/script/verify_signing.sh" "$APP_PATH" >/dev/null 2>&1; then
  developer_id=true
  hardened_runtime=true
  package_ready=true
fi
if xcrun stapler validate "$APP_PATH" >/dev/null 2>&1; then notarized=true; fi
if /usr/sbin/spctl --assess --type execute "$APP_PATH" >/dev/null 2>&1; then gatekeeper=true; fi
if [[ "${PATCHWRIGHT_CLEAN_MACHINE_VERIFIED:-0}" == 1 ]]; then clean_machine=true; fi
if [[ -n "$DMG_PATH" ]]; then
  if ! xcrun stapler validate "$DMG_PATH" >/dev/null 2>&1; then notarized=false; fi
  if ! /usr/sbin/spctl --assess --type open --context context:primary-signature "$DMG_PATH" >/dev/null 2>&1; then gatekeeper=false; fi
fi
release_candidate_ready=false
if [[ "$repo_ready" == true && "$codex_integration" == true && "$github_integration" == true && "$bundle_valid" == true && "$developer_id" == true && "$hardened_runtime" == true && "$notarized" == true && "$gatekeeper" == true && "$clean_machine" == true ]]; then
  release_candidate_ready=true
fi
mkdir -p "$(dirname "$JSON_PATH")"
jq -n \
  --argjson repo_ready "$repo_ready" \
  --argjson codex_integration "$codex_integration" \
  --argjson github_integration "$github_integration" \
  --argjson bundle_valid "$bundle_valid" \
  --argjson developer_id "$developer_id" \
  --argjson hardened_runtime "$hardened_runtime" \
  --argjson package_ready "$package_ready" \
  --argjson notarized "$notarized" \
  --argjson gatekeeper "$gatekeeper" \
  --argjson clean_machine "$clean_machine" \
  --argjson release_candidate_ready "$release_candidate_ready" \
  --arg app "$APP_PATH" --arg dmg "$DMG_PATH" \
  '{repo_ready:$repo_ready,integration_ready:{codex:$codex_integration,github_delivery_merge:$github_integration},bundle_valid:$bundle_valid,developer_id:$developer_id,hardened_runtime:$hardened_runtime,package_ready:$package_ready,notarized:$notarized,gatekeeper:$gatekeeper,clean_machine:$clean_machine,release_candidate_ready:$release_candidate_ready,evidence:{app:$app,dmg:$dmg}}' \
  >"$JSON_PATH"
if [[ "$release_candidate_ready" == true ]]; then
  echo "release-candidate ready"
  exit 0
fi
echo "blocked:external — inspect $JSON_PATH for independent release gates" >&2
exit 78
