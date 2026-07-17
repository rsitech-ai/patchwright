# Patchwright Durable Local Orchestrator Design

## Status

- Date: 2026-07-13
- Status: approved design
- Target: macOS 26 or newer on Apple silicon
- Distribution: Developer ID, notarized direct download
- Product boundary: local-first, single operator, multiple repositories

This specification supersedes the earlier Stage 1–3 design where the two conflict. In particular, `mergePullRequest` is no longer permanently disabled. Merge is permitted only through the separate, exact-SHA approval contract defined below.

## Outcome

Patchwright turns ingested GitHub issues and pull requests into durable engineering tasks, runs each task in an isolated worktree with a fully embedded Codex thread, verifies and reviews the result, performs explicitly approved GitHub writes through a production GitHub App, organizes open pull requests into an explainable queue, and eventually performs a separately approved merge.

The finished product must remain recoverable, inspectable, and fail-closed across app restarts, engine crashes, network ambiguity, credential expiry, cancellation, and changing GitHub state.

## Non-goals

- Unattended merges without a fresh human approval.
- Administrator or branch-protection bypass.
- Giving Codex direct access to GitHub, Apple, signing, or approval credentials.
- Treating issue text, comments, labels, or repository files as authority.
- Hosted multi-user orchestration, organization SSO, billing, or arbitrary agent swarms.
- Mac App Store distribution in this release.
- App Sandbox when it would prevent the approved local process and repository workflow.

## Architecture choice

Patchwright uses a durable local orchestrator.

- `PatchwrightApp` is the native operator console.
- `patchwright-core` owns typed state, policy, approvals, queue decisions, findings, and evidence.
- `patchwright-engine` owns persistence, repositories, worktrees, commands, Codex app-server processes, cancellation, and lifecycle orchestration.
- `patchwright-relay` owns GitHub App authentication, typed GitHub API mutations, webhook verification, and remote reconciliation.
- SQLite stores durable state and append-only events. Credentials, source bodies, command output, and private repository content do not become general telemetry.

SwiftUI never executes GitHub mutations. Codex never receives GitHub credentials or Patchwright approval tokens.

## Core data model

### Repository binding

A `RepositoryBinding` contains:

- GitHub repository ID and full name
- GitHub App installation ID
- clone and HTML URLs
- default branch
- optional user-selected checkout
- optional Patchwright-managed clone
- local state and worktree roots
- current default-branch SHA and commit timestamp
- permission and credential-health snapshot

If no local checkout is bound, Patchwright proposes a managed clone under `~/.patchwright/repositories`. Network access is explicit and the clone remains separate from task worktrees.

### Task source

A `TaskSource` is one of:

- local request
- GitHub issue
- GitHub pull request

GitHub sources retain repository identity, item number, immutable source URL, base/head refs and SHAs where applicable, and the snapshot timestamp used to create the contract.

### Task contract

A `TaskContract` contains:

- source and repository binding
- goal and acceptance criteria
- base and optional head identities
- relevant comments, reviews, checks, and changed paths
- effective instruction sources with hashes and precedence
- repository verification commands
- required capabilities
- risk and sensitive-path classification
- queue identity and dependency edges
- Codex process/thread/turn identities
- worktree and branch identities
- evidence, findings, approvals, and remote delivery identities

GitHub and repository content is untrusted context. It cannot modify policy, grant capabilities, change an approval, or enable a tool.

## Task lifecycle

The lifecycle is:

`discovered → assessing → planned → awaitingPreparationApproval → preparing → implementing → verifying → reviewing → awaitingDeliveryApproval → delivering → monitoring → awaitingMergeApproval → merging → completed`

Any nonterminal task may enter:

- `paused`: recoverable operator stop
- `blocked`: needs clarification, credentials, dependency, or external action
- `failed`: operation failed with retained evidence
- `cancelled`: explicitly terminated with retained worktree and evidence

Each transition appends an event and a safe checkpoint. Restart recovery may resume only from a recorded checkpoint and may never skip an approval state.

## Issue-to-task workflow

