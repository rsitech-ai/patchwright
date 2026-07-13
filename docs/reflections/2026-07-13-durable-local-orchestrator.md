# Durable Local Orchestrator Design Reflection

## Task

- **ID/Title:** Patchwright durable local orchestrator expansion
- **Date:** 2026-07-13
- **Scope:** repo-wide design

## Plan and Risks

- **Planned approach:** Extend the existing local-first engine through dependency-ordered vertical slices. Keep SwiftUI as the operator console, make the Rust engine the lifecycle authority, isolate one Codex app-server process per active task, centralize GitHub writes behind a production GitHub App broker, and retain independently verifiable release gates.
- **Top failure hypotheses:** A stale or overly broad approval could merge changed code; Codex or Git commands could outlive cancellation and mutate the wrong worktree; installation-token or signing credentials could leak through argv, logs, persistence, or model context.
- **Success criteria:** One ingested issue and one existing PR complete the typed lifecycle in a disposable repository; every remote mutation is previewed, scoped, idempotent, and recoverable; a merge requires a fresh exact-SHA approval; cancellation and restart fault tests pass; the final Developer ID artifact passes notarization, Gatekeeper, and clean-machine validation when external credentials are present.

## Candidate Attempts

| Candidate | Summary | Outcome | Signals | Why selected / rejected |
|---|---|---|---|---|
| A | Durable local Rust orchestrator with native SwiftUI control plane | Selected | Matches existing boundaries, supports restart recovery and fail-closed approvals | Best balance of local privacy, auditability, and incremental delivery |
| B | SwiftUI invokes `git`, `gh`, and `codex` directly | Rejected | Fewer components initially | Approval enforcement, cancellation, idempotency, and recovery would fragment across UI code |
| C | Hosted orchestration service first | Deferred | Strong future team/remote potential | Adds infrastructure and credential scope before the single-operator workflow is complete |

## Reflection

- **Failure modes observed:** The baseline presents a Stage 1–3 architecture but currently exposes only task creation/timeline and read-only GitHub ingestion. The Codex setting is not connected. Merge is hard-denied. The package is ad-hoc signed, and the current Keychain has no Developer ID Application identity.
- **Root cause:** The baseline implemented domain and integration scaffolding before wiring the complete user-visible lifecycle and external release credentials.
- **Fix that resolved it:** Not applicable yet; this reflection establishes the retained design and rollback boundaries before implementation.
- **What improved score/quality:** Separating Codex approvals, local capabilities, GitHub delivery, and merge approval; binding merge to exact remote state; using a task-owned app-server process; and splitting repository readiness from external Apple/GitHub gates.
- **Useful command-level evidence:** `git status --short`; `codex --version`; `codex app-server --help`; `security find-identity -p codesigning -v`; source inspection of RPC, policy, relay, SwiftUI, release, and security files.
- **Branch comparison insight:** The working branch contains authenticated read-only GitHub ingestion and package hardening beyond `origin/main`, but the complete task/Codex/write/merge lifecycle remains unimplemented.

## Reusable Lesson

- **Pattern that worked:** Bind every irreversible remote action to a typed preview, fresh preconditions, short-lived approval, idempotency identity, and reconciliation path.
- **Pattern to avoid:** Treating a model/runtime approval or a broad delivery approval as authority for later remote state.
- **Where to apply next:** Task preparation, GitHub pushes, review replies, check publication, merge, signing, and notarization.

## Decision

- **Final chosen approach:** Durable local orchestrator with task-owned embedded Codex, production GitHub App broker, explainable bounded PR queue, exact-SHA approval-gated merge, and Developer ID direct distribution.
- **Commit/rollback decision:** Commit the approved design only. Implementation will land in independently revertible vertical slices; the global kill switch and read-only ingestion remain the retained rollback path.
- **Next step / follow-up:** User reviews the committed specification, then a dependency-ordered ExecPlan is written before code changes.
