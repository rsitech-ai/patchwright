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

die() {
  printf 'GitHub App qualification failed — %s\n' "$1" >&2
  exit 1
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
KEY_REFERENCE="$(jq -er '.keyReference | select(type == "string" and length > 0)' "$CONFIGURATION")" || fail "private-key reference is missing"
case "$KEY_REFERENCE" in
  keychain:*)
    KEYCHAIN_PATH="${KEY_REFERENCE#keychain:}"
    KEYCHAIN_SERVICE="${KEYCHAIN_PATH%/*}"
    KEYCHAIN_ACCOUNT="${KEYCHAIN_PATH##*/}"
    [[ -n "$KEYCHAIN_SERVICE" && -n "$KEYCHAIN_ACCOUNT" ]] || fail "Keychain reference is invalid"
    security find-generic-password -s "$KEYCHAIN_SERVICE" -a "$KEYCHAIN_ACCOUNT" >/dev/null 2>&1 \
      || fail "the referenced GitHub App private key is unavailable in Keychain"
    ;;
  file:*)
    KEY_PATH="${KEY_REFERENCE#file:}"
    [[ "$KEY_PATH" == /* ]] || fail "the protected private-key path must be absolute"
    [[ -f "$KEY_PATH" && ! -L "$KEY_PATH" ]] || fail "the protected private-key file is missing or symlinked"
    KEY_MODE="$(stat -f '%Lp' "$KEY_PATH")"
    [[ "$KEY_MODE" == 400 || "$KEY_MODE" == 600 ]] || fail "the protected private-key file must have owner-only mode 400 or 600"
    ;;
  *)
    fail "private-key reference must use keychain: or file:"
    ;;
esac

LIVE_REPOSITORY_ID="$(gh api "repos/$TARGET" --jq .id 2>/dev/null)" \
  || fail "the disposable repository is not readable with the current development credential"
[[ "$LIVE_REPOSITORY_ID" == "$TARGET_REPOSITORY_ID" ]] || fail "repository identity does not match the allowlisted ID"

printf 'GitHub App E2E target: %s (repository %s, installation %s, app %s, client %s)\n' \
  "$TARGET" "$TARGET_REPOSITORY_ID" "$TARGET_INSTALLATION_ID" "$APP_ID" "$CLIENT_ID"

cd "$ROOT_DIR"
cargo build --release -p patchwright-relay
target/release/patchwright-relay github-app-health --config "$CONFIGURATION"
cargo test -p patchwright-relay --test app_auth --test installation_tokens --test mutations
cargo test -p patchwright-engine --test delivery_flow --test monitoring_flow --test queue_recovery

STATE_DIR="$(mktemp -d)"
SOCKET="$STATE_DIR/engine.sock"
DATABASE="$STATE_DIR/engine.sqlite3"
ENGINE_LOG="$STATE_DIR/engine.log"
cleanup() {
  local status=$?
  test -z "${ENGINE_PID:-}" || kill "$ENGINE_PID" >/dev/null 2>&1 || true
  if [[ "$status" == 0 ]]; then
    rm -rf "$STATE_DIR"
  else
    printf 'Local failure evidence retained: %s\n' "$STATE_DIR" >&2
  fi
  return "$status"
}
trap cleanup EXIT

rpc_result() {
  local method="$1"
  local params="$2"
  local request response
  request="$(jq -cn --arg method "$method" --argjson params "$params" \
    '{jsonrpc:"2.0",id:1,method:$method,params:$params}')"
  response="$(printf '%s\n' "$request" | nc -U "$SOCKET")" \
    || die "engine RPC transport failed for $method"
  if ! jq -e 'has("result") and (has("error") | not)' >/dev/null <<<"$response"; then
    jq -c '{method:"'"$method"'",error:.error}' <<<"$response" >&2 || true
    die "engine RPC rejected $method"
  fi
  jq -c '.result' <<<"$response"
}

deliver() {
  local task_id="$1"
  local action="$2"
  local expected_base_sha="$3"
  local expected_head_sha="$4"
  local generation="$5"
  local action_preview preview approval approval_id
  action_preview="$(jq -cn \
    --argjson remote "$REMOTE_IDENTITY" \
    --argjson action "$action" \
    --arg expectedBaseSha "$expected_base_sha" \
    --arg expectedHeadSha "$expected_head_sha" \
    --argjson snapshotGeneration "$generation" \
    '{remote:$remote,action:$action,expectedBaseSha:$expectedBaseSha,snapshotGeneration:$snapshotGeneration}
      + if $expectedHeadSha == "" then {} else {expectedHeadSha:$expectedHeadSha} end')"
  preview="$(rpc_result delivery.preview "$(jq -cn --arg taskId "$task_id" \
    --argjson actionPreview "$action_preview" '{taskId:$taskId,actionPreview:$actionPreview}')")"
  approval="$(rpc_result delivery.approve "$(jq -cn --arg approvedBy "owner-qualified:$TARGET" \
    --argjson preview "$preview" '{preview:$preview,approvedBy:$approvedBy}')")"
  approval_id="$(jq -er '.id' <<<"$approval")" || die "delivery approval ID is missing"
  rpc_result delivery.execute "$(jq -cn --arg approvalId "$approval_id" \
    --argjson preview "$preview" '{preview:$preview,approvalId:$approvalId}')"
}

mkdir -p "$STATE_DIR/repository" "$STATE_DIR/state" "$STATE_DIR/worktrees"
target/debug/patchwright-engine serve --socket "$SOCKET" --database "$DATABASE" \
  >"$ENGINE_LOG" 2>&1 &
ENGINE_PID=$!
for _ in {1..100}; do
  test -S "$SOCKET" && break
  sleep 0.05
done
test -S "$SOCKET" || die "engine socket did not become ready"
rpc_result system.health '{}' >/dev/null

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
BRANCH="patchwright/e2e-$RUN_ID"
MARKER="Patchwright GitHub App qualification $RUN_ID"
BASE_BRANCH="$(gh api "repos/$TARGET" --jq .default_branch)" \
  || die "default branch lookup failed"
BASE_SHA="$(gh api "repos/$TARGET/commits/$BASE_BRANCH" --jq .sha)" \
  || die "default branch commit lookup failed"
BASE_TREE="$(gh api "repos/$TARGET/git/commits/$BASE_SHA" --jq .tree.sha)" \
  || die "base tree lookup failed"
BLOB_SHA="$(gh api --method POST "repos/$TARGET/git/blobs" \
  -f content="$MARKER" -f encoding=utf-8 --jq .sha)" \
  || die "qualification blob creation failed"
TREE_SHA="$(jq -cn --arg base_tree "$BASE_TREE" --arg sha "$BLOB_SHA" \
  '{base_tree:$base_tree,tree:[{path:"patchwright-e2e.txt",mode:"100644",type:"blob",sha:$sha}]}' \
  | gh api --method POST "repos/$TARGET/git/trees" --input - --jq .sha)" \
  || die "qualification tree creation failed"
SEED_SHA="$(jq -cn --arg message "$MARKER" --arg tree "$TREE_SHA" --arg parent "$BASE_SHA" \
  '{message:$message,tree:$tree,parents:[$parent]}' \
  | gh api --method POST "repos/$TARGET/git/commits" --input - --jq .sha)" \
  || die "qualification commit creation failed"
ISSUE="$(gh api --method POST "repos/$TARGET/issues" \
  -f title="[Patchwright E2E] Approval-gated delivery" \
  -f body="Disposable private qualification fixture: $MARKER")" \
  || die "qualification issue creation failed"
ISSUE_NUMBER="$(jq -er .number <<<"$ISSUE")" || die "qualification issue number is missing"
ISSUE_UPDATED_AT="$(jq -er .updated_at <<<"$ISSUE")" || die "qualification issue timestamp is missing"
ISSUE_URL="$(jq -er .html_url <<<"$ISSUE")" || die "qualification issue URL is missing"

rpc_result github.sync '{"repositoryLimit":"100","resourceLimit":"1000"}' >/dev/null
SNAPSHOT="$(rpc_result github.repository "$(jq -cn --arg fullName "$TARGET" '{fullName:$fullName}')")"
jq -e --argjson issue "$ISSUE_NUMBER" \
  '.workItems[] | select(.number == $issue and .kind == "issue")' \
  >/dev/null <<<"$SNAPSHOT" || die "qualification issue was not ingested"
rpc_result repository.bind "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg installationId "$TARGET_INSTALLATION_ID" \
  --arg managedClone "$STATE_DIR/repository" \
  --arg stateRoot "$STATE_DIR/state" \
  --arg worktreeRoot "$STATE_DIR/worktrees" \
  '{repositoryFullName:$repositoryFullName,installationId:$installationId,
    managedClone:$managedClone,stateRoot:$stateRoot,worktreeRoot:$worktreeRoot}')" >/dev/null
ISSUE_TASK="$(rpc_result task.createFromGitHub "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg itemNumber "$ISSUE_NUMBER" \
  --arg expectedUpdatedAt "$ISSUE_UPDATED_AT" \
  '{repositoryFullName:$repositoryFullName,itemNumber:$itemNumber,expectedUpdatedAt:$expectedUpdatedAt}')")"
ISSUE_TASK_ID="$(jq -er '.task.id' <<<"$ISSUE_TASK")" || die "qualification issue task is missing"
REMOTE_IDENTITY="$(jq -cn \
  --argjson repositoryId "$TARGET_REPOSITORY_ID" \
  --argjson installationId "$TARGET_INSTALLATION_ID" \
  --arg repositoryFullName "$TARGET" \
  '{repositoryId:$repositoryId,installationId:$installationId,repositoryFullName:$repositoryFullName}')"

BRANCH_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --arg branch "$BRANCH" --arg fromSha "$SEED_SHA" \
  '{kind:"createBranch",branch:$branch,fromSha:$fromSha}')" "$BASE_SHA" "" 1)"
CHECK_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --arg headSha "$SEED_SHA" \
  '{kind:"checkRun",name:"Patchwright Qualification",headSha:$headSha,status:"completed",conclusion:"success"}')" \
  "$BASE_SHA" "" 1)"
COMMENT_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --argjson issueNumber "$ISSUE_NUMBER" --arg body "$MARKER" \
  '{kind:"comment",issueNumber:$issueNumber,body:$body}')" "$BASE_SHA" "" 1)"
PR_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn \
  --arg title "[Patchwright E2E] Approval-gated draft" \
  --arg head "$BRANCH" --arg base "$BASE_BRANCH" --arg body "$MARKER" \
  '{kind:"draftPullRequest",title:$title,head:$head,base:$base,body:$body}')" "$BASE_SHA" "" 1)"
PR_NUMBER="$(jq -er '.result.number' <<<"$PR_RESULT")" || die "draft pull request number is missing"
PR_URL="$(jq -er '.result.htmlUrl' <<<"$PR_RESULT")" || die "draft pull request URL is missing"
PR_NODE_ID="$(gh api "repos/$TARGET/pulls/$PR_NUMBER" --jq .node_id)" \
  || die "draft pull request node lookup failed"
gh api graphql --silent \
  -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){pullRequest{isDraft}}}' \
  -F id="$PR_NODE_ID" || die "marking the qualification pull request ready failed"

rpc_result github.sync '{"repositoryLimit":"100","resourceLimit":"1000"}' >/dev/null
SNAPSHOT="$(rpc_result github.repository "$(jq -cn --arg fullName "$TARGET" '{fullName:$fullName}')")"
PR_UPDATED_AT="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .updatedAt' \
  <<<"$SNAPSHOT")" || die "qualification pull request was not ingested"
PR_TASK="$(rpc_result task.createFromGitHub "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg itemNumber "$PR_NUMBER" \
  --arg expectedUpdatedAt "$PR_UPDATED_AT" \
  '{repositoryFullName:$repositoryFullName,itemNumber:$itemNumber,expectedUpdatedAt:$expectedUpdatedAt}')")"
PR_TASK_ID="$(jq -er '.task.id' <<<"$PR_TASK")" || die "qualification pull request task is missing"
PR_BASE_SHA="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .baseSha' \
  <<<"$SNAPSHOT")" || die "qualification pull request base SHA is missing"
PR_HEAD_SHA="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .headSha' \
  <<<"$SNAPSHOT")" || die "qualification pull request head SHA is missing"

REVIEW_RESULT="$(deliver "$PR_TASK_ID" "$(jq -cn --argjson pullRequestNumber "$PR_NUMBER" --arg body "$MARKER" \
  '{kind:"review",pullRequestNumber:$pullRequestNumber,event:"comment",body:$body,inlineComments:[]}')" \
  "$PR_BASE_SHA" "$PR_HEAD_SHA" 2)"
MERGE_RESULT="$(deliver "$PR_TASK_ID" "$(jq -cn --argjson pullRequestNumber "$PR_NUMBER" --arg expectedHeadSha "$PR_HEAD_SHA" \
  '{kind:"mergePullRequest",pullRequestNumber:$pullRequestNumber,expectedHeadSha:$expectedHeadSha,method:"squash"}')" \
  "$PR_BASE_SHA" "$PR_HEAD_SHA" 2)"
jq -e '.result.merged == true' >/dev/null <<<"$MERGE_RESULT" \
  || die "qualification pull request was not merged"

APP_SLUG="$(target/release/patchwright-relay github-app-health --config "$CONFIGURATION" | jq -er .slug)" \
  || die "final GitHub App identity check failed"
gh api "repos/$TARGET/issues/$ISSUE_NUMBER/comments" \
  | jq -e --arg marker "$MARKER" --arg login "$APP_SLUG[bot]" \
    '.[] | select(.body == $marker and .user.login == $login)' >/dev/null \
  || die "App-authored issue comment could not be reconciled"
gh api "repos/$TARGET/pulls/$PR_NUMBER/reviews" \
  | jq -e --arg marker "$MARKER" --arg login "$APP_SLUG[bot]" \
    '.[] | select(.body == $marker and .user.login == $login)' >/dev/null \
  || die "App-authored pull request review could not be reconciled"
gh api "repos/$TARGET/commits/$PR_HEAD_SHA/check-runs" \
  | jq -e --argjson appId "$APP_ID" \
    '.check_runs[] | select(.name == "Patchwright Qualification" and .app.id == $appId and .conclusion == "success")' \
    >/dev/null || die "App-authored check run could not be reconciled"
gh api "repos/$TARGET/pulls/$PR_NUMBER" --jq '.merged == true' | grep -qx true \
  || die "merged pull request state could not be reconciled"

if strings "$DATABASE" "$ENGINE_LOG" | grep -Eq 'BEGIN (RSA )?PRIVATE KEY|ghs_[A-Za-z0-9]+'; then
  die "credential material appeared in durable qualification evidence"
fi
EVIDENCE_DIR="${PATCHWRIGHT_GITHUB_E2E_EVIDENCE_DIR:-$HOME/.patchwright/evidence}"
mkdir -p "$EVIDENCE_DIR"
chmod 700 "$EVIDENCE_DIR"
EVIDENCE="$EVIDENCE_DIR/github-app-e2e-$RUN_ID.json"
jq -n \
  --arg completedAt "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg repository "$TARGET" \
  --argjson repositoryId "$TARGET_REPOSITORY_ID" \
  --argjson installationId "$TARGET_INSTALLATION_ID" \
  --arg appSlug "$APP_SLUG" \
  --arg issueUrl "$ISSUE_URL" \
  --arg pullRequestUrl "$PR_URL" \
  --arg branch "$BRANCH" \
  --arg baseSha "$BASE_SHA" \
  --arg headSha "$PR_HEAD_SHA" \
  --arg issueTaskId "$ISSUE_TASK_ID" \
  --arg pullRequestTaskId "$PR_TASK_ID" \
  --argjson branchResult "$BRANCH_RESULT" \
  --argjson checkResult "$CHECK_RESULT" \
  --argjson commentResult "$COMMENT_RESULT" \
  --argjson pullRequestResult "$PR_RESULT" \
  --argjson reviewResult "$REVIEW_RESULT" \
  --argjson mergeResult "$MERGE_RESULT" \
  '{completedAt:$completedAt,repository:$repository,repositoryId:$repositoryId,
    installationId:$installationId,appSlug:$appSlug,issueUrl:$issueUrl,
    pullRequestUrl:$pullRequestUrl,branch:$branch,baseSha:$baseSha,headSha:$headSha,
    issueTaskId:$issueTaskId,pullRequestTaskId:$pullRequestTaskId,
    actions:{branch:$branchResult,check:$checkResult,comment:$commentResult,
      draftPullRequest:$pullRequestResult,review:$reviewResult,merge:$mergeResult},
    credentialPersistenceCheck:"passed"}' >"$EVIDENCE"
chmod 600 "$EVIDENCE"
printf 'GitHub App E2E passed: %s\nEvidence: %s\n' "$PR_URL" "$EVIDENCE"
