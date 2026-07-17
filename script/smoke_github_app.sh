#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail() {
  printf 'blocked:external — %s\n' "$1" >&2
  exit 78
}

die() {
  printf 'GitHub App qualification failed — %s\n' "$1" >&2
  exit 1
}

TARGET_OWNER="${PATCHWRIGHT_GITHUB_E2E_OWNER:-}"
TARGET_REPOSITORY="${PATCHWRIGHT_GITHUB_E2E_REPOSITORY:-}"
TARGET_REPOSITORY_ID="${PATCHWRIGHT_GITHUB_E2E_REPOSITORY_ID:-}"
TARGET_INSTALLATION_ID="${PATCHWRIGHT_GITHUB_E2E_INSTALLATION_ID:-}"
TARGET="$TARGET_OWNER/$TARGET_REPOSITORY"
NORMALIZED_TARGET="$(printf '%s' "$TARGET" | tr '[:upper:]' '[:lower:]')"
ALLOWLIST="${PATCHWRIGHT_GITHUB_E2E_ALLOWLIST:-}"
CONFIRMATION="${PATCHWRIGHT_GITHUB_E2E_CONFIRM:-}"
CONFIGURATION="${PATCHWRIGHT_GITHUB_APP_CONFIG:-$HOME/.patchwright/github-app.json}"
EXISTING_ISSUE_NUMBER="${PATCHWRIGHT_GITHUB_E2E_EXISTING_ISSUE_NUMBER:-}"
MANUAL_UI_FIXTURE=false
if [[ -n "$EXISTING_ISSUE_NUMBER" ]]; then
  MANUAL_UI_FIXTURE=true
  [[ "$EXISTING_ISSUE_NUMBER" =~ ^[1-9][0-9]*$ ]] \
    || fail "PATCHWRIGHT_GITHUB_E2E_EXISTING_ISSUE_NUMBER must be positive"
fi

[[ -n "$TARGET_OWNER" && -n "$TARGET_REPOSITORY" ]] || fail "set the disposable owner and repository"
[[ "$NORMALIZED_TARGET" != "s1korrrr/patchwright" ]] || fail "the Patchwright production repository is forbidden"
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

if [[ "$MANUAL_UI_FIXTURE" == false ]]; then
  LIVE_REPOSITORY_ID="$(gh api "repos/$TARGET" --jq .id 2>/dev/null)" \
    || fail "the disposable repository is not readable with the current development credential"
  [[ "$LIVE_REPOSITORY_ID" == "$TARGET_REPOSITORY_ID" ]] \
    || fail "repository identity does not match the allowlisted ID"
fi

printf 'GitHub App E2E target: %s (repository %s, installation %s, app %s, client %s)\n' \
  "$TARGET" "$TARGET_REPOSITORY_ID" "$TARGET_INSTALLATION_ID" "$APP_ID" "$CLIENT_ID"

cd "$ROOT_DIR"
cargo build --release -p patchwright-relay
target/release/patchwright-relay github-app-health --config "$CONFIGURATION"
cargo test -p patchwright-relay --test app_auth --test installation_tokens --test mutations
cargo test -p patchwright-engine --test delivery_flow --test monitoring_flow --test queue_recovery
cargo build -p patchwright-engine

STATE_DIR="$(mktemp -d)"
SOCKET="$STATE_DIR/engine.sock"
DATABASE="$STATE_DIR/engine.sqlite3"
ENGINE_LOG="$STATE_DIR/engine.log"
cleanup() {
  local status=$?
  if [[ -n "${ENGINE_PID:-}" ]]; then
    kill "$ENGINE_PID" >/dev/null 2>&1 || true
    wait "$ENGINE_PID" >/dev/null 2>&1 || true
  fi
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
  response="$(printf '%s\n' "$request" | nc -U "$SOCKET")" || {
    printf 'engine RPC transport failed for %s\n' "$method" >&2
    return 1
  }
  if ! jq -e 'has("result") and (has("error") | not)' >/dev/null <<<"$response"; then
    jq -c '{method:"'"$method"'",error:.error}' <<<"$response" >&2 || true
    return 1
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
    --argjson actionPreview "$action_preview" '{taskId:$taskId,actionPreview:$actionPreview}')")" \
    || return 1
  approval="$(rpc_result delivery.approve "$(jq -cn --arg approvedBy "owner-qualified:$TARGET" \
    --argjson preview "$preview" '{preview:$preview,approvedBy:$approvedBy}')")" || return 1
  approval_id="$(jq -er '.id' <<<"$approval")" || return 1
  rpc_result delivery.execute "$(jq -cn --arg approvalId "$approval_id" \
    --argjson preview "$preview" '{preview:$preview,approvalId:$approvalId}')"
}

