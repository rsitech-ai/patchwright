# Orchestrator foundation audit — 2026-07-13

## Readiness

**Label: repo-ready orchestrator foundation.** The durable task model, persisted GitHub snapshot, issue/PR conversion boundary, deterministic sorting/filtering, and native macOS workbench are implemented and locally verified.

This gate does **not** claim embedded Codex readiness, approval-bound GitHub write or merge readiness, production GitHub App readiness, or signed/notarized distribution readiness. Those remain separate plans and gates.

## Verified scope

| Surface | Evidence | Result |
| --- | --- | --- |
| Engine and staged app | `./script/build_and_run.sh --verify`; staged `Patchwright.app` and bundled engine remained running over the Unix socket | Pass |
| Persisted ingestion | Relaunched app restored 51 repositories and 26 open pull requests from `~/.patchwright/patchwright.sqlite3` | Pass |
| Pull request ordering | Applied Recently Updated; `s1korrrr/patchwright#1` moved ahead of older PRs after the table refresh | Pass |
| Repository ordering | Applied Recently Updated; `s1korrrr/patchwright` and `s1korrrr/clip_vault` moved to the top | Pass |
| Search | Searching `patchwright` reduced the PR table to `s1korrrr/patchwright#1` | Pass |
| PR inspection | Rendered title, author, exact/relative time, long Markdown body, discussion, and checks from the local snapshot | Pass |
| Conversion boundary | Previewing `s1korrrr/patchwright#1` failed closed with `Install the Patchwright GitHub App for this repository before creating tasks.` | Pass, expected external blocker |
| Inspector | Opened repository snapshot, ingested-record, and credential-handling evidence | Pass |
| Empty navigation | Issues shows an explicit empty state and clears the prior PR detail rather than presenting stale selection | Pass |
| Window behavior | AppKit zoom/resize retained the three-column workbench without losing navigation or table content | Pass |
| Appearance and motion | Current Dark appearance inspected; Light appearance and Reduce Motion were not changed during this run | Manual follow-up |

The current local snapshot contains no open issues, so the Issues table correctly presents `No Issues — Sync GitHub to ingest open issues.` The engine and Swift store paths for issue ingestion and typed conversion are covered by fixtures; a production GitHub App installation is still required before converting a live repository item into a locally bound task.

## Defects found and retained fixes

1. Nested SwiftUI `GroupBox` detail cards caused accessibility inspection to terminate when a table selection revealed a long PR. Minimization proved the outer split view, scroll view, header, and long body were healthy. Replacing the cards with bounded vertical detail cards retained all content and made the full detail tree inspectable.
2. Changing to an empty workspace section left the previously selected PR in the detail column. A store regression test now requires section changes to clear incompatible task, repository, work-item, preview, and error selection.
3. Computer Use row selection emits AppKit `Invalid view geometry` diagnostics even when the selected detail is reduced to a single `Text`. It does not crash or hang the app, and full accessibility inspection succeeds after the detail-card fix. Classification remains a Computer Use/native-Table interaction artifact pending a human click comparison; it is not counted as clean runtime-log proof.

No correct unit-test seam exists for the SwiftUI/AppKit geometry diagnostic itself. The original staged-app interaction is therefore the regression loop; store navigation behavior is locked down with a focused unit test.

## Automated evidence

Commands run during this gate:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
swift test
swift build -c release -Xswiftc -warnings-as-errors
./script/build_and_run.sh --verify
/usr/bin/codesign --verify --deep --strict ~/.patchwright/staged/Patchwright.app
```

The final verification script completed with 47 Rust tests and 23 Swift tests, plus the focused sorting, conversion, presentation, and RPC suites.

The local database is `0600` (`-rw-------`). No live GitHub mutation, review, check, PR delivery, or merge was attempted in this foundation gate.

## Rollback and open gates

The retained rollback point before this workbench slice is commit `b500285` (`Convert GitHub work items into durable tasks`). The workbench commit can be reverted independently without deleting the SQLite snapshot.

Open gates:

- Production GitHub App registration, installation-token brokering, and repository installation.
- Embedded, supervised Codex thread lifecycle and restart recovery.
- Approval-bound branch, comment, review, check, draft-PR, and exact-SHA merge operations.
- Long-running sync cancellation.
- Developer ID identity, notarization credentials, clean-machine validation, and distribution packaging.
- Manual Light appearance and Reduce Motion interaction pass.
