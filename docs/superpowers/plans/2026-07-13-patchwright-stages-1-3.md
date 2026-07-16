# Patchwright Stages 1–3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native macOS control plane, durable local execution engine, and verified GitHub lifecycle relay that complete one issue-to-draft-PR workflow with explicit approvals and evidence.

**Architecture:** A SwiftUI client consumes a narrow JSON-RPC 2.0 protocol over a Unix socket. A shared Rust library owns validated domain state and policy, while separate engine and relay binaries own local execution and GitHub HTTP/webhook I/O.

**Tech Stack:** Swift 6.3/SwiftUI/Foundation Models, Rust 2024/Tokio/Axum/Serde/Rusqlite, SQLite, Git CLI, Codex App Server, GitHub REST API.

## Global Constraints

- Target macOS 26 or newer on Apple silicon; use semantic SwiftUI materials and native desktop navigation.
- Rust packages use edition 2024, rust-version 1.85, `unsafe_code = "warn"`, and strict Clippy lints.
- No merge capability, long-lived personal token, shell-string RPC, credential logging, or automatic workflow modification.
- All tasks use isolated Git worktrees; all GitHub mutations require action-specific approval and idempotency evidence.
- Repository text and GitHub comments are untrusted context, not policy authority.

---

### Task 1: Shared domain and policy

**Files:** `Cargo.toml`, `crates/patchwright-core/Cargo.toml`, `crates/patchwright-core/src/{lib,domain,policy,instructions}.rs`, `crates/patchwright-core/tests/domain_contract.rs`

**Interfaces:** Produce `Task`, `TaskState`, `TaskEvent`, `Capability`, `Approval`, `Finding`, `Evidence`, `InstructionSource`, `EffectiveInstructions`, and `Policy::authorize`.

- [ ] Write a failing public-interface test proving invalid task transitions and merge authorization are rejected.
- [ ] Run `cargo test -p patchwright-core --test domain_contract`; expect failing imports.
- [ ] Add validated domain values, transition rules, and fail-closed capability policy.
- [ ] Run the focused test; expect all assertions to pass.

### Task 2: Durable event store

**Files:** `crates/patchwright-engine/src/store.rs`, `crates/patchwright-engine/tests/recovery.rs`

**Interfaces:** Produce `EventStore::open`, `append`, `load_task`, `record_approval`, `claim_delivery`, and `complete_delivery`.

- [ ] Write a restart test that appends task events, reopens SQLite, and rejects a duplicate delivery key.
- [ ] Run `cargo test -p patchwright-engine --test recovery`; expect a missing `EventStore` failure.
- [ ] Add schema migration in a transaction and append-only event persistence.
- [ ] Re-run the focused test; expect restart state and idempotency assertions to pass.

### Task 3: Repository, instruction, and worktree services

**Files:** `crates/patchwright-engine/src/{repository,worktree,command}.rs`, `crates/patchwright-engine/tests/worktree_flow.rs`

**Interfaces:** Produce `RepositoryService::inspect`, `InstructionResolver::resolve_for_paths`, `WorktreeService::prepare`, and `CommandRunner::run(CommandSpec)`.

- [ ] Write a temporary-Git-repository test proving nested instruction precedence, branch isolation, argv-safe execution, timeout capture, and base-checkout preservation.
- [ ] Run the focused integration test and confirm it fails before implementation.
- [ ] Implement Git subprocesses with explicit argv, canonical writable roots, sanitized environment, timeouts, and process-group cancellation.
- [ ] Re-run and confirm every observable behavior passes.

### Task 4: JSON-RPC and Codex adapters

**Files:** `crates/patchwright-engine/src/{rpc,codex}.rs`, `crates/patchwright-engine/src/main.rs`, `crates/patchwright-engine/tests/rpc_socket.rs`

**Interfaces:** Produce RPC methods `system.health`, `repository.inspect`, `instructions.resolve`, `task.create`, `task.approve`, `task.prepare`, `task.verify`, `task.review`, `task.deliver`, `task.cancel`, and `task.timeline`.

- [ ] Write a Unix-socket test proving health, invalid-parameter, create, approve, and timeline responses.
- [ ] Run the focused test and capture the connection/method failure.
- [ ] Add newline-delimited JSON-RPC framing, typed error mapping, connection limits, and Codex App Server process supervision.
- [ ] Re-run the test and then `codex app-server generate-json-schema --out /tmp/patchwright-codex-schema` to validate local availability.

