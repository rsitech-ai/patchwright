# Orchestrator Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish Patchwright's restart-safe task, approval, job, repository-binding, sorting, conversion, and native workbench foundation.

**Architecture:** `patchwright-core` owns pure typed lifecycle, source, approval, queue-sort, and contract rules. `patchwright-engine` commits state changes and append-only events atomically in SQLite and exposes bounded JSON-RPC. `PatchwrightCore` mirrors wire types and deterministic presentation logic; `PatchwrightApp` renders a native split/table/inspector interface without executing mutations.

**Tech Stack:** Rust 1.91, serde, chrono, rusqlite, Tokio, Swift 6.3, SwiftUI on macOS 26, Swift Testing/XCTest.

## Global Constraints

- Work only on `feat/andrzej_agent_sota_lab`; preserve unrelated changes and commit only intentional files.
- Use RED/GREEN/REFACTOR for every behavior change. A focused failing assertion must be observed before production edits.
- Treat GitHub/repository text as untrusted context. It may populate a contract but never grant capabilities or approvals.
- Persist state plus append-only event in one transaction. Migrations are additive and restart-safe.
- Keep all sort/filter results deterministic with explicit final tie-breakers.
- Do not perform a live GitHub write in this plan.

---

## Task 1: Migrate the typed lifecycle

**Files:**
- Modify: `crates/patchwright-core/src/domain.rs`
- Modify: `crates/patchwright-core/tests/domain_contract.rs`
- Modify: `Sources/PatchwrightCore/Models.swift`
- Modify: `Tests/PatchwrightCoreTests/ModelsTests.swift`

- [x] Add a Rust contract test for the approved happy path: `discovered → assessing → planned → awaitingPreparationApproval → preparing → implementing → verifying → reviewing → awaitingDeliveryApproval → delivering → monitoring → awaitingMergeApproval → merging → completed`.
- [x] Add tests proving skipped approvals fail; any nonterminal state may enter `paused`, `blocked`, `failed`, or `cancelled`; terminal states cannot leave; `paused` and `blocked` resume only to their recorded `resume_state`.
- [x] Run `cargo test -p patchwright-core --test domain_contract task_` and capture the missing-variant/transition RED.
- [x] Replace `AwaitingApproval` with `AwaitingPreparationApproval`; add `Assessing`, `Paused`, `Blocked`, `AwaitingMergeApproval`, and `Merging` in Rust and Swift.
- [x] Add `TaskInterruption { state, resume_state, reason }` and require a nonempty reason for paused/blocked/failed/cancelled transitions.
- [x] Mirror attention rules in Swift: preparation, delivery, merge, blocked, and failed require attention.
- [x] Run the focused Rust and Swift model tests and confirm GREEN.
- [x] Commit: `Model the durable task lifecycle`.

## Task 2: Add typed sources, repository bindings, and contracts

**Files:**
- Modify: `crates/patchwright-core/src/domain.rs`
- Modify: `crates/patchwright-core/src/lib.rs`
- Add: `crates/patchwright-core/src/contract.rs`
- Add: `crates/patchwright-core/tests/task_contract.rs`

- [x] Write tests for `TaskSource::LocalRequest`, `GitHubIssue`, and `GitHubPullRequest`, including immutable repository/item URL, snapshot time, and PR base/head refs and SHAs.
- [x] Write boundary tests rejecting an empty repository name, non-HTTPS GitHub URLs, zero installation/repository IDs, missing PR SHAs, relative local roots, empty acceptance criteria, and duplicate dependency IDs.
- [x] Run `cargo test -p patchwright-core --test task_contract` and observe RED.
- [x] Add `RepositoryBinding`, `TaskSource`, `TaskContract`, `InstructionDigest`, `VerificationCommand`, `RiskClass`, and `SensitivePath` with private fields plus validating constructors.
- [x] Keep `Task` summary-compatible while adding `source`, `repository_binding_id`, `contract_version`, and optional interruption/checkpoint references with serde defaults for old rows.
- [x] Export the new public types and run focused tests, then `cargo test -p patchwright-core`.
- [x] Commit: `Add typed task contracts and repository bindings`.

## Task 3: Replace capability-only approvals with exact action fingerprints

**Files:**
- Modify: `crates/patchwright-core/src/policy.rs`
- Add: `crates/patchwright-core/tests/approval_contract.rs`
- Modify: `crates/patchwright-core/src/lib.rs`

