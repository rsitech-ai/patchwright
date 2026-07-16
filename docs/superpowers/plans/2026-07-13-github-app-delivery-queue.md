# GitHub App Delivery and PR Queue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace development-only GitHub credentials with a production GitHub App broker, make ingestion cancellable, deliver exact approved writes, organize PR work explainably, and perform approval-gated exact-SHA merges or merge-queue handoff.

**Architecture:** `patchwright-relay` owns GitHub App JWT/token minting and typed remote mutations. `patchwright-engine` owns durable jobs, action previews, policy/approval checks, ephemeral Git push coordination, queue decisions, and reconciliation. The Swift app only requests previews, collects scoped approvals, and displays durable results.

**Tech Stack:** Rust, Tokio, reqwest, jsonwebtoken/RS256, macOS Keychain via Security framework or `security` subprocess with stdin-safe boundaries, SQLite, GitHub REST API, SwiftUI.

## Global Constraints

- Depend on the foundation approval/job model and embedded-Codex cancellation model.
- `gh` remains development/read-only fallback. Production reads/writes use installation tokens.
- Private key material lives in Keychain or a protected relay secret mount; installation tokens live only in memory and never appear in debug output.
- Every remote mutation requires a fresh typed preview, exact action fingerprint, unexpired matching approval, idempotency identity, and ambiguous-result reconciliation.
- No qualification write targets a production repository. A disposable GitHub App/repository requires a separately confirmed external test setup.
- Never use admin bypass or treat labels/comments/repository text as authority.

---

## Task 1: Model GitHub App configuration and secret storage

**Files:**
- Add: `crates/patchwright-relay/src/app_auth.rs`
- Add: `crates/patchwright-relay/tests/app_auth.rs`
- Modify: `crates/patchwright-relay/src/lib.rs`
- Modify: `crates/patchwright-relay/Cargo.toml`
- Add: `Sources/PatchwrightCore/GitHubAppModels.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`
- Modify: `Sources/PatchwrightApp/Views/SettingsView.swift`

- [ ] Add tests for PEM import validation, wrong algorithm, encrypted/invalid/truncated key, world-readable secret mount rejection, Keychain item reference persistence, and redacted Debug/Display/error output.
- [ ] Observe RED.
- [ ] Add `GitHubAppConfiguration { app_id, client_id, key_reference, api_base_url }`; persist only metadata/reference.
- [ ] Add a narrow `PrivateKeyProvider` trait with macOS Keychain and protected-file implementations. Raw environment-variable private keys are rejected; an environment variable may contain only a path/reference.
- [ ] Add Settings import through a file panel, app/installation health, credential source, permission summary, test connection, and actionable missing/revoked states.
- [ ] Run tests plus a tracked-file/database/log secret scan and commit: `Add the GitHub App secret boundary`.

## Task 2: Mint and cache scoped installation tokens

**Files:**
- Modify: `crates/patchwright-relay/src/app_auth.rs`
- Add: `crates/patchwright-relay/src/installation.rs`
- Add: `crates/patchwright-relay/tests/installation_tokens.rs`

- [ ] Add deterministic-clock tests for RS256 JWT `iat` skew, `exp` maximum, `iss`, signature, installation discovery, repository/permission scoping, token expiry refresh, concurrent cache collapse, revocation, rate limit, and GitHub error redaction.
- [ ] Observe RED.
- [ ] Implement short-lived app JWT signing and installation lookup; mint tokens limited to the selected repository IDs and minimum requested permissions.
- [ ] Cache installation tokens only in memory until a safety margin before expiration; key cache by installation+repository set+permission set.
- [ ] Expose typed health/provenance without returning JWT/token/key material.
- [ ] Run the mock GitHub suite under leak-detecting assertions and commit: `Broker scoped GitHub installation tokens`.

## Task 3: Migrate reads and make sync a cancellable durable job

