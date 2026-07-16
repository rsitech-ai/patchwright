# Patchwright Stages 1–3 Design

## Product contract

Patchwright is a local-first engineering control plane for one developer operating one GitHub repository at a time. It turns a local repository or authorized GitHub issue into an auditable task that can be reviewed locally, prepared in an isolated worktree, verified, delivered as a draft pull request, and monitored through GitHub feedback and CI.

The first release includes the complete product loop described as Stages 1–3 in the source brief. It does not include Stage 4 team administration, hosted runners, organization SSO, automatic merge, arbitrary agent swarms, or iOS execution.

## Target and support

- Native SwiftUI application for macOS 26 or newer on Apple silicon.
- Local Rust 2024 engine for macOS and future headless portability.
- Rust webhook relay suitable for a local tunnel or a separately operated HTTPS deployment.
- GitHub App credentials supplied at runtime and never stored in the repository.
- Codex App Server discovered from the local `codex` executable.
- Apple Foundation Models used only when available; unavailability is explicit and recoverable.

## Primary workflow

1. The developer adds a local repository.
2. Patchwright resolves repository policy and hierarchical `AGENTS.md` instructions.
3. The developer selects a base/head comparison or enters a GitHub issue.
4. Patchwright constructs a typed implementation contract and shows required capabilities.
5. An approval creates an isolated worktree and starts the coding runtime.
6. Configured format, lint, test, and build commands run under command policy.
7. A separate read-only review pass produces structured, evidence-backed findings.
8. An approval commits, pushes, and creates a draft pull request.
9. GitHub webhook events update the durable task timeline; CI and review feedback can trigger bounded repair proposals.
10. Patchwright returns control to the developer. Merge remains outside the product.

## Architecture

### `PatchwrightApp`

The SwiftUI app uses a `WindowGroup` with a sidebar-detail-inspector layout and a separate `Settings` scene. App-wide state lives in `WorkspaceStore`; selection remains window-scoped. The app presents repositories and tasks in the sidebar, a task/evidence timeline in the detail pane, and effective instructions, approvals, and findings in the inspector. Primary actions have menu and keyboard equivalents.

`EngineClient` talks JSON-RPC 2.0 over a Unix domain socket. `FoundationReviewProvider` uses Apple Foundation Models behind a `ReviewProviding` protocol and reports exact availability and recovery states. The UI never executes Git or GitHub mutations directly.

### `patchwright-core`

The shared Rust library owns domain types, validated identifiers, the task state machine, approval/action policy, instruction precedence, structured findings, evidence records, GitHub event envelopes, and typed error codes. Domain types are serializable and form the JSON-RPC contract.

### `patchwright-engine`

The local engine owns repository inspection, worktrees, command execution, Codex App Server sessions, durable SQLite state, and evidence capture. Mutating actions are possible only after an action-specific approval token. Every command has a configured executable, argument vector, working directory, timeout, and network policy. Shell strings are not accepted at the RPC boundary.

### `patchwright-relay`

The relay verifies GitHub webhook HMAC signatures before parsing events, deduplicates deliveries, records sanitized lifecycle events, and forwards typed events to the engine. Its GitHub client obtains installation tokens from a GitHub App JWT and supports draft PRs, check runs, pending reviews with batched comments, replies, and CI status reads. Private keys and installation tokens stay in memory and logs redact authentication headers.

## Task states and approvals

Task transitions are explicit:

`discovered → planned → awaitingApproval → preparing → implementing → verifying → reviewing → awaitingDeliveryApproval → delivering → monitoring → completed`

Any active state can transition to `failed` or `cancelled`. Recovery replays durable events and may continue only from a recorded safe checkpoint.

Capabilities are `readRepository`, `modifyWorktree`, `runKnownCommand`, `accessNetwork`, `installDependency`, `pushBranch`, `createPullRequest`, `postReview`, `resolveThread`, `modifyWorkflow`, and `mergePullRequest`. The shipped defaults automatically allow the first three only inside an isolated worktree, require approval for network and all GitHub mutations, always require a distinct approval for workflow changes, and disable merge.

## Instruction resolution

Instruction sources use this precedence, from lowest to highest:

1. organization policy
2. user preferences
3. `.engineering-agent/project.yml`
4. root `AGENTS.md`
5. nearest directory `AGENTS.md`
6. branch or pull-request instructions
7. current task instruction

The resolver preserves source path, content hash, scope, precedence, advisory/enforced status, and conflicts. Repository text and GitHub comments are untrusted context, never authority.

## Persistence and evidence

SQLite stores tasks, append-only task events, approvals, evidence, webhook delivery IDs, and remote cursors. Repository contents, prompts, command output, and diffs remain local. Evidence entries contain a content hash, timestamp, producer, task/step relationship, and local artifact path. Sensitive environment variables are never persisted.

## Error behavior

- Invalid RPC input returns a stable typed code and does not mutate state.
- A missing repository, dirty base checkout, policy denial, stale approval, timeout, failed verification, rejected webhook signature, and GitHub rate limit are distinct errors.
- GitHub mutations carry idempotency keys derived from task, action, and head SHA.
- Restart recovery never repeats a mutation whose successful remote identity was recorded.
- Model unavailability is visible and allows deterministic/Codex alternatives when policy permits.

## Verification contract

- Rust unit tests cover state transitions, policy decisions, instruction ordering, signature verification, delivery deduplication, and command allowlisting.
- Rust integration tests cover JSON-RPC over a real Unix socket, SQLite restart recovery, a real temporary Git worktree, and mocked GitHub HTTP responses.
- Swift tests cover decoding, store state, Foundation Models availability mapping, and task presentation.
- The repository provides one command for all tests and a project-local build/run script that stages and verifies the real `.app` bundle.
- A smoke fixture initializes a disposable repository, creates a review task, prepares a worktree, runs a harmless verification command, restarts the engine, and verifies that evidence survives.

## Privacy and release boundary

Patchwright is local-first. The first release collects no product analytics. Unified logs contain identifiers and state changes, not source contents, prompts, tokens, or command output. The app uses the smallest feasible sandbox/entitlement set; repository access is user-selected. The local build can be release-candidate quality, but App Store upload readiness remains blocked on final bundle identity, signing profile, store metadata, privacy declarations, and owner authorization.