prepare_task() {
  local task_id="$1"
  local preview approval approval_id
  rpc_result task.plan "$(jq -cn --arg taskId "$task_id" '{taskId:$taskId}')" >/dev/null \
    || return 1
  preview="$(rpc_result task.preparation.preview \
    "$(jq -cn --arg taskId "$task_id" '{taskId:$taskId}')")" || return 1
  approval="$(rpc_result task.preparation.approve "$(jq -cn \
    --arg approvedBy "owner-qualified:$TARGET" --argjson preview "$preview" \
    '{preview:$preview,approvedBy:$approvedBy}')")" || return 1
  approval_id="$(jq -er '.id' <<<"$approval")" || return 1
  rpc_result task.prepare "$(jq -cn --arg taskId "$task_id" \
    --argjson preview "$preview" --arg approvalId "$approval_id" \
    '{taskId:$taskId,preview:$preview,approvalId:$approvalId}')"
}

submit_task_for_delivery() {
  local task_id="$1"
  rpc_result task.readyForDelivery "$(jq -cn --arg taskId "$task_id" '{taskId:$taskId}')"
}

start_and_observe_monitor() {
  local task_id="$1"
  local pull_request_number="$2"
  local expected_head_sha="$3"
  local expected_base_sha="$4"
  local monitor monitor_id
  monitor="$(rpc_result monitor.start "$(jq -cn \
    --arg taskId "$task_id" --arg repositoryFullName "$TARGET" \
    --argjson pullRequestNumber "$pull_request_number" \
    --arg expectedHeadSha "$expected_head_sha" --arg expectedBaseSha "$expected_base_sha" \
    '{monitor:{taskId:$taskId,repositoryFullName:$repositoryFullName,
      pullRequestNumber:$pullRequestNumber,expectedHeadSha:$expectedHeadSha,
      expectedBaseSha:$expectedBaseSha,repairBudget:2}}')")" || return 1
  monitor_id="$(jq -er '.id' <<<"$monitor")" || return 1
  rpc_result monitor.observe "$(jq -cn --arg monitorId "$monitor_id" '{monitorId:$monitorId}')"
}

sync_until_work_item() {
  local kind="$1"
  local number="$2"
  local attempt
  for attempt in 1 2 3; do
    SNAPSHOT="$(rpc_result github.sync.repository "$(jq -cn \
      --arg fullName "$TARGET" \
      --arg repositoryId "$TARGET_REPOSITORY_ID" \
      --arg installationId "$TARGET_INSTALLATION_ID" \
      '{fullName:$fullName,repositoryId:$repositoryId,installationId:$installationId,resourceLimit:1000}')")" \
      || return 1
    if jq -e --arg kind "$kind" --argjson number "$number" \
      '.workItems[] | select(.number == $number and .kind == $kind)' \
      >/dev/null <<<"$SNAPSHOT"; then
      return 0
    fi
    if [[ "$attempt" != 3 ]]; then
      sleep "$((attempt * 2))"
    fi
  done
  die "qualification $kind #$number was not ingested after three bounded sync attempts"
}

mkdir -p "$STATE_DIR/state" "$STATE_DIR/worktrees"
target/debug/patchwright-engine serve --socket "$SOCKET" --database "$DATABASE" \
  >"$ENGINE_LOG" 2>&1 &
ENGINE_PID=$!
for _ in {1..100}; do
  test -S "$SOCKET" && break
  sleep 0.05
done
test -S "$SOCKET" || die "engine socket did not become ready"
rpc_result system.health '{}' >/dev/null || die "engine health check failed"

RUN_ID="${PATCHWRIGHT_GITHUB_E2E_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
[[ "$RUN_ID" =~ ^[0-9]{8}T[0-9]{6}Z$ ]] || die "qualification run ID is invalid"
BRANCH=""
MARKER="Patchwright GitHub App qualification $RUN_ID"
if [[ "$MANUAL_UI_FIXTURE" == true ]]; then
  ISSUE_NUMBER="$EXISTING_ISSUE_NUMBER"
else
  BASE_BRANCH="$(gh api "repos/$TARGET" --jq .default_branch)" \
    || die "default branch lookup failed"
  BASE_SHA="$(gh api "repos/$TARGET/commits/$BASE_BRANCH" --jq .sha)" \
    || die "default branch commit lookup failed"
  ISSUE="$(gh api --method POST "repos/$TARGET/issues" \
    -f title="[Patchwright E2E] Approval-gated delivery" \
    -f body="Disposable private qualification fixture: $MARKER")" \
    || die "qualification issue creation failed"
  ISSUE_NUMBER="$(jq -er .number <<<"$ISSUE")" || die "qualification issue number is missing"
  ISSUE_UPDATED_AT="$(jq -er .updated_at <<<"$ISSUE")" || die "qualification issue timestamp is missing"
  ISSUE_URL="$(jq -er .html_url <<<"$ISSUE")" || die "qualification issue URL is missing"
