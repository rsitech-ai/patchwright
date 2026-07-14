# Embedded Codex End-to-End Audit

Date: 2026-07-14

Branch: `feat/andrzej_agent_sota_lab`

Readiness: `integration-ready: Codex`

## Decision

The embedded Codex boundary is integration-ready on this Mac with `codex-cli 0.144.2` and the current signed-in ChatGPT account. Patchwright can supervise a task-owned app-server process, persist and resume its thread, stream durable events, bind one-time runtime approvals, pause or cancel safely, and recover after reopening SQLite.

This is not a release-candidate or distribution-readiness claim. Production GitHub App installation access and Developer ID signing/notarization remain separate owner-controlled gates.

## Real disposable smoke

`./script/smoke_codex.sh` verifies the generated 0.144.2 app-server schema and signed-in account, runs the complete fake-server boundary suite, then invokes an ignored real integration test. The real test:

- creates a temporary Git repository outside Patchwright's worktree;
- starts a task-owned Codex app-server with the temporary repository as its working directory;
- starts a new thread and asks for exactly one deterministic `result.txt` file;
- polls and resolves only exact pending Codex runtime approvals, if Codex requests them;
- waits for a persisted terminal turn event and verifies the exact file contents;
- pauses the task, closes the service and database, reopens SQLite, and resumes the same thread under a new process generation;
- cancels the resumed task and verifies the durable terminal task state.

The final run reported `codex-cli 0.144.2`, zero requested runtime approvals, and 69 persisted events. The test prints only the pinned version and aggregate approval/event counts. Thread, turn, process-generation, task, and approval identifiers remain ephemeral and are not committed.

## Live protocol findings retained

The first real runs exposed three differences that the sanitized fixtures did not exercise:

1. Codex 0.144 incoming notifications may omit the JSON-RPC `jsonrpc` member. The decoder now accepts an omitted member while still rejecting any present value other than `2.0`.
2. Status notifications may arrive while a handshake request is awaiting its response. Session request handling now tolerates bounded interleaved events and continues waiting for the matching response, while still refusing server approval requests in handshake-only paths.
3. A terminated app-server descendant can retain the inherited stderr pipe. Process-group termination now performs a final group kill sweep before awaiting stderr capture, with focused repeated process isolation coverage.

Each behavior has a deterministic regression test in addition to the real smoke.

## Inspector interface review

| Before | After | Reason |
| --- | --- | --- |
| Repository and work-item links both looked generic | Repository context and work-item `Open on GitHub` actions are distinct | Makes the navigation target unambiguous |
| Pull-request body appeared as raw Markdown and GitHub HTML | Native heading, list, paragraph, and code-block presentation with GitHub HTML normalization | Restores readable hierarchy and removes raw tags |
| PR state and metadata formed one dense line | State/draft pills, author/time, branches, diff metrics, review, and CI are grouped | Supports rapid queue inspection |
| Disabled preview remained visually prominent without an installation | Preview is absent until available; an explicit GitHub App requirement card explains the blocker | Avoids a false affordance and names the actual gate |
| Discussion and checks were heavy nested cards | Lightweight native sections with counts and rendered comment bodies | Improves scanning in the narrow inspector |

The staged app was relaunched and inspected through macOS accessibility at the supplied three-column window size. The Patchwright PR showed the new hierarchy without raw Markdown headings. A Dependabot PR containing `<details>` markup rendered without raw HTML, and discussion Markdown used the same native renderer. The missing-installation state showed no `Preview Task` action and retained the `Manage GitHub Apps` recovery path.

## Verification matrix

| Gate | Command | Result |
| --- | --- | --- |
| Protocol schema | `./script/verify_codex_schema.sh` | Generated schema and required methods/fields match `codex-cli 0.144.2` |
| Full static/test/build gate | `./script/verify.sh` | Rust fmt/Clippy/workspace tests, focused Codex tests, Swift tests, and macOS build passed |
| Fake + real Codex lifecycle | `./script/smoke_codex.sh` | Disposable real turn, persistence, restart/resume, and cancellation passed |
| Engine smoke | `./script/smoke.sh` | Unix-socket engine smoke passed |
| Staged runtime | `./script/build_and_run.sh --verify` | Staged app launched with its engine and passed the verification probe |
| Secret scan | repository credential-pattern scan | No committed production credential found; fixture-shaped test strings are synthetic |
| Runtime log scan | unified log error/fault/panic/crash filter | No Patchwright crash or panic found during the final staged interaction |

## Remaining external gates

- The selected repositories do not yet expose a production Patchwright GitHub App installation to the app, so repository-bound task creation and approval-gated GitHub delivery remain `blocked: external installation`.
- Developer ID identity, notarization credentials, clean-machine installation proof, and final distribution packaging remain `blocked: external Apple credentials and release operation`.
- Approval-gated merge remains intentionally disabled until its dedicated policy and production-installation gates are completed.