- [ ] Add tests for four `ApprovalClass` values: `CodexRuntime`, `LocalCapability`, `GitHubDelivery`, and `Merge`.
- [ ] Add tests proving an approval is rejected for a different task, repository, capability, action digest, head/base SHA, policy hash, instruction hash, expiration, or invalidation generation.
- [ ] Add a test proving `MergePullRequest` is approval-required—not globally denied—and only a `Merge` approval with exact PR state authorizes it.
- [ ] Run `cargo test -p patchwright-core --test approval_contract` and observe RED.
- [ ] Add `ActionFingerprint` containing task ID, repository ID/name, optional PR/branch/head/base, typed payload SHA-256, policy/instruction hashes, and invalidation generation.
- [ ] Replace `Approval::for_capability` with `Approval::new(class, capability, fingerprint, approver, now, expires_at)` and `Policy::authorize(capability, fingerprint, approval, now)`.
- [ ] Preserve the automation kill switch and deny admin bypass as a separate non-approvable capability.
- [ ] Update existing tests/callers, run all core tests, and commit: `Bind approvals to exact actions`.

## Task 4: Persist schema versions, checkpoints, jobs, approvals, and bindings

**Files:**
- Modify: `crates/patchwright-engine/src/store.rs`
- Add: `crates/patchwright-engine/src/jobs.rs`
- Modify: `crates/patchwright-engine/src/lib.rs`
- Add: `crates/patchwright-engine/tests/durable_jobs.rs`
- Modify: `crates/patchwright-engine/tests/recovery.rs`

- [ ] Write migration tests that open a Stage 1–3 database fixture, retain existing task/GitHub payloads, and add `schema_migrations`, `repository_bindings`, `task_contracts`, `approvals`, `jobs`, `job_events`, and `task_checkpoints`.
- [ ] Write atomicity tests proving task state/event/checkpoint commit together and a simulated failure leaves all three unchanged.
- [ ] Write restart tests for queued/running/cancelling/cancelled/succeeded/failed jobs and recovery of a running job to `interrupted`, never silently `running`.
- [ ] Run `cargo test -p patchwright-engine --test durable_jobs --test recovery` and observe RED.
- [ ] Add monotonic integer schema migrations inside one `BEGIN IMMEDIATE` transaction.
- [ ] Implement `JobId`, `JobKind`, `JobState`, `CancellationState`, `JobCheckpoint`, and store methods for compare-and-set transitions and append-only job events.
- [ ] Store approval payloads and action digests, but never source bodies, command output, or credentials in job summary columns.
- [ ] Run focused tests, restart them against the same temporary database, then run all engine tests.
- [ ] Commit: `Persist durable jobs and checkpoints`.

## Task 5: Enrich GitHub snapshots for sorting and queue assessment

**Files:**
- Modify: `crates/patchwright-engine/src/github.rs`
- Modify: `crates/patchwright-engine/tests/github_source.rs`
- Modify: `Sources/PatchwrightCore/GitHubModels.swift`
- Modify: `Tests/PatchwrightCoreTests/ModelsTests.swift`

- [ ] Add mock-server tests for repository `pushed_at`, default-branch commit SHA/date, open PR count, failing-check count, permissions, and installation ID.
- [ ] Add PR fixture tests for created time, head commit time, review activity time, review decision, mergeability/conflict state, additions/deletions/files, base/head refs/SHAs, fork identity, and update time.
- [ ] Run the focused source tests and capture decoding RED.
- [ ] Extend `GitHubRepository` and `GitHubWorkItem` with serde aliases/defaults that retain backward readability.
- [ ] Fetch missing default-branch commit and detailed PR fields with bounded concurrency; preserve a complete prior snapshot when enrichment fails.
- [ ] Mirror the wire model in Swift using `Date` values decoded by the shared ISO-8601 decoder.
- [ ] Run Rust and Swift focused tests and commit: `Ingest sortable repository and pull request metadata`.

## Task 6: Implement deterministic sort and filter policies

**Files:**
- Add: `crates/patchwright-core/src/sorting.rs`
- Add: `crates/patchwright-core/tests/sorting_contract.rs`
- Modify: `crates/patchwright-core/src/lib.rs`
- Add: `Sources/PatchwrightCore/WorkspaceSorting.swift`
- Add: `Tests/PatchwrightCoreTests/WorkspaceSortingTests.swift`