fi

sync_until_work_item issue "$ISSUE_NUMBER" || die "qualification issue was not ingested"
if [[ "$MANUAL_UI_FIXTURE" == true ]]; then
  BASE_BRANCH="$(jq -er '.repository.defaultBranch' <<<"$SNAPSHOT")" \
    || die "default branch is missing from the App snapshot"
  BASE_SHA="$(jq -er '.repository.defaultBranchSha' <<<"$SNAPSHOT")" \
    || die "default branch SHA is missing from the App snapshot"
  ISSUE_UPDATED_AT="$(jq -er --argjson number "$ISSUE_NUMBER" \
    '.workItems[] | select(.number == $number and .kind == "issue") | .updatedAt' \
    <<<"$SNAPSHOT")" || die "qualification issue timestamp is missing"
  ISSUE_URL="$(jq -er --argjson number "$ISSUE_NUMBER" \
    '.workItems[] | select(.number == $number and .kind == "issue") | .htmlUrl' \
    <<<"$SNAPSHOT")" || die "qualification issue URL is missing"
fi
rpc_result repository.bind "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg installationId "$TARGET_INSTALLATION_ID" \
  --arg managedClone "$STATE_DIR/repository" \
  --arg stateRoot "$STATE_DIR/state" \
  --arg worktreeRoot "$STATE_DIR/worktrees" \
  '{repositoryFullName:$repositoryFullName,installationId:$installationId,
    managedClone:$managedClone,stateRoot:$stateRoot,worktreeRoot:$worktreeRoot}')" >/dev/null \
  || die "repository binding failed"
ISSUE_TASK="$(rpc_result task.createFromGitHub "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg itemNumber "$ISSUE_NUMBER" \
  --arg expectedUpdatedAt "$ISSUE_UPDATED_AT" \
  '{repositoryFullName:$repositoryFullName,itemNumber:$itemNumber,expectedUpdatedAt:$expectedUpdatedAt}')")" \
  || die "qualification issue task creation failed"
ISSUE_TASK_ID="$(jq -er '.task.id' <<<"$ISSUE_TASK")" || die "qualification issue task is missing"
BRANCH="patchwright/$ISSUE_TASK_ID"
ISSUE_PREPARED="$(prepare_task "$ISSUE_TASK_ID")" \
  || die "qualification issue task preparation failed"
ISSUE_WORKTREE="$(jq -er '.repositoryPath' <<<"$ISSUE_PREPARED")" \
  || die "qualification issue worktree is missing"
printf '%s\n' "$MARKER" >"$ISSUE_WORKTREE/patchwright-e2e.txt"
git -C "$ISSUE_WORKTREE" add -- patchwright-e2e.txt
git -C "$ISSUE_WORKTREE" \
  -c user.name="Patchwright Qualification" \
  -c user.email="patchwright-qualification@localhost" \
  commit --no-gpg-sign -m "$MARKER" >/dev/null \
  || die "qualification worktree commit failed"
SEED_SHA="$(git -C "$ISSUE_WORKTREE" rev-parse HEAD)" \
  || die "qualification worktree head is missing"
submit_task_for_delivery "$ISSUE_TASK_ID" >/dev/null \
  || die "qualification issue verification failed"
REMOTE_IDENTITY="$(jq -cn \
  --argjson repositoryId "$TARGET_REPOSITORY_ID" \
  --argjson installationId "$TARGET_INSTALLATION_ID" \
  --arg repositoryFullName "$TARGET" \
  '{repositoryId:$repositoryId,installationId:$installationId,repositoryFullName:$repositoryFullName}')"

BRANCH_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --arg branch "$BRANCH" --arg fromSha "$BASE_SHA" \
  '{kind:"createBranch",branch:$branch,fromSha:$fromSha}')" "$BASE_SHA" "" 1)" \
  || die "approval-gated branch delivery failed"
PUSH_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --arg branch "$BRANCH" --arg headSha "$SEED_SHA" \
  '{kind:"pushIntent",branch:$branch,headSha:$headSha}')" "$BASE_SHA" "" 1)" \
  || die "approval-gated branch push failed"
