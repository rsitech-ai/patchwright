# Patchwright architecture

Patchwright is a local-first engineering control plane for one macOS operator
working across GitHub repositories. It turns local requests, issues, and pull
requests into durable tasks, runs approved work in isolated worktrees, records
verification evidence, and keeps every remote mutation behind an exact,
short-lived approval.

## System boundary

Patchwright is composed of four cooperating modules:

- `PatchwrightApp` is the native SwiftUI operator console.
- `patchwright-core` owns typed state, policy, approvals, queue decisions, and
  evidence contracts.
- `patchwright-engine` owns SQLite persistence, repositories, worktrees,
  command execution, Codex sessions, cancellation, and recovery.
- `patchwright-relay` verifies GitHub webhooks and performs typed GitHub App
  mutations after the engine presents a valid approval.

The SwiftUI process does not perform GitHub writes. Codex does not receive
GitHub App credentials or Patchwright approval tokens. Repository text, issue
content, comments, and model output are untrusted context rather than authority.

## Durable state and recovery

SQLite stores tasks, append-only events, approval records, evidence metadata,
webhook delivery identities, remote cursors, and safe lifecycle checkpoints.
Credentials, raw repository content, prompts, command output, and diffs are not
general telemetry.

A task progresses through explicit states:

`discovered → assessing → planned → awaitingPreparationApproval → preparing → implementing → verifying → reviewing → awaitingDeliveryApproval → delivering → monitoring → awaitingMergeApproval → merging → completed`

Tasks may also be paused, blocked, failed, or cancelled with their evidence and
worktree retained. Restart recovery resumes only from a durable checkpoint and
never skips an approval state or blindly repeats an ambiguous remote write.

## Repository execution

Each mutating task receives an isolated Git worktree and branch. Commands are
represented as an executable plus argument vector, working directory, timeout,
and network policy; shell strings are not accepted at the engine boundary.
Repository-controlled verification commands are not OS-sandboxed, so the app
shows the exact commands and requires a separate confirmation immediately
before running them.

Instruction sources retain their path, content hash, scope, precedence, and
enforcement status. Project and directory instructions may constrain a task but
cannot grant new capabilities.

## Codex integration

Each active coding task owns a supervised Codex App Server process, thread, and
turn identity. The engine performs protocol initialization, starts or resumes
the recorded thread, streams typed events, surfaces runtime approvals, supports
steering and interruption, and terminates the owned process group only after a
graceful cancellation timeout.

Protocol input has per-line and request-wide duration, event-count, and byte
budgets. Responses and completion events must match the exact active request,
thread, and turn. Credential-shaped event or approval content is redacted before
durable storage. Codex authentication remains owned by Codex; Patchwright does
not copy or persist its access tokens.

## GitHub integration

Read-only ingestion can use the GitHub CLI credential already present on the
operator's Mac. Snapshots cover repositories, issues, pull requests, comments,
reviews, review threads, checks, and Actions runs. Each repository refresh is
atomic: a failed refresh preserves its previous complete snapshot. One global
budget bounds nested pull-request fan-out.

GitHub writes use a bring-your-own GitHub App. The private key is referenced
through macOS Keychain or an owner-only file, and installation tokens remain in
memory. Every mutation has:

- a typed action preview;
- repository, pull-request, branch, and relevant SHA identities;
- a policy capability and short-lived approval;
- a fresh precondition check;
- an idempotency key and remote result identity;
- an explicit retry or ambiguous-result reconciliation class.

Drafting, readying, closing, reviewing, resolving threads, pushing, and merging
must consume the exact approved identities supported by the corresponding
GitHub operation. Merge never uses administrator bypass and always requires a
separate merge-class approval.

## Native interface

The app uses native macOS navigation, tables, inspectors, menus, keyboard
commands, settings, and approval sheets. The primary surfaces are Queue,
Repositories, Active Tasks, Awaiting Approval, Monitoring, and Completed.

Approval sheets show the exact target, capability, changed files, commit range,
remote action, expiry, and invalidation conditions. There is no global
"approve everything" action. Loading, empty, partial, cancelled, credential,
blocked, expired-approval, and unknown-delivery states remain explicit.

## Distribution lanes

The official direct-download lane requires a clean tagged commit, Developer ID
Application signing, Hardened Runtime, Apple notarization and stapling,
Gatekeeper verification, digest-bound release evidence, independent promotion,
and clean-machine validation. The signed Sparkle feed belongs only to this lane.

A separate community-prerelease lane may package an exact-tag ad-hoc-signed app
for source review and evaluation. Its manifest explicitly records
`notarized: false`; it is never published through Sparkle or represented as an
official trusted binary.

## Readiness labels

- `repo-ready`: source checks, tests, and builds pass.
- `integration-ready`: real Codex and disposable GitHub App workflows pass.
- `package-ready`: the intended packaging and signing path is validated.
- `notarized candidate`: Developer ID, notarization, stapling, and Gatekeeper
  verification pass for one frozen artifact.
- `release-candidate ready`: the exact candidate passes clean-machine install,
  update, integration, cancellation, and recovery checks.
- `community prerelease`: a checksum-bound ad-hoc build is downloadable but is
  not Developer ID signed or Apple notarized.
- `blocked:external`: an owner credential, account action, or independent
  distribution proof is still required.

See [production operations](production-plan.md), [security operations](security.md),
and [release procedures](RELEASING.md) for the maintained operating contracts.