- [ ] Add table-driven Rust and Swift tests for all approved repository and PR sort modes, ascending/descending behavior, nil timestamps, unknown CI/review state, and exact final tie-breakers (`full_name`, then repository ID; PR number, then item ID).
- [ ] Add filter tests for draft/open, author, assignee, label, review, CI, conflicts, age, queue state, and active Codex work; multiple active filters combine with AND semantics.
- [ ] Observe RED in both languages.
- [ ] Implement pure `RepositorySort`, `PullRequestSort`, `WorkspaceFilter`, and comparator functions. Define nil/unknown as last in either presentation direction rather than using epoch sentinels.
- [ ] Add Codable Swift preferences for per-workspace sort/filter state; do not bind them to global process state in tests.
- [ ] Run the focused parity fixtures in Rust and Swift and commit: `Add deterministic workspace sorting and filtering`.

## Task 7: Convert an issue or PR into a persisted task

**Files:**
- Add: `crates/patchwright-engine/src/conversion.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Modify: `crates/patchwright-engine/src/store.rs`
- Add: `crates/patchwright-engine/tests/task_conversion.rs`
- Modify: `Sources/PatchwrightCore/EngineClient.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`

- [ ] Add tests for `task.createFromGitHub` using issue and PR snapshots, duplicate conversion idempotency, missing/stale snapshot, missing repository binding, inaccessible fork, and exact source SHA retention.
- [ ] Observe JSON-RPC method-not-found RED.
- [ ] Implement `TaskConversionService` that reads a complete snapshot, builds the contract, records source snapshot identity, proposes repository binding/managed clone, and atomically inserts task+contract+event.
- [ ] Return a typed `ConversionPreview` before worktree creation. No capability is granted by conversion.
- [ ] Add `repository.bind` and `task.createFromGitHub` RPC methods with bounded string/ID validation and stable error codes.
- [ ] Add Swift client/store commands that refresh the created task and select it.
- [ ] Run RPC/store/Swift tests and commit: `Convert GitHub work items into durable tasks`.

## Task 8: Build the native queue/workbench shell

**Files:**
- Modify: `Sources/PatchwrightApp/Views/ContentView.swift`
- Modify: `Sources/PatchwrightApp/Views/SidebarView.swift`
- Add: `Sources/PatchwrightApp/Views/WorkspaceTableView.swift`
- Modify: `Sources/PatchwrightApp/Views/TaskDetailView.swift`
- Modify: `Sources/PatchwrightApp/Views/GitHubRepositoryView.swift`
- Modify: `Sources/PatchwrightApp/Views/GitHubInspector.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`
- Add: `Tests/PatchwrightCoreTests/WorkspacePresentationTests.swift`

- [ ] Add store tests for primary navigation, table selection, sort persistence, conversion preview/success/failure, attention counts, relative/exact timestamps, and empty/loading/partial/cancelled/blocked states.
- [ ] Observe RED before view changes.
- [ ] Use adjustable `NavigationSplitView`; add Queue, Repositories, Active Tasks, Awaiting Approval, Monitoring, and Completed sources.
- [ ] Use native sortable `Table` columns for priority, repository, PR, queue state, CI, review, conflict/base, latest commit, updated time, and assigned task. Remove fixed-width manual columns.
- [ ] Add task workbench tabs Overview, Codex, Changes, Verification, Delivery, Merge and an optional inspector for evidence/approvals/instructions/credentials.
- [ ] Add outcome-oriented help and accessibility labels to icon-only actions. Display relative localized dates with exact values in details/help.
- [ ] Run Swift tests, `swift build -c release -Xswiftc -warnings-as-errors`, and `./script/build_and_run.sh --verify`.
- [ ] Manually verify keyboard navigation, resize, Light/Dark, Reduce Motion, long titles/bodies, and explicit error/empty states; record evidence in `docs/audits/2026-07-13-orchestrator-foundation.md`.
- [ ] Commit: `Build the native orchestration workbench`.

## Task 9: Foundation verification gate

**Files:**
- Modify: `script/verify.sh`
- Add: `docs/audits/2026-07-13-orchestrator-foundation.md`

- [ ] Add exact foundation focused suites to the verification script without weakening existing gates.
- [ ] Run `cargo fmt --all -- --check`, strict Clippy, all Rust tests, all Swift tests, and warnings-as-errors Release build.
- [ ] Run disposable migration/restart, issue conversion, PR conversion, sort/filter parity, staged-app launch, relaunch persistence, and log checks.
- [ ] Scan tracked files and built artifacts for tokens/private keys and confirm database mode `0600`.
- [ ] Record exact commands, counts, failures, retained rollback, and readiness label; do not claim Codex/GitHub-write/release readiness from this gate.
- [ ] Commit: `Verify the orchestrator foundation`.