CHECK_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --arg headSha "$SEED_SHA" \
  '{kind:"checkRun",name:"Patchwright Qualification",headSha:$headSha,status:"completed",conclusion:"success"}')" \
  "$BASE_SHA" "" 1)" || die "approval-gated check delivery failed"
COMMENT_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn --argjson issueNumber "$ISSUE_NUMBER" --arg body "$MARKER" \
  '{kind:"comment",issueNumber:$issueNumber,body:$body}')" "$BASE_SHA" "" 1)" \
  || die "approval-gated comment delivery failed"
PR_RESULT="$(deliver "$ISSUE_TASK_ID" "$(jq -cn \
  --arg title "[Patchwright E2E] Approval-gated draft" \
  --arg head "$BRANCH" --arg base "$BASE_BRANCH" --arg expectedBaseSha "$BASE_SHA" --arg body "$MARKER" \
  '{kind:"draftPullRequest",title:$title,head:$head,base:$base,expectedBaseSha:$expectedBaseSha,body:$body}')" "$BASE_SHA" "" 1)" \
  || die "approval-gated draft pull request delivery failed"
PR_NUMBER="$(jq -er '.result.number' <<<"$PR_RESULT")" || die "draft pull request number is missing"
PR_URL="$(jq -er '.result.htmlUrl' <<<"$PR_RESULT")" || die "draft pull request URL is missing"
if [[ "$MANUAL_UI_FIXTURE" == true ]]; then
  printf 'Mark this disposable draft ready for review in GitHub: %s\n' "$PR_URL" >&2
  printf 'Type ready after GitHub confirms the change: ' >&2
  IFS= read -r READY_CONFIRMATION
  [[ "$READY_CONFIRMATION" == "ready" ]] || die "manual ready-for-review confirmation is invalid"
else
  PR_NODE_ID="$(gh api "repos/$TARGET/pulls/$PR_NUMBER" --jq .node_id)" \
    || die "draft pull request node lookup failed"
  gh api graphql --silent \
    -f query='mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){pullRequest{isDraft}}}' \
    -F id="$PR_NODE_ID" || die "marking the qualification pull request ready failed"
fi

sync_until_work_item pullRequest "$PR_NUMBER" || die "qualification pull request was not ingested"
PR_UPDATED_AT="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .updatedAt' \
  <<<"$SNAPSHOT")" || die "qualification pull request was not ingested"
PR_TASK="$(rpc_result task.createFromGitHub "$(jq -cn \
  --arg repositoryFullName "$TARGET" \
  --arg itemNumber "$PR_NUMBER" \
  --arg expectedUpdatedAt "$PR_UPDATED_AT" \
  '{repositoryFullName:$repositoryFullName,itemNumber:$itemNumber,expectedUpdatedAt:$expectedUpdatedAt}')")" \
  || die "qualification pull request task creation failed"
PR_TASK_ID="$(jq -er '.task.id' <<<"$PR_TASK")" || die "qualification pull request task is missing"
PR_BASE_SHA="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .baseSha' \
  <<<"$SNAPSHOT")" || die "qualification pull request base SHA is missing"
PR_HEAD_SHA="$(jq -er --argjson number "$PR_NUMBER" \
  '.workItems[] | select(.number == $number and .kind == "pullRequest") | .headSha' \
  <<<"$SNAPSHOT")" || die "qualification pull request head SHA is missing"

prepare_task "$PR_TASK_ID" >/dev/null \
  || die "qualification pull request task preparation failed"
submit_task_for_delivery "$PR_TASK_ID" >/dev/null \
  || die "qualification pull request verification failed"

REVIEW_RESULT="$(deliver "$PR_TASK_ID" "$(jq -cn --argjson pullRequestNumber "$PR_NUMBER" --arg expectedHeadSha "$PR_HEAD_SHA" --arg body "$MARKER" \
  '{kind:"review",pullRequestNumber:$pullRequestNumber,expectedHeadSha:$expectedHeadSha,event:"comment",body:$body,inlineComments:[]}')" \
  "$PR_BASE_SHA" "$PR_HEAD_SHA" 2)" || die "approval-gated review delivery failed"
if [[ "$MANUAL_UI_FIXTURE" == true ]]; then
  printf 'Approve this disposable pull request in GitHub: %s\n' "$PR_URL" >&2
  printf 'Type approved after GitHub confirms the review: ' >&2
  IFS= read -r REVIEW_CONFIRMATION
  [[ "$REVIEW_CONFIRMATION" == "approved" ]] \
    || die "manual approval confirmation is invalid"
else
  gh api --method POST "repos/$TARGET/pulls/$PR_NUMBER/reviews" \
    -f event=APPROVE -f body="$MARKER owner approval" >/dev/null \
    || die "qualification owner approval failed"