**Files:**
- Modify: `crates/patchwright-engine/src/github.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Modify: `crates/patchwright-engine/src/jobs.rs`
- Modify: `crates/patchwright-engine/src/store.rs`
- Add: `crates/patchwright-engine/tests/github_jobs.rs`
- Modify: `Sources/PatchwrightCore/GitHubModels.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`

- [ ] Add tests for `github.sync.start/status/cancel`, one active sync per workspace, job progress, cancel before discovery/during fan-out/during response, request abort, engine restart, expired token refresh, installation-token primary path, and `gh` read-only fallback labeling.
- [ ] Assert cancellation stops new fan-out, preserves every prior complete repository snapshot, never stores a partial current snapshot, and yields a durable cancelled summary.
- [ ] Observe RED.
- [ ] Inject a `GitHubCredentialProvider` into `GitHubSource`; route production reads through relay-issued installation tokens.
- [ ] Replace blocking `github.sync` RPC with start/status/cancel; keep a temporary deprecated wrapper only if existing Swift migration tests require it.
- [ ] Add cancellation tokens to discovery, semaphore acquisition, pagination, enrichment, and snapshot commit boundaries.
- [ ] Update UI progress/cancel/partial/cancelled/retry states and commit: `Make GitHub ingestion cancellable and app-authenticated`.

## Task 4: Define typed mutation previews and relay adapters

**Files:**
- Add: `crates/patchwright-core/src/github_actions.rs`
- Add: `crates/patchwright-core/tests/github_action_contract.rs`
- Modify: `crates/patchwright-core/src/lib.rs`
- Add: `crates/patchwright-relay/src/mutations.rs`
- Add: `crates/patchwright-relay/tests/mutations.rs`

- [ ] Add contract tests for branch create/update, push intent, comment, pending/submitted review, inline review comments, thread reply/resolve, check run, draft PR create/update, update branch, close/supersede PR, enqueue, and merge.
- [ ] Test payload bounds, repository/ref validation, exact expected SHAs, Markdown preview hashing, duplicate inline-comment positions, permission mapping, stable idempotency digest, and redacted serialization.
- [ ] Observe RED.
- [ ] Add `GitHubAction`, `GitHubActionPreview`, `RemotePrecondition`, `RemoteIdentity`, `RetryClass`, and `ReconciliationQuery`.
- [ ] Implement relay REST adapters with exact API version/Accept headers, typed status handling, primary rate-limit metadata, and no automatic retry for ambiguous writes.
- [ ] Run mock response matrices and commit: `Define typed GitHub delivery actions`.

## Task 5: Execute approval-bound branch, push, comment, review, check, and draft PR delivery

**Files:**
- Add: `crates/patchwright-engine/src/delivery.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Modify: `crates/patchwright-engine/src/store.rs`
- Add: `crates/patchwright-engine/tests/delivery_flow.rs`
- Add: `Sources/PatchwrightCore/DeliveryModels.swift`
- Add: `Sources/PatchwrightApp/Views/DeliveryApprovalSheet.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`

- [ ] Add end-to-end mock tests for preview→approval→fresh precondition→claim idempotency key→execute→record identity→re-fetch, plus expired/mismatched approval, changed head/base, changed payload, denied permission, definite failure, ambiguous timeout, restart reconciliation, and cancellation.
- [ ] Add a Git push harness proving installation tokens do not appear in argv, remote URL, local/global Git config, reflog, environment dump, or logs; use a temporary credential-helper script/socket with restrictive permissions and remove it after the process exits.
- [ ] Observe RED.
- [ ] Implement `DeliveryService` with one serialized mutation lane per task and transactionally claimed action digests.
- [ ] Add `delivery.preview`, `delivery.approve`, `delivery.execute`, `delivery.status`, and `delivery.cancel` RPC methods. Approval never executes implicitly.
- [ ] Render an exact approval sheet with target, branch/PR, SHA range, changed files, remote body/review/check content, expiry, invalidations, and discrete actions—no Approve Everything.
- [ ] Run the full mock/restart/secret suite and commit: `Execute approved GitHub delivery actions`.

## Task 6: Build the explainable PR queue and workflow presets

