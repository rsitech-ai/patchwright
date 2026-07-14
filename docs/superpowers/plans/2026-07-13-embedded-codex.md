# Embedded Codex Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every active Patchwright task a supervised, persistent, cancellable Codex app-server process and a fully native thread experience.

**Architecture:** A Rust `CodexSupervisor` owns one process group per active task, speaks newline-delimited app-server JSON-RPC, normalizes notifications into durable task events, and exposes typed Patchwright RPC methods. Swift consumes normalized events and sends operator input/approvals; it never owns the Codex subprocess or authentication.

**Tech Stack:** Rust/Tokio subprocess and async I/O, Codex app-server 0.144.x protocol schemas, SQLite, Unix process groups, Swift concurrency, SwiftUI.

## Global Constraints

- Depend on the foundation plan's task contracts, checkpoints, approvals, and jobs.
- Generate or validate protocol fixtures against the installed Codex app-server schema; never infer message shapes from UI text.
- A task process starts with its isolated worktree as `cwd`; Codex receives no GitHub, Apple, or Patchwright approval credential.
- Persist thread/turn/item/approval identities and bounded summaries, not raw secrets or unbounded terminal output.
- Graceful interrupt precedes process-group termination; task cancellation retains worktree and evidence.

---

## Task 1: Pin and validate the app-server protocol boundary

**Files:**
- Add: `crates/patchwright-engine/src/codex/protocol.rs`
- Add: `crates/patchwright-engine/src/codex/mod.rs`
- Modify: `crates/patchwright-engine/src/lib.rs`
- Add: `crates/patchwright-engine/tests/codex_protocol.rs`
- Add: `crates/patchwright-engine/tests/fixtures/codex/*.jsonl`

- [x] Capture sanitized fixtures for initialize/result, initialized, account/read, thread/start, thread/resume, turn/start, turn/steer, turn/interrupt, streamed item notifications, approval requests, completion, and error.
- [x] Write decode/encode tests that reject unknown required enum values, missing IDs, oversized lines, malformed JSON, duplicate completion, and a response ID that does not match a pending request.
- [x] Run `cargo test -p patchwright-engine --test codex_protocol` and observe missing-module RED.
- [x] Implement typed request/response/notification envelopes with serde tagging only at the exact discriminator fields emitted by the validated official schema; retain an explicit `Unsupported` event for forward-compatible notifications.
- [x] Add a 4 MiB per-line bound and redact credential-shaped fields from debug output.
- [x] Add `script/verify_codex_schema.sh` that resolves the exact `codex` executable/version and compares required methods/fields with generated app-server schema output.
- [x] Run fixtures and schema validation, then commit: `Define the Codex app-server protocol boundary`.

## Task 2: Supervise one task-owned process group

**Files:**
- Add: `crates/patchwright-engine/src/codex/process.rs`
- Add: `crates/patchwright-engine/tests/codex_process.rs`
- Add: `crates/patchwright-engine/tests/support/fake_codex_app_server.rs`
- Modify: `crates/patchwright-engine/Cargo.toml`

- [x] Write fake-server tests for exact executable discovery, missing executable, version mismatch warning, worktree `cwd`, independent task processes, stderr capture bounds, early exit, hung initialization, and process-group cleanup.
- [x] Observe RED.
- [x] Implement `CodexExecutable`, `CodexProcess`, and `CodexProcessFactory`; launch with piped stdio/stderr and a distinct Unix process group.
- [x] Add initialization and request timeouts, bounded stderr ring buffer, child-exit watcher, and explicit process states `starting/ready/stopping/exited/failed`.
- [x] Never pass access tokens or GitHub environment variables into the child; construct an allowlisted environment plus required user/Codex paths.
- [x] Run focused tests including two simultaneous fake tasks and commit: `Supervise task-owned Codex processes`.

## Task 3: Initialize account state and persist thread identity

**Files:**
- Add: `crates/patchwright-engine/src/codex/session.rs`
- Modify: `crates/patchwright-engine/src/store.rs`
- Add: `crates/patchwright-engine/tests/codex_session.rs`

- [x] Write tests for initialize→initialized order, account signed-in/signed-out/unavailable states, new thread start, saved thread resume after engine restart, stale thread fallback requiring operator confirmation, and no task transition before ready.
- [x] Observe RED.
- [x] Add `codex_sessions` and `codex_events` tables keyed by task and process generation; persist protocol version, executable version, account state, thread ID, last turn ID, last sequence, and bounded status.
- [x] Implement `CodexSession` handshake and thread start/resume using task contract instructions and isolated worktree `cwd`.
- [x] Atomically checkpoint thread identity with the task event that enters implementing.
- [x] Run restart tests twice against the same database and commit: `Persist Codex task sessions`.

## Task 4: Stream turns and render a native thread

