# GitHub Ingestion Implementation Plan

**Goal:** Let an authenticated local user ingest and browse their accessible GitHub repositories, issues, pull requests, discussion, reviews, checks, and Actions runs without persisting credentials.

**Architecture:** `gh auth token` is a local credential broker; the token exists only in process memory. A paginated Rust GitHub source maps REST payloads into stable domain records, SQLite atomically replaces each repository snapshot with sync provenance, JSON-RPC exposes sync/query methods, and the SwiftUI client presents account/repository/work-item navigation.

**Tech Stack:** Rust 2024, Reqwest, Rusqlite, Tokio, GitHub REST API version `2026-03-10`, Swift 6.3, SwiftUI.

## Global constraints

- Initial live proof is read-only; no comments, labels, branches, reviews, checks, or pull requests are mutated.
- Never persist or log bearer tokens, authorization headers, private repository bodies, or raw webhook secrets.
- Pagination is mandatory and bounded by an explicit repository limit and per-resource page limit.
- Issues returned with a `pull_request` key are not duplicated as issues.
- A failed repository sync preserves its previous complete snapshot and records a sanitized failure.
- UI shows exact connection, syncing, partial, empty, and failure states.

### Task 1: GitHub source tracer

- Add a mock-server integration test for authenticated user discovery, paginated repository discovery, issue/PR separation, and authorization-header presence.
- Implement validated GitHub records, `GhCliCredentialBroker`, Link-header pagination, and `GitHubSource::snapshot`.
- Run the focused test RED then GREEN.

### Task 2: Atomic snapshot store

- Add a restart test that writes account/repository/work-item/review/check/run records, replaces one repository snapshot, and proves failed replacement retains prior data.
- Add schema-v2 tables and one-transaction `replace_github_snapshot` plus typed queries.
- Run the focused test RED then GREEN.

### Task 3: Engine ingestion surface

- Add RPC/CLI tests for `github.status`, `github.sync`, `github.repositories`, and `github.repository`.
- Add bounded orchestration, progress summaries, sanitized errors, and live `ingest-github` CLI output.
- Run socket and CLI integration tests.

### Task 4: Native GitHub workspace

- Add Swift decoding/store tests for account, repository, issue/PR, review/check/run, empty, syncing, partial, and failure states.
- Add GitHub sidebar, repository dashboard, work-item detail, sync toolbar/menu, and read-only provenance inspector.
- Run Swift tests and warnings-as-errors release build.

### Task 5: Live read-only proof and audit

- Sync the authenticated `s1korrrr` account with a bounded repository limit into a disposable database and compare representative repository/PR counts with direct `gh api` reads.
- Build/launch the app against that database, exercise sync/navigation/relaunch/error recovery, and inspect logs.
- Run security/diff audit, full verification, smoke, exact app launch, commit, push, and update draft PR #1.

## Progress

- [x] Authenticated, same-origin, paginated GitHub source with redacted credentials.
- [x] Atomic SQLite snapshots with account, repository, issue/PR, discussion, check, and Actions records.
- [x] JSON-RPC sync/query surface with bounded four-way repository fan-out and partial-failure preservation.
- [x] Native sidebar, repository/work-item browser, search, sync state, failure state, and provenance inspector.
- [x] Live disposable sync: 51/51 repositories, 344 work items, 521 discussion records, 1,092 checks, and 1,298 workflow runs with no failures.
- [x] Direct API parity for `s1korrrr/patchwright` and `s1korrrr/devscope`.
- [x] Security regressions cover cross-origin pagination credential forwarding and database file permissions.
- [ ] Final packaged-app interaction sweep, full verification, commit, push, and PR update.

## Decisions and outcomes

- GitHub CLI remains the local credential broker for this stage; GitHub App installation credentials remain an external deployment concern.
- The initial account snapshot is intentionally read-only and bounded to 100 repositories and 1,000 records per resource class per repository.
- The evidence inspector starts closed because opening it during initial SwiftUI window layout reproducibly emitted negative-geometry AppKit faults. Opening it after launch is clean.
- A second engine process now refuses to replace a healthy Unix socket, so app relaunches cannot disconnect the existing owner.
