# Patchwright Production Plan

## Release target

Ship a reviewable Stage 1–3 MVP that runs locally on macOS, exposes a durable
Rust engine, accepts verified GitHub App webhooks, and demonstrates the complete
prepare/verify/deliver/monitor lifecycle. Publish source openly and distribute
only Developer ID-signed, Apple-notarized binaries through GitHub Releases.

## Quality gates

| Gate | Required evidence |
| --- | --- |
| Domain correctness | State, policy, and instruction-resolution unit tests pass |
| Engine integration | Unix-socket RPC, SQLite recovery, and disposable Git worktree tests pass |
| GitHub lifecycle | Signature, deduplication, API request, and status-transition tests pass |
| Native client | Swift tests and Release build pass without warnings |
| Runtime | Built `.app` launches and remains running under `build_and_run.sh --verify` |
| Security | No committed secrets, shell-string RPC, raw credential logging, or default merge capability |
| Documentation | Setup, GitHub App permissions, operations, kill switch, and rollback are reproducible |

## Runtime operations

- Engine shutdown: terminate the `patchwright-engine serve` process; in-flight tasks remain recoverable in SQLite.
- Relay shutdown: terminate `patchwright-relay`; accepted deliveries remain in the durable relay inbox and resume bounded forwarding to the engine after restart. GitHub does not automatically redeliver ingress attempts that failed before the relay returned `202`; an authorized GitHub App owner must request those from **Advanced → Recent deliveries** (or use GitHub's authenticated App-delivery redelivery API). The relay and engine both deduplicate the original delivery ID.
- Task kill switch: cancel a task in the app or call `task.cancel`; the engine kills only the owned child process group and leaves the worktree intact.
- Global kill switch: set `PATCHWRIGHT_AUTOMATION_DISABLED=1`; read-only inspection remains available while all mutating capabilities fail closed.
- Rollback: stop app/engine/relay, revert the release commit, and retain the SQLite database plus task worktrees for inspection.

## Release-status vocabulary

- `repo-ready`: source, tests, build, and local smoke are green.
- `package-ready`: a signed local `.app` bundle and engine/relay binaries are assembled and inspected.
- `release-candidate ready`: clean-machine launch and full lifecycle smoke are green.
- `promoted-release`: the frozen notarized candidate and clean-machine evidence passed independent promotion checks and are eligible for GitHub Releases.
- `blocked:external`: an Apple, GitHub, network, or owner action remains.