**Files:**
- Add: `crates/patchwright-core/src/queue.rs`
- Add: `crates/patchwright-core/tests/queue_contract.rs`
- Add: `crates/patchwright-engine/src/queue.rs`
- Modify: `crates/patchwright-engine/src/store.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Add: `crates/patchwright-engine/tests/queue_recovery.rs`
- Add: `Sources/PatchwrightCore/QueueModels.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`
- Modify: `Sources/PatchwrightApp/Views/WorkspaceTableView.swift`

- [ ] Add table-driven tests for all tiers, reason strings, dependency ordering, manual pins/reorder persistence, workflow presets, stale/unknown states, and deterministic tie-breakers.
- [ ] Add overlap tests for dependency edges and changed-path intersections; one mutating task per repository; read-only assessment may coexist; a completed mutation cannot advance the next item until remote monitoring is fresh.
- [ ] Observe RED.
- [ ] Implement `QueueState`, `QueueTier`, `QueueReason`, `WorkflowPreset`, `DependencyEdge`, and pure assessment functions for Quick Wins, CI Rescue, Review Closure, Conflict Recovery, Dependency Chain, Security First, Release Train, Stale PR Triage, Draft Completion, Post-Merge Watch, Review Load Balancing, and Duplicate/Overlap Detection.
- [ ] Persist queue items/decisions/manual order/decision input hash; recompute when source snapshot changes and retain the prior explanation in events.
- [ ] Add queue start/pause/advance RPC and native table/toolbars/saved workflow selection.
- [ ] Run queue/restart/Swift presentation tests and commit: `Organize pull requests into explainable workflows`.

## Task 7: Monitor delivery and create bounded repair iterations

**Files:**
- Add: `crates/patchwright-engine/src/monitoring.rs`
- Add: `crates/patchwright-engine/tests/monitoring_flow.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`

- [ ] Add tests for CI pending/success/failure, requested changes, dismissed approval, new head SHA, base movement, conflict, inaccessible fork, rate limit, network loss, repair budget exhaustion, and post-push cancellation.
- [ ] Observe RED.
- [ ] Implement bounded polling with webhook wakeups, backoff/jitter, durable next-attempt time, fresh installation token, and repair iteration limits.
- [ ] Re-enter assessing/planned for actionable repair; invalidate prior delivery/merge approvals; never let monitor text grant authority.
- [ ] Stop the repository lane on unknown remote state or exhausted repair budget and show the exact blocker.
- [ ] Run time-controlled tests and commit: `Monitor and repair delivered pull requests`.

## Task 8: Add exact-SHA approval-gated merge and native merge-queue handoff

**Files:**
- Modify: `crates/patchwright-core/src/github_actions.rs`
- Add: `crates/patchwright-core/tests/merge_approval.rs`
- Modify: `crates/patchwright-relay/src/mutations.rs`
- Add: `crates/patchwright-relay/tests/merge.rs`
- Add: `crates/patchwright-engine/src/merge.rs`
- Add: `crates/patchwright-engine/tests/merge_flow.rs`
- Add: `Sources/PatchwrightCore/MergeModels.swift`
- Add: `Sources/PatchwrightApp/Views/MergeApprovalSheet.swift`

- [ ] Add tests binding approval to repository/installation/PR/exact head+base/method/check snapshot/review snapshot/expiry/idempotency key.
- [ ] Add invalidation tests for any SHA/check/review/mergeability/branch-rule/permission change, new commit, conflict, stale snapshot, expired approval, automation kill switch, and admin bypass request.
- [ ] Add direct merge tests that send expected head SHA and record merge SHA; add required merge-queue tests that enqueue, monitor merge group, and record final merge identity.
- [ ] Add ambiguous timeout/restart reconciliation and post-merge regression tests that stop the repository queue.
- [ ] Observe RED.
- [ ] Implement fresh preflight immediately before execution and separate `merge.preview/approve/execute/status/cancel` RPC methods.
- [ ] Render checks/reviews/method/SHAs/expiry/invalidation exactly; never combine merge approval with delivery approval.
- [ ] Run the complete merge matrix and commit: `Gate merges on fresh exact pull request state`.

## Task 9: Authorized disposable GitHub App E2E gate

**Files:**
- Add: `script/smoke_github_app.sh`
- Add: `docs/audits/2026-07-13-github-app-delivery-queue.md`
- Modify: `script/verify.sh`

- [ ] Fail closed unless environment identifies an allowlisted disposable owner/repository and GitHub App installation. Print targets and require a one-shot explicit confirmation variable; reject the Patchwright production repository.
- [ ] Exercise installation discovery, read sync, cancellation, task branch push, comment, check, pending/submitted review where supported, draft PR create/update, CI/review monitoring, approval invalidation on a changed SHA, approved merge or native merge-queue handoff, and post-merge queue advancement.
- [ ] Revoke/expire credentials in the disposable setup and verify recovery without token persistence.
- [ ] Audit database, process table, environment captures, Git config, logs, bundle, and evidence for secrets.
- [ ] Run full local gates before and after E2E; record exact remote IDs/URLs and cleanup actions without deleting evidence needed for reconciliation.
- [ ] Assign `integration-ready: GitHub delivery/merge` only if this authorized disposable workflow passes. Otherwise report `blocked:external` with the missing App/repository/permission.
- [ ] Commit: `Verify GitHub App delivery and queue workflows`.