1. Convert the issue snapshot into a typed contract.
2. Bind or materialize the repository.
3. Resolve instructions, policy, verification commands, and sensitive paths.
4. Present the plan, required capabilities, branch proposal, and intended GitHub actions.
5. After preparation approval, create an isolated worktree and task branch.
6. Start the task-owned Codex app-server process and thread.
7. Run verification and an independent read-only review.
8. Preview commit, push, check, comment, and draft-PR actions.
9. Execute only the approved delivery actions.
10. Monitor CI and reviews and create bounded repair iterations.
11. Request a distinct merge approval only after all readiness gates pass.

## Existing-PR workflow

1. Materialize the exact base and head SHAs.
2. Import requested changes, unresolved review threads, failing checks, conflicts, and changed paths into the contract.
3. Determine whether the GitHub App can update the head repository and branch.
4. If permitted, prepare an isolated worktree from the PR head and push repairs only after approval.
5. If the head is an inaccessible fork, fail safely and offer a local patch, suggested review, or separate repair branch/PR when policy permits.
6. Re-fetch CI, review, and head/base state after every push.
7. Treat review-thread resolution as a separately previewed GitHub write.

## Embedded Codex integration

Each active task owns one supervised `codex app-server` process and process group. Bounded WIP limits keep this isolation affordable.

The engine must:

- discover the exact Codex CLI and version;
- validate the matching generated app-server schema during verification;
- communicate over newline-delimited JSON on stdio;
- perform `initialize` followed by `initialized`;
- read account state and expose signed-out/unavailable conditions;
- start or resume the persisted thread;
- start turns with the isolated worktree as `cwd`;
- persist relevant thread, turn, item, approval, and completion events;
- support user input and `turn/steer`;
- surface command and file-change approval requests in the native UI;
- use `turn/interrupt` for graceful cancellation;
- terminate the owned process group only after an interrupt timeout;
- restart the process and resume the thread after a recoverable crash.

Codex authentication remains owned by Codex. Patchwright does not copy or persist Codex access tokens.

## Approval model

Approval classes remain separate:

1. Codex runtime approval for a command or file change.
2. Local capability approval for network, dependency, or exceptional command access.
3. GitHub delivery approval for exact push, comment, review, check, thread-resolution, branch, or draft-PR actions.
4. Merge approval for one exact PR state.

Approval of one class never authorizes another.

Every Patchwright approval records:

- capability and typed action preview
- repository and task
- relevant branch and PR identity, plus head/base SHAs only when the GitHub mutation atomically consumes them
- approver and timestamp
- short expiration
- policy and instruction hashes
- invalidation conditions

## Approval-gated merge

Merge approval binds to:

- repository and installation
- PR number
- exact head SHA consumed atomically by GitHub's merge endpoint
- configured base-branch identity (GitHub does not expose an atomic expected-base-SHA merge parameter)
- merge method
- required-check snapshot
- review-decision snapshot
- approval identity and expiration
- idempotency key

Before the merge approval gate, Patchwright refreshes the PR, checks, reviews, branch rules, and mergeability. It rejects stale monitored evidence if the head or base changed, required checks regressed, reviews changed, conflicts appeared, or policy no longer permits the merge. The merge mutation itself carries the exact approved head SHA; the base is branch-identity-bound because GitHub offers no atomic expected-base-SHA parameter.

Patchwright never uses administrator bypass. If the repository requires GitHub's native merge queue, Patchwright enqueues the approved PR and monitors the merge-group result. Otherwise it uses the merge endpoint with the expected head SHA. A successful response is recorded with the remote merge SHA before the next queue item may advance.

A post-merge regression stops that repository queue and records a blocker.

## PR queue

Each repository has a durable local queue with these states:

`inbox → assessed → ready | needsWork | blocked → active → awaitingWriteApproval → monitoring → mergeReady → awaitingMergeApproval → merged | failed`

Default deterministic tiers are:

1. manually pinned security, release, or production blockers;
2. PRs that block other queued PRs;
3. green, approved, conflict-free PRs that can complete immediately;
4. PRs with actionable requested changes;
5. PRs with reproducible failing CI;
6. conflicted or behind-base PRs;
7. active drafts with recent commits;
8. stale drafts or PRs needing product clarification.

Within a tier, order by dependency, critical-path risk, latest review or CI activity, latest head commit, latest GitHub update, creation time, and PR number.

Every queue item stores an explainable reason such as `ready and blocks two PRs` or `three required checks failing`. Labels and comments are signals, never authority.

