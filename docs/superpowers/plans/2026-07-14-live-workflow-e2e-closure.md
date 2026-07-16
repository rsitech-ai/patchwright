# Patchwright live workflow end-to-end closure

## Goal

- Prove that an ingested issue or pull request can become a durable Patchwright task, visibly run through an embedded Codex thread, create and push a branch, publish progress/checks/comments/reviews, resolve review work, deliver a draft pull request, and complete an exact-SHA approval-gated merge into the configured base branch.
- Make failures and in-progress work visible in the native app with durable recovery evidence instead of relying on terminal-only status.

## Audit Contract

- App: `/Users/s1kor/.patchwright/staged/Patchwright.app`; SwiftPM workspace `/Users/s1kor/dev/apps/patchwright`; bundle ID `ai.patchwright.app`; launch through `./script/build_and_run.sh --verify`.
- Platform: native macOS app backed by the local Rust engine, SQLite state, Codex app-server, GitHub App installation-token broker, and GitHub REST/git delivery adapters.
- Live mutation target: private disposable repository `s1korrrr/patchwright-e2e-sandbox` only. Patchwright PR `s1korrrr/patchwright#1`, its branch, and production repositories are read-only during this audit.
- Allowed live actions in the sandbox: create disposable issues, task branches, commits, checks, comments, review threads/reviews, draft PRs, exact-SHA approvals, and merges into the sandbox default branch; reconcile and close test-only artifacts when the workflow owns them.
- Forbidden: bypass branch protection, use admin merge, expose credentials, log tokens/private keys, mutate unrelated repositories, or merge Patchwright PR #1.
- Evidence: exact commands, GitHub object URLs/SHAs, SQLite/job events, Codex thread/turn state, app screenshots/accessibility trees, engine/app logs, restart/cancellation proof, and a committed audit report.
- Readiness target: interaction-clean for the complete sandbox workflow; release-candidate signing/notarization remains an independent gate.

## Failure Hypotheses

- The UI exposes previews but does not drive the real typed lifecycle or lacks actionable progress after task creation.
- Codex supervision can start locally while thread/turn streaming, recovery, cancellation, or task ownership is not visible or durable.
- GitHub branch, push, check, comment, review, draft-PR, or merge adapters pass mocks but fail with production installation tokens or stale SHA preconditions.
- Review resolution or merge approval can be applied to a stale head SHA, repeated after restart, or confused with native GitHub merge-queue handoff.
- The queue can advance overlapping mutations or report completion before remote GitHub state reconciles.

## Scenario Matrix

1. Build, launch, sync, relaunch, and confirm production GitHub App health in the native UI.
2. Ingest a disposable sandbox issue and existing PR; inspect source content, task preview, and acceptance criteria.
3. Create a typed task and verify visible queued/running/Codex progress, persisted thread and turn identifiers, streamed output, and cancellation/restart semantics.
4. Run branch creation, safe commit, push, progress check, issue/PR comment, draft-PR delivery, review submission, review-thread resolution, and remote reconciliation.
5. Verify stale-head rejection, exact-SHA merge approval, branch-protection/no-admin-bypass behavior, merge execution, and recorded merge SHA.
6. Exercise queue sorting/WIP gates, failure/retry states, app relaunch recovery, and final completed state.
7. Re-run focused/full tests, real Codex smoke, disposable GitHub E2E, native interaction/log checks, and security/secret scans; update the audit with the weakest truthful readiness label.

## Final Closure Slice

1. Ingest GraphQL pull-request review threads with their opaque node IDs, exact PR identity, resolved/outdated state, and viewer resolution authority. Keep thread IDs as untrusted opaque values and never infer them from REST comment IDs.
2. Add a typed `resolveReviewThread` action with exact PR/head preconditions, delivery approval, stable idempotency, scoped `pull_requests:write` permission, server-side ownership revalidation, and GraphQL mutation result verification.
3. Render unresolved threads in the task Delivery workbench and expose only a preview action; reuse the existing approval sheet and execute boundary.
4. Add a read-only `task.reconcileGitHub` RPC. Re-fetch the task's exact issue or PR through the installation token, require matching repository/number/head identity, and atomically advance only remotely closed/merged tasks through the remaining legal lifecycle states.
5. Reconcile the two pre-fix sandbox tasks, relaunch the app, and verify they move from Active Tasks to Completed with durable timeline evidence.
6. Create one disposable open sandbox PR with an unresolved inline thread, ingest it, resolve it through Patchwright's preview/approval/execute path, and re-ingest to prove `isResolved=true`.
7. Repeat full verification, real Codex smoke, staged runtime/UI inspection, remote GitHub readback, release readiness, secret scan, commit, and branch push. Do not merge Patchwright PR #1.

## Final Closure Decisions

- Review-thread discovery uses GitHub GraphQL because REST review comments do not expose the `PullRequestReviewThread` node ID required by `resolveReviewThread`.
- Reconciliation is a read-only remote verification followed by a local durable transition. It never performs a GitHub write and cannot accept an operator-supplied final state.
- A merged PR must retain the task's captured head SHA; a closed-but-unmerged PR is not treated as completed unless the task explicitly executed an approved close action.
- Historical records are reconciled by the same production RPC used after ambiguous outcomes, not by direct SQLite edits.

## Rollback

- Keep all live mutations confined to test objects in `patchwright-e2e-sandbox`. Preserve local task/evidence state for diagnosis. Revert cohesive source/test commits in reverse order; never guess at remote rollback or force-push a shared branch.

## Progress Log

- 2026-07-14: User requested a real end-to-end audit of issue/PR resolution, Codex progress visibility, GitHub delivery/review/merge, and recovery. Baseline audit started from clean branch `feat/andrzej_agent_sota_lab` with the local test app building and launching.
- 2026-07-14: Live sandbox issue #3 became a typed task and exact worktree commit `078ee96c`; Patchwright pushed the task branch and created PR #5 through App-token delivery.
- 2026-07-14: PR #5 became a second typed task. Embedded Codex persisted 430 review events, Patchwright posted an exact-head review, and separately approved merge delivery produced sandbox main commit `0dd94eec` while closing issue #3.
- 2026-07-14: Global sync cancellation reached durable `cancelled/acknowledged` state after 15 repositories. Failed-delivery retry, ready-for-review, visible turn-completion refresh, and atomic merge-to-Completed reconciliation were added with regressions.
- 2026-07-14: Full verify, engine smoke, real Codex smoke, staged runtime, canonical bundle metadata validation, strict code-sign verification, native workbench inspection, and remote GitHub reconciliation passed. A Developer ID identity is installed; Apple distribution remains blocked by the missing `notarytool` Keychain profile and clean-machine evidence.
- 2026-07-14: GraphQL review-thread ingestion and exact typed resolution passed on sandbox PR #6. GitHub rejected installation identity for resolution, so the approved action now narrowly falls back to the non-persisted signed-in user token; a second App-token snapshot proved only the intended thread resolved.
- 2026-07-14: Read-only `task.reconcileGitHub` verified issue #3 closed-completed and PR #5 merged at the captured head, then moved both pre-fix task records to Completed through atomic lifecycle events rather than direct database edits.
- 2026-07-14: Clean-commit release assembly found a valid Developer ID identity, then reached the macOS Keychain private-key approval prompt. The attempt was cancelled safely. The `Patchwright` notary profile is absent, so signing needs one local Keychain approval and notarization remains externally blocked until `notarytool store-credentials Patchwright` is completed.
