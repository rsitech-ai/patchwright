# Patchwright live lifecycle closure audit

Audit date: 2026-07-14

Branch: `feat/andrzej_agent_sota_lab`

## Outcome

**Readiness: integration-ready for approval-gated GitHub issue and pull-request work on an installed repository.** The native app, durable Rust orchestrator, production GitHub App token broker, embedded Codex app-server, exact delivery previews, and exact-SHA merge boundary completed a live private-sandbox workflow. The staged app is bundle-valid and usable for local testing. Developer ID signing, notarization, Gatekeeper distribution acceptance, and clean-machine installation remain independent external Apple gates.

Patchwright PR #1 was read-only throughout this audit and remains open.

## Scope and authority boundary

- App: `/Users/s1kor/.patchwright/staged/Patchwright.app`
- Bundle ID: `ai.patchwright.app`
- Workspace: `/Users/s1kor/dev/apps/patchwright`
- Live mutation target: private repository `s1korrrr/patchwright-e2e-sandbox` only
- Production GitHub App: `patchwright-s1korrrr`, App ID `4294269`
- Forbidden during the audit: mutation or merge of `s1korrrr/patchwright#1`, admin bypass, default-branch push, force-push, secret logging, and unrelated repository mutation

## Live end-to-end evidence

| Surface | Result | Evidence |
| --- | --- | --- |
| Issue ingestion | Verified | Sandbox issue [#3](https://github.com/s1korrrr/patchwright-e2e-sandbox/issues/3) was ingested with the installation token and converted to typed task `44bc6755-9412-4d58-89a8-6aa7189ea092`. |
| Isolated worktree | Verified | `/Users/s1kor/.patchwright/repositories/1300326259/worktrees/44bc6755-9412-4d58-89a8-6aa7189ea092`, branch `patchwright/44bc6755-9412-4d58-89a8-6aa7189ea092`. |
| Exact branch push | Verified | Commit `078ee96c9c5c53b962188b9554136ef3bcc65537` was previewed, approved, pushed through the ephemeral App-token Git transport, and read back from GitHub. |
| Draft PR | Verified | Patchwright created draft PR [#5](https://github.com/s1korrrr/patchwright-e2e-sandbox/pull/5), then the ready-for-review boundary preserved the captured head SHA. |
| PR ingestion | Verified | PR #5 was re-ingested and converted to typed task `593a577a-7cd8-4b1f-8d0f-1b3dfcdb2f43`, capturing base `e0c4157ea9d616053d6c3cd26f2ce2b2f8d8b231` and head `078ee96c9c5c53b962188b9554136ef3bcc65537`. |
| Embedded Codex | Verified | The signed-in `codex-cli 0.144.2` reviewed the exact worktree without network or file mutation. SQLite persisted 430 ordered events and a terminal turn. The native Codex tab renders the prompt, command evidence, state, and signed-in status. |
| Review delivery | Verified | The GitHub App posted review `PRR_kwDOTYFnc88AAAABGAr3ag` against exact commit `078ee96c9c5c53b962188b9554136ef3bcc65537`. |
| Review-thread resolution | Verified | Sandbox PR [#6](https://github.com/s1korrrr/patchwright-e2e-sandbox/pull/6) ingested opaque thread `PRRT_kwDOTYFnc86Q50_7`; exact approval `976057bb61b5…` resolved only that thread at head `0b62863be0a327245b8ec25c97f9e86ddbe9b3ca`, and the next App-token snapshot reported `threadResolved: true`. The unrelated thread remained unresolved. |
| Exact-SHA merge | Verified | A separate Merge-class approval produced merge commit [`0dd94eecab8f72ae258fda4e971b71e53c324591`](https://github.com/s1korrrr/patchwright-e2e-sandbox/commit/0dd94eecab8f72ae258fda4e971b71e53c324591). PR #5 is `MERGED`, issue #3 is `CLOSED`, and sandbox `main` points to that commit. |
| Historical task recovery | Verified | Read-only `task.reconcileGitHub` refreshed exact issue #3 and PR #5 through installation tokens, required the PR's captured head SHA and merged state, and atomically moved tasks `44bc6755…` and `593a577a…` to Completed. No direct SQLite repair was used. |
| Sync cancellation | Verified | Durable job `afcd09b9-aed5-4ee5-8ad4-dfd8ed4dce04` ended `cancelled` with cancellation `acknowledged` and summary `GitHub sync cancelled after 15 repositories`. |
| Patchwright PR safety | Verified | `s1korrrr/patchwright#1` remained `OPEN` on `feat/andrzej_agent_sota_lab`; no merge was attempted. |

## Product behavior retained

- Ingested issues and pull requests become immutable typed task contracts bound to repository installation, source URL, source SHAs, instruction digests, acceptance criteria, and declared capabilities.
- Preparation creates a task-owned worktree at the captured source SHA. Git push uses an ephemeral installation-token credential helper and pushes only the inspected task branch head.
- Embedded Codex has visible thread state, streamed durable events, steering, one-time runtime approvals, cancellation, process-group cleanup, and restart recovery.
- GitHub writes cover branch creation/push, comments, reviews, exact review-thread resolution, checks, draft PRs, update branch, ready-for-review, issue/PR closure, merge-queue handoff, and exact-SHA merge. Every write requires a fresh typed preview, matching approval, precondition validation, and idempotency claim. Thread resolution uses installation identity first, then a non-persisted signed-in user token only when GitHub reports that the App viewer cannot resolve the author-owned thread.
- Successful merge persistence and task lifecycle completion now occur in one local transaction. The task advances through delivery, monitoring, merge, and completion while recording visible timeline events.
- A definitive failed delivery can be retried with a fresh approval. Ambiguous and successful outcomes remain claimed. Legacy non-JSON delivery results remain safely non-retryable instead of breaking database recovery.
- Queue controls include Latest Commit and Updated sorting plus explainable workflow presets for Quick Wins, CI Rescue, Review Closure, Conflict Recovery, Dependency Chain, Security First, Release Train, Stale PR Triage, Draft Completion, Post-Merge Watch, Review Load Balancing, and Duplicate/Overlap Detection.

## Verification matrix

| Gate | Command or surface | Result |
| --- | --- | --- |
| Full static/test/build gate | `./script/verify.sh` | Passed: Rust fmt/Clippy, all Rust workspace tests and doc tests, 28 Swift tests, release contract, and production Swift build. |
| Engine smoke | `./script/smoke.sh` | Passed with clean engine shutdown. |
| Real Codex smoke | `./script/smoke_codex.sh` | Passed against signed-in `codex-cli 0.144.2`; disposable lifecycle persisted 42 events. |
| Staged app | `./script/build_and_run.sh --verify` | Passed; app and bundled engine are running from the staged bundle. |
| Bundle structure/signature | `validate_bundle.sh --require-signed` and strict deep `codesign` verification | Passed with marketing version `0.1.0`, build `1`, arm64 executables, and an ad-hoc local signature. |
| Native interaction | Computer Use accessibility and screenshot inspection | Passed: queue sorting headers, selected PR detail, Active Tasks workbench, Merge gate, and embedded Codex transcript rendered in the rebuilt app. |
| Review-thread interaction | Native Delivery workbench plus GraphQL readback | Passed: one unresolved thread exposes Preview Resolve with user-authority copy; the exact completed thread renders Resolved with file and line. |
| Remote reconciliation | `gh pr view`, `gh issue view`, and `gh api .../commits/main` | Passed: PR merged, issue closed, review bound to exact head, and main bound to exact merge SHA. |
| Release readiness | `script/release_readiness.sh` | Repo, Codex, GitHub integration, and bundle-valid gates are true; external Apple distribution gates are false. Evidence: `/Users/s1kor/.patchwright/evidence/release-readiness-20260714.json`. |

## Defects found and fixed in this pass

| Severity | Finding | Retained fix and re-verification |
| --- | --- | --- |
| High | A completed Codex turn moved the engine task to Verifying but the visible task header did not refresh. | The store refreshes tasks when a new terminal turn event arrives; focused Swift regression and full suite pass. |
| High | Draft PRs could not be marked ready through Patchwright, so GitHub correctly rejected merge. | Added approval-bound `readyPullRequest`, exact-head validation, GraphQL ready-for-review mutation, UI preview, and request-capture tests. |
| High | A definitive failed delivery remained permanently claimed. | Failed JSON results can be reclaimed; successful, ambiguous, and legacy results cannot. Restart and delivery regressions pass. |
| High | Successful GitHub merge did not atomically advance the durable task to Completed. | Delivery result plus all remaining lifecycle events now commit in one SQLite transaction; focused lifecycle regression passes. |
| High | REST review ingestion lacked the opaque thread identity required to resolve an individual discussion. | Added paginated GraphQL thread ingestion, typed `ResolveReviewThread`, exact repository/PR/head revalidation, approval UI, and request-capture regressions. Live PR #6 resolution passed. |
| High | GitHub installation identities report `viewerCanResolve: false`, even for an App-authored thread. | Resolution remains App-first but narrowly falls back to the already signed-in user's brokered token after the App attempt fails. The token is never persisted; the same exact approval and relay validations apply. |
| High | Pre-fix remotely completed tasks remained stuck in Awaiting Delivery Approval. | Added read-only App-token reconciliation that rejects unmerged PRs and changed heads, persists the fresh snapshot, and commits remaining legal task events atomically. Both historical tasks now render under Completed. |
| Medium | The retry predicate called `json_extract` on legacy non-JSON results and broke restart recovery. | JSON is inspected only after `json_valid`; the original restart failure is green. |
| Medium | `build_and_run.sh` generated a launchable but release-invalid plist without marketing/build versions. | The staged build now uses canonical `Packaging/Info.plist` and validates the bundle before signing. Rebuilt bundle validation passes. |

## Remaining boundaries

- GitHub requires user-context authority to resolve author-owned review threads. The current local product brokers that authority from an authenticated `gh` installation after the App attempt fails; a clean-machine distribution needs either authenticated `gh` or a future in-app GitHub App user-authorization flow for this one action.
- The staged test app is locally ad-hoc signed. A valid `Developer ID Application: Rafal Sikora (2NY8A789TN)` identity is now installed, but `notarytool history --keychain-profile Patchwright` reports that no Keychain password item exists. Developer ID package assembly can proceed; notarization/stapling, Gatekeeper distribution acceptance, final DMG qualification, and clean-machine validation remain `blocked:external` until a notary profile is stored.
- The GitHub App is intentionally installed only on the disposable sandbox. Installing it on another repository is an owner-controlled authorization decision.

## Rollback

All remote writes are confined to `patchwright-e2e-sandbox`. Source changes are isolated on `feat/andrzej_agent_sota_lab`. Patchwright PR #1 remains the review boundary; no production merge or force-push was used.