The initial WIP limit is one mutating task per repository. Read-only assessment may run concurrently. Patchwright does not start mutating tasks whose changed-path or dependency graph overlaps an active task.

Supported workflow presets are:

- Quick Wins
- CI Rescue
- Review Closure
- Conflict Recovery
- Dependency Chain
- Security First
- Release Train
- Stale PR Triage
- Draft Completion
- Post-Merge Watch
- Review Load Balancing
- Duplicate/Overlap Detection

Manual pinning and reorder persist locally and do not mutate GitHub.

## Sorting and filtering

Repository sorts are:

- queue priority
- recently updated
- recently pushed
- latest default-branch commit
- open PR count
- failing-check count
- name

PR sorts are:

- queue priority
- recently updated
- latest head commit
- latest review activity
- CI health
- review state
- created newest/oldest
- change size
- PR number

Repository `latest commit` means the latest commit on the default branch. PR `latest commit` means the current PR head commit. Repository `recently pushed` remains a separate GitHub timestamp.

Filters include draft/open state, author, assignee, labels, review state, CI result, merge conflict, age, queue state, and active Codex work. Preferences persist locally per workspace.

## Cancellation

GitHub ingestion becomes a durable job with:

- `github.sync.start`
- `github.sync.status`
- `github.sync.cancel`

Cancellation stops new fan-out, aborts active requests, preserves previously complete snapshots, and never replaces a repository snapshot with partial data.

Task cancellation interrupts the active Codex turn, cancels owned commands and Git processes, prevents new GitHub mutations, reconciles any already-sent idempotent mutation, records a final remote identity when GitHub completed the action, and retains the worktree and evidence.

## Production GitHub App broker

The preferred local credential path is:

1. Import a GitHub App private key through a file panel.
2. Validate the private key.
3. Store the key in macOS Keychain, not SQLite.
4. Allow only the Rust credential broker to request it.
5. Sign a short-lived RS256 app JWT.
6. Resolve the repository installation.
7. Mint a repository- and permission-scoped installation token.
8. Cache the installation token only in memory until before expiration.

For a deployed relay, the private key comes from a protected secret-file mount or secret manager. An environment variable may reference the secret but must not contain raw private-key content.

GitHub CLI remains a development/fallback read-only credential bridge. The production path does not require `gh`.

Typed GitHub adapters cover:

- create/update branches and push task commits;
- create/update draft pull requests;
- post issue/PR comments;
- create pending reviews with batched inline comments;
- submit reviews and reply to or resolve threads;
- create/update check runs;
- update PR branches;
- enqueue or merge PRs;
- close or supersede stale PRs after approval.

Git pushes use an ephemeral credential helper so tokens do not appear in argv, remotes, Git configuration, or logs.

Every mutation has a policy capability, typed preview, fresh precondition check, scoped approval, idempotency key, recorded remote identity, retry classification, and ambiguous-result reconciliation.

## Native macOS interface

The root is an adjustable `NavigationSplitView` with a native source-list sidebar, a queue/repository/task content column, a detail workbench, and an optional SwiftUI inspector.

Primary navigation is:

- Queue
- Repositories
- Active Tasks
- Awaiting Approval
- Monitoring
- Completed

Settings remains a separate macOS scene.

The central PR queue uses a sortable `Table` with priority, repository, PR, queue state, CI, review state, conflict/base state, latest commit, update time, and assigned task. Toolbar actions expose sorting, filters, saved workflows, search, refresh, queue start/pause, and cancellation.

The task workbench has stable modes:

- Overview
- Codex
- Changes
- Verification
- Delivery
- Merge

Evidence, approvals, effective instructions, and credential provenance live in the optional inspector. Fixed-width manual columns are removed. Window-scoped state uses scene storage; durable preferences use app storage.

Approval sheets show the exact target, capability, commit range, changed files, remote content, merge method, checks, reviews, expiration, invalidation conditions, and reason. There is no `Approve Everything` action.

Settings includes real Codex and GitHub App health, versions, account/installation state, permissions, credential source, webhook status, test connection, and actionable recovery.

All empty, loading, partial, cancelled, blocked, expired-approval, credential, and unknown-delivery states are explicit. Icon-only actions have outcome-oriented help and accessibility labels. Raw timestamps are presented as localized relative dates with exact values available in details.