fi
sync_until_work_item pullRequest "$PR_NUMBER" \
  || die "approved pull request evidence was not refreshed"
MONITOR_RESULT="$(start_and_observe_monitor \
  "$PR_TASK_ID" "$PR_NUMBER" "$PR_HEAD_SHA" "$PR_BASE_SHA")" \
  || die "trusted pull request monitoring failed"
jq -e '.outcome.state == "succeeded"' >/dev/null <<<"$MONITOR_RESULT" \
  || die "trusted pull request evidence did not satisfy monitoring"
MERGE_RESULT="$(deliver "$PR_TASK_ID" "$(jq -cn --argjson pullRequestNumber "$PR_NUMBER" --arg expectedHeadSha "$PR_HEAD_SHA" \
  '{kind:"mergePullRequest",pullRequestNumber:$pullRequestNumber,expectedHeadSha:$expectedHeadSha,method:"squash"}')" \
  "$PR_BASE_SHA" "$PR_HEAD_SHA" 2)" || die "approval-gated merge delivery failed"
jq -e '.result.merged == true' >/dev/null <<<"$MERGE_RESULT" \
  || die "qualification pull request was not merged"

APP_SLUG="$(target/release/patchwright-relay github-app-health --config "$CONFIGURATION" | jq -er .slug)" \
  || die "final GitHub App identity check failed"
if [[ "$MANUAL_UI_FIXTURE" == true ]]; then
  sync_until_work_item pullRequest "$PR_NUMBER" || die "merged pull request could not be reconciled"
  jq -e --arg marker "$MARKER" --arg login "$APP_SLUG[bot]" --argjson number "$ISSUE_NUMBER" \
    '.discussions[] | select(.itemNumber == $number and .body == $marker and .author == $login)' \
    >/dev/null <<<"$SNAPSHOT" || die "App-authored issue comment could not be reconciled"
  jq -e --arg marker "$MARKER" --arg login "$APP_SLUG[bot]" --argjson number "$PR_NUMBER" \
    '.discussions[] | select(.itemNumber == $number and .body == $marker and .author == $login)' \
    >/dev/null <<<"$SNAPSHOT" || die "App-authored pull request review could not be reconciled"
  jq -e --argjson number "$PR_NUMBER" \
    '.checks[] | select(.itemNumber == $number and .name == "Patchwright Qualification" and .conclusion == "success")' \
    >/dev/null <<<"$SNAPSHOT" || die "App-authored check run could not be reconciled"
  jq -e --argjson number "$PR_NUMBER" \
    '.workItems[] | select(.number == $number and .kind == "pullRequest" and .state == "closed")' \
    >/dev/null <<<"$SNAPSHOT" || die "merged pull request state could not be reconciled"
else
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
fi

if strings "$DATABASE" "$ENGINE_LOG" | grep -Eq 'BEGIN (RSA )?PRIVATE KEY|ghs_[A-Za-z0-9]+'; then
  die "credential material appeared in durable qualification evidence"
fi
EVIDENCE_DIR="${PATCHWRIGHT_GITHUB_E2E_EVIDENCE_DIR:-$HOME/.patchwright/evidence}"
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
  --argjson pushResult "$PUSH_RESULT" \
  --argjson checkResult "$CHECK_RESULT" \
  --argjson commentResult "$COMMENT_RESULT" \
  --argjson pullRequestResult "$PR_RESULT" \
  --argjson reviewResult "$REVIEW_RESULT" \
  --argjson monitorResult "$MONITOR_RESULT" \
  --argjson mergeResult "$MERGE_RESULT" \
  '{completedAt:$completedAt,repository:$repository,repositoryId:$repositoryId,
    installationId:$installationId,appSlug:$appSlug,issueUrl:$issueUrl,
    pullRequestUrl:$pullRequestUrl,branch:$branch,baseSha:$baseSha,headSha:$headSha,
    issueTaskId:$issueTaskId,pullRequestTaskId:$pullRequestTaskId,
    actions:{branch:$branchResult,push:$pushResult,check:$checkResult,comment:$commentResult,
      draftPullRequest:$pullRequestResult,review:$reviewResult,monitor:$monitorResult,
      merge:$mergeResult},
    credentialPersistenceCheck:"passed"}' \
  | "$ROOT_DIR/script/write_owner_evidence.py" --directory "$EVIDENCE_DIR" --name "$(basename "$EVIDENCE")" >/dev/null
printf 'GitHub App E2E passed: %s\nEvidence: %s\n' "$PR_URL" "$EVIDENCE"