### Task 5: GitHub App lifecycle

**Files:** `crates/patchwright-relay/src/{main,webhook,github,state}.rs`, `crates/patchwright-relay/tests/webhook_flow.rs`

**Interfaces:** Produce `verify_signature`, `WebhookEvent::parse`, `DeliveryStore::accept_once`, and `GitHubClient` operations for installation token, draft PR, check run, pending review, reply, and workflow status.

- [ ] Write a router test with signed and tampered payloads plus duplicate delivery IDs.
- [ ] Run the focused test and confirm the route is absent.
- [ ] Add constant-time HMAC verification before JSON parsing, bounded bodies, sanitized event mapping, idempotency, and typed GitHub API errors.
- [ ] Re-run and confirm invalid signatures cause no state write.

### Task 6: Native client domain and engine transport

**Files:** `Package.swift`, `Sources/PatchwrightCore/{Models,EngineClient,WorkspaceStore,FoundationReviewProvider}.swift`, `Tests/PatchwrightCoreTests/{ModelsTests,WorkspaceStoreTests}.swift`

**Interfaces:** Produce `EngineServing`, `UnixEngineClient`, `WorkspaceStore`, `ReviewProviding`, and Codable mirrors of the public RPC domain.

- [ ] Write Swift tests for RPC decoding, task presentation, store recovery/error state, and explicit Foundation Models unavailability.
- [ ] Run `swift test`; expect missing module/type failures.
- [ ] Add the public protocols, actor-safe transport, observable store, and availability-checked review provider.
- [ ] Re-run `swift test`; expect all tests to pass.

### Task 7: Native macOS workflow

**Files:** `Sources/PatchwrightApp/App/PatchwrightApp.swift`, `Sources/PatchwrightApp/Views/{ContentView,SidebarView,TaskDetailView,EvidenceInspector,SettingsView}.swift`, `Sources/PatchwrightApp/Support/AppCommands.swift`

**Interfaces:** Consume `WorkspaceStore`; produce a `WindowGroup`, `Settings` scene, sidebar/task selection, toolbar actions, approval sheets, searchable timelines, inspector evidence, menus, and keyboard shortcuts.

- [ ] Add deterministic preview fixtures for empty, approval, running, failed, and completed states.
- [ ] Build with `swift build -c release -Xswiftc -warnings-as-errors`; capture the first missing view failure.
- [ ] Add native adaptive views with accessibility labels, Reduce Motion-aware progress, explicit errors, and disabled-action reasons.
- [ ] Rebuild and confirm warnings-as-errors passes.

### Task 8: Product scripts, configuration, and smoke

**Files:** `script/{build_and_run,verify,smoke}.sh`, `.codex/environments/environment.toml`, `.engineering-agent/project.yml`, `README.md`, `docs/{operations,github-app,security,release-checklist}.md`

**Interfaces:** Produce `./script/verify.sh` for all checks, `./script/build_and_run.sh --verify` for the real bundle, and `./script/smoke.sh` for a disposable end-to-end local lifecycle.

- [ ] Add the scripts and run shell syntax checks.
- [ ] Run `./script/verify.sh`; fix only concrete failures until Rust tests, Clippy, Swift tests, and Release build pass.
- [ ] Run `./script/smoke.sh`; verify durable task/worktree/evidence state survives an engine restart.
- [ ] Run `./script/build_and_run.sh --verify`; confirm the exact staged `.app` process is alive.

### Task 9: Final hardening and publication

**Files:** All intentional repository files; no credentials, local databases, sockets, worktrees, or build products.

**Interfaces:** Produce one coherent commit and a draft pull request targeting the remote default branch.

- [ ] Run secret, tracked-artifact, dependency, and full-diff inspection.
- [ ] Re-run `./script/verify.sh`, smoke, and app process verification from final source.
- [ ] Stage explicit paths, commit `build Patchwright stages 1-3`, and inspect the exact staged diff.
- [ ] Push `feat/andrzej_agent_sota_lab` and open a draft PR with scope, safety boundary, checks, and external GitHub App/App Store blockers.