## Distribution

The first release is distributed outside the Mac App Store using Developer ID and notarization.

The release pipeline produces:

- `Patchwright.app`
- signed and notarized `Patchwright.dmg`
- SHA-256 checksum manifest
- version/build metadata
- dependency and license manifest
- verification report
- credential-free reproducibility bundle

The pipeline builds Release artifacts, assembles outside File Provider, validates bundle metadata, signs nested helpers inside-out, signs the app with `Developer ID Application`, Hardened Runtime, secure timestamp, and minimal entitlements, verifies signatures and Gatekeeper, creates and signs the DMG, submits with `notarytool`, retains the notarization log, staples tickets, and verifies the final artifact.

No Hardened Runtime exception entitlement is added without a proven requirement. App Sandbox is not required for direct distribution and is excluded when it prevents the approved local orchestration model.

The current machine lacks a `Developer ID Application` identity and configured notarization credentials. Repository-side packaging is in scope; real Developer ID signing and notarization remain `blocked:external` until the owner installs the certificate and configures a Keychain notary profile.

## Clean-machine validation

The release candidate is installed from the final DMG in a fresh macOS environment with no source checkout, developer toolchain, Patchwright state, cached GitHub token, or running engine.

Proof must include:

- Gatekeeper acceptance and normal installation;
- bundled helper launch and health;
- actionable missing-Codex state;
- Codex install/sign-in followed by app-server connection;
- GitHub App installation discovery without `gh`;
- test-repository ingestion;
- relaunch recovery of queue, task, and thread state;
- update-over-prior-version data preservation;
- offline, expired, revoked, and missing-permission states;
- clear uninstall/data-retention behavior.

## Verification

Implementation uses behavior-first vertical slices.

Required proof includes:

- public domain tests for queue, lifecycle, policy, approvals, merge invalidation, and restart recovery;
- mock GitHub tests for JWT claims, installation tokens, permission scoping, expiry, rate limits, typed writes, exact-SHA merge, merge-queue handoff, idempotency, and ambiguous-result reconciliation;
- fake app-server tests for initialization, account state, thread start/resume, streaming, approvals, steering, interruption, process failure, and isolation;
- a real local Codex smoke in a disposable repository;
- cancellation and restart fault injection at every lifecycle boundary;
- a disposable GitHub App and test repository for the live write/merge workflow;
- Swift tests for store behavior, sorting, filtering, state presentation, and recovery;
- native interaction, accessibility, resize, keyboard, menu, Light/Dark, Reduce Motion, long-content, log, and Release-performance evidence;
- secret, tracked-artifact, signing, notarization, Gatekeeper, and clean-machine audits.

No production repository mutation is used for release qualification.

## Readiness labels

- `repo-ready`: source and local tests/lints/builds pass.
- `integration-ready`: real Codex and disposable GitHub App workflows pass.
- `package-ready`: packaging is reproducible and the available signing path is validated.
- `notarized candidate`: Developer ID, Hardened Runtime, notarization, stapling, and Gatekeeper pass.
- `release-candidate ready`: clean-machine install, update, ingestion, Codex, delivery, cancellation, and approved test-repository merge pass.
- `blocked:external`: an Apple certificate, notary credential, GitHub App registration/installation, or owner decision remains.

## Implementation order

1. Domain/lifecycle/approval migration and durable jobs.
2. Sorting, filtering, timestamps, and native split/table interface.
3. Issue/PR-to-task conversion and repository binding.
4. Embedded task-owned Codex app-server integration.
5. Cancellable ingestion and task cancellation.
6. GitHub App credential broker and read migration.
7. Approval-gated GitHub delivery writes.
8. Explainable PR queue and remediation workflows.
9. Exact-SHA approval-gated merge and native merge-queue handoff.
10. Developer ID packaging, notarization automation, clean-machine proof, and final E2E audit.

## Authoritative references

- Codex App Server: https://developers.openai.com/codex/app-server/
- GitHub App installation tokens: https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app
- GitHub pull-request merge API: https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request
- GitHub merge queue: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue
- Apple Design Resources: https://developer.apple.com/design/resources/
- Developer ID certificates: https://developer.apple.com/help/account/certificates/create-developer-id-certificates/
- Notarizing macOS software: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
