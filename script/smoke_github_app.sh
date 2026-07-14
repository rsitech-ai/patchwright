#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_OWNER="${PATCHWRIGHT_GITHUB_E2E_OWNER:-}"
TARGET_REPOSITORY="${PATCHWRIGHT_GITHUB_E2E_REPOSITORY:-}"
TARGET_REPOSITORY_ID="${PATCHWRIGHT_GITHUB_E2E_REPOSITORY_ID:-}"
TARGET_INSTALLATION_ID="${PATCHWRIGHT_GITHUB_E2E_INSTALLATION_ID:-}"
TARGET="$TARGET_OWNER/$TARGET_REPOSITORY"
ALLOWLIST="${PATCHWRIGHT_GITHUB_E2E_ALLOWLIST:-}"
CONFIRMATION="${PATCHWRIGHT_GITHUB_E2E_CONFIRM:-}"
CONFIGURATION="${PATCHWRIGHT_GITHUB_APP_CONFIG:-$HOME/.patchwright/github-app.json}"

fail() {
  printf 'blocked:external — %s\n' "$1" >&2
  exit 78
}

[[ -n "$TARGET_OWNER" && -n "$TARGET_REPOSITORY" ]] || fail "set the disposable owner and repository"
[[ "$TARGET" != "s1korrrr/patchwright" ]] || fail "the Patchwright production repository is forbidden"
[[ "$TARGET" == "$ALLOWLIST" ]] || fail "PATCHWRIGHT_GITHUB_E2E_ALLOWLIST must exactly equal $TARGET"
[[ "$CONFIRMATION" == "authorize:$TARGET" ]] || fail "set PATCHWRIGHT_GITHUB_E2E_CONFIRM=authorize:$TARGET for this one run"
[[ "$TARGET_REPOSITORY_ID" =~ ^[1-9][0-9]*$ ]] || fail "set the numeric disposable repository ID"
[[ "$TARGET_INSTALLATION_ID" =~ ^[1-9][0-9]*$ ]] || fail "set the numeric GitHub App installation ID"
[[ -f "$CONFIGURATION" && ! -L "$CONFIGURATION" ]] || fail "GitHub App metadata is missing or symlinked"
[[ "$(stat -f '%Lp' "$CONFIGURATION")" == 600 ]] || fail "GitHub App metadata must have mode 600"

APP_ID="$(jq -er '.appId | select(type == "number" and . > 0)' "$CONFIGURATION")" || fail "App ID is missing"
CLIENT_ID="$(jq -er '.clientId | select(type == "string" and length > 0)' "$CONFIGURATION")" || fail "Client ID is missing"
KEY_REFERENCE="$(jq -er '.keyReference | select(type == "string" and startswith("keychain:"))' "$CONFIGURATION")" || fail "Keychain reference is missing"
KEYCHAIN_PATH="${KEY_REFERENCE#keychain:}"
KEYCHAIN_SERVICE="${KEYCHAIN_PATH%/*}"
KEYCHAIN_ACCOUNT="${KEYCHAIN_PATH##*/}"
[[ -n "$KEYCHAIN_SERVICE" && -n "$KEYCHAIN_ACCOUNT" ]] || fail "Keychain reference is invalid"
security find-generic-password -s "$KEYCHAIN_SERVICE" -a "$KEYCHAIN_ACCOUNT" >/dev/null 2>&1 \
  || fail "the referenced GitHub App private key is unavailable in Keychain"

LIVE_REPOSITORY_ID="$(gh api "repos/$TARGET" --jq .id 2>/dev/null)" \
  || fail "the disposable repository is not readable with the current development credential"
[[ "$LIVE_REPOSITORY_ID" == "$TARGET_REPOSITORY_ID" ]] || fail "repository identity does not match the allowlisted ID"

printf 'GitHub App E2E target: %s (repository %s, installation %s, app %s, client %s)\n' \
  "$TARGET" "$TARGET_REPOSITORY_ID" "$TARGET_INSTALLATION_ID" "$APP_ID" "$CLIENT_ID"

cd "$ROOT_DIR"
cargo test -p patchwright-relay --test app_auth --test installation_tokens --test mutations
cargo test -p patchwright-engine --test delivery_flow --test monitoring_flow --test queue_recovery

fail "local and credential preflight passed; the authorized remote mutation sequence has not yet been run"