**Files:**
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Add: `crates/patchwright-engine/src/codex/service.rs`
- Add: `crates/patchwright-engine/tests/codex_rpc.rs`
- Add: `Sources/PatchwrightCore/CodexModels.swift`
- Modify: `Sources/PatchwrightCore/EngineClient.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`
- Add: `Sources/PatchwrightApp/Views/CodexThreadView.swift`
- Modify: `Sources/PatchwrightApp/Views/TaskDetailView.swift`
- Add: `Tests/PatchwrightCoreTests/CodexPresentationTests.swift`

- [x] Add engine tests for `codex.status`, `codex.start`, `codex.events`, `codex.turn.start`, and `codex.turn.steer`, including pagination cursor, duplicate client message ID, invalid task state, and input bounds.
- [x] Add Swift tests for ordered text/reasoning/command/file-change/status events, streaming deltas, reconnect cursor, long content, unknown event, and send/steer disabled states.
- [x] Observe RED in Rust and Swift.
- [x] Implement a task-scoped event fan-in that normalizes app-server events and persists sequence numbers; Swift polls/cursors through the existing Unix socket until a later streaming transport is justified.
- [x] Build `CodexThreadView` with native selectable transcript, operator composer, streaming status, command/file-change cards, interruption/failure/recovery states, and exact task/thread/turn details in inspector.
- [x] Keep the task workbench mode stable across refresh with scene storage.
- [x] Run focused tests, strict Swift Release build, staged app launch, and commit: `Embed the Codex task thread`.

## Task 5: Bridge Codex runtime approvals without conflating authority

**Files:**
- Modify: `crates/patchwright-engine/src/codex/service.rs`
- Modify: `crates/patchwright-engine/src/rpc.rs`
- Add: `crates/patchwright-engine/tests/codex_approvals.rs`
- Modify: `Sources/PatchwrightCore/CodexModels.swift`
- Add: `Sources/PatchwrightApp/Views/CodexApprovalSheet.swift`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`

- [ ] Add tests that a Codex command/file request creates only a `CodexRuntime` approval request; it cannot authorize network, GitHub delivery, workflow, or merge capability.
- [ ] Test exact request ID/process generation/turn binding, expiration, duplicate response idempotency, restart recovery, and invalidation after a new turn/process generation.
- [ ] Observe RED.
- [ ] Normalize approval requests into typed previews with command argv/cwd or file diff summary and feed accept/decline back to the originating app-server request.
- [ ] Add `codex.approval.resolve` RPC with optimistic generation check and append-only decision event.
- [ ] Render an exact approval sheet with target, reason, expiration, invalidation, and Approve once/Decline. Do not add global approval.
- [ ] Run all approval tests and commit: `Bridge exact Codex runtime approvals`.

## Task 6: Implement interruption, cancellation, and crash recovery

**Files:**
- Modify: `crates/patchwright-engine/src/codex/process.rs`
- Modify: `crates/patchwright-engine/src/codex/service.rs`
- Modify: `crates/patchwright-engine/src/jobs.rs`
- Add: `crates/patchwright-engine/tests/codex_cancellation.rs`
- Modify: `Sources/PatchwrightCore/WorkspaceStore.swift`
- Modify: `Sources/PatchwrightApp/Views/CodexThreadView.swift`

- [ ] Write fault tests for cancel before turn, during stream, during command, after app-server completion but before checkpoint, ignored interrupt, child crash, engine crash, and relaunch resume.
- [ ] Assert `turn/interrupt` is sent once; new turns/commands/GitHub work are prevented; after timeout the owned process group is terminated; unrelated processes survive; worktree/evidence remain.
- [ ] Observe RED.
- [ ] Implement cancellation-token propagation, graceful timeout, TERM then KILL only for the recorded group, and compare-and-set terminal checkpoint.
- [ ] Reconcile a completion received during cancellation before marking the task cancelled.
- [ ] Add Pause/Cancel UI with explicit semantics and retained-worktree message.
- [ ] Run fault matrix repeatedly and commit: `Cancel and recover Codex task execution`.

## Task 7: Real local Codex integration gate

**Files:**
- Add: `script/smoke_codex.sh`
- Add: `docs/audits/2026-07-13-embedded-codex.md`
- Modify: `script/verify.sh`

- [ ] Create a disposable local Git repository containing a deterministic one-file task and verification command; never use Patchwright's own worktree for the smoke mutation.
- [ ] Verify installed Codex version/schema, account state, process isolation, new thread, one turn, streamed event persistence, runtime approval path if requested, file result, task interrupt, engine restart, and thread resume.
- [ ] Run the fake-server suite, complete Rust/Swift verification, real smoke, staged app interaction, relaunch, and secret/log scan.
- [ ] Record exact process/thread/turn IDs only in local evidence if they are not secrets; sanitize command output and repository content.
- [ ] Assign `integration-ready: Codex` only if the real disposable smoke passes; otherwise name the exact local/account blocker.
- [ ] Commit: `Verify embedded Codex end to end`.
