# Patchwright GitHub Ingestion and SwiftUI Audit

## Scope

- Date: 2026-07-13
- Platform: macOS 26, Apple silicon
- App entry point: SwiftPM executable `Patchwright`
- Runtime: signed Release app in `~/.patchwright/staged`, exposed at `dist/Patchwright.app`, with bundled Rust engine
- Primary workflow: authenticated GitHub CLI account → read-only API ingestion → atomic SQLite snapshot → repository/issue/PR navigation in SwiftUI
- Readiness target: repo-ready read-only GitHub workspace ingestion

This was a targeted implementation and audit of the new GitHub ingestion diff. It was not an exhaustive Codex Security scan: the required scan workspace and delegated scan-worker capabilities were unavailable in this task.

## Commands and evidence

| Check | Command or tool | Result | Evidence |
| --- | --- | --- | --- |
| Account | `gh auth status` and authenticated REST calls | verified | Active account `s1korrrr`; token never printed by Patchwright |
| Full account ingestion | `github.sync` with repository limit 100 and resource limit 1,000 | verified | 51/51 repositories in 40 seconds, zero failures |
| Persistence parity | Decode every row in disposable and normal SQLite stores | verified | API summary and persisted totals matched exactly |
| Rust formatting/lint/tests | `./script/verify.sh` | verified | fmt, Clippy `-D warnings`, workspace tests and doc tests passed |
| Swift tests/build | `./script/verify.sh` | verified | 4 tests passed; Release build passed with warnings as errors |
| Engine smoke | `./script/smoke.sh` | verified | Unix-socket health response and non-empty SQLite store |
| Packaged launch | `./script/build_and_run.sh --verify` | verified | Two consecutive rebuild/launch cycles passed strict deep signature checks; app and exact bundled helper both running; zero new crash reports |
| UI smoke | Computer Use accessibility tree | verified | Account repositories, repository snapshot, PR selection, detail, search and sync exercised |
| Runtime logs | macOS unified log after controlled non-automation workflow | verified | No crash, layout recursion, geometry fault, or app error entries; Computer Use-only geometry faults are recorded below |
| Local permissions | `stat` on `~/.patchwright` and database | verified | directory `0700`, SQLite `0600` |
| Secret scan | repository pattern scan | verified | no credential or private-key material found |

## Full-ingestion result

| Record type | API summary | Persisted |
| --- | ---: | ---: |
| Repositories | 51 | 51 |
| Issues and pull requests | 344 | 344 |
| Discussion comments and reviews | 521 | 521 |
| Check runs | 1,092 | 1,092 |
| Workflow runs | 1,298 | 1,298 |

The normal app database at `~/.patchwright/patchwright.sqlite3` contains the same 51-repository workspace. Repository snapshots are replaced only after a complete repository fetch; a failed refresh retains the prior complete snapshot.

## Feature matrix

| Workflow or state | Status | Notes |
| --- | --- | --- |
| Use existing GitHub CLI login | verified | Credential is requested at sync time and retained only in engine memory |
| Discover all currently accessible repositories | verified | 51 found; default raised from 25 to 100 |
| Paginate repository resources | verified | Standard arrays plus check-run and workflow-run wrapper pages |
| Separate issues from PRs | verified | GitHub issue endpoints include PRs; `pull_request` entries are filtered before explicit PR ingestion |
| Ingest issue/PR metadata | verified | Body, author, state, draft, head SHA, labels, assignees and milestone |
| Ingest discussion and review data | verified | Issue comments, review comments and submitted reviews |
| Ingest CI state | verified | PR-head check runs and Actions workflow runs |
| Restart and reload | verified | SQLite restart test and real app relaunch both retained data |
| Search issues and PRs | verified | Non-match removed the PR; matching text restored it |
| Open PR detail | verified | Title, body, GitHub link, discussion state and checks state visible |
| Partial repository failure | verified by integration contract | Failure is reported; previous snapshot is not overwritten |
| Unauthenticated GitHub CLI | verified by error path | Sync returns a typed authentication error; existing snapshots remain readable |
| GitHub mutations | not applicable | This ingestion surface is intentionally read-only |

## Interaction sweep

| Surface | Action | Result | Status |
| --- | --- | --- | --- |
| Toolbar | Sync GitHub | Honest busy state; repositories persisted incrementally; control disabled during sync | verified |
| Sidebar | Select `s1korrrr/patchwright` | Repository counts and work-item list appeared | verified |
| Work-item list | Select PR #1 | Full detail appeared without accessibility bridge failure | verified |
| Search | Enter impossible query | Work-item result disappeared | verified |
| Search | Enter `Build Patchwright` | PR result returned | verified |
| Repository link | Hover/accessibility description | Described as opening the repository on GitHub | verified |
| Comment link | Hover description | Described as opening the comment on GitHub | verified by code and build |
| Inspector toolbar item | Help text | Describes evidence and ingestion details | verified by accessibility tree/build |
| New Task | Existing local-repository sheet | Outside the ingestion diff; covered by prior Stage 1–3 MVP checks | not re-audited |

## Issues found and fixed

| Severity | Area | Finding | Fix and proof |
| --- | --- | --- | --- |
| High | Credential boundary | An untrusted pagination `Link` could redirect the bearer token to another origin | Pagination now accepts only the configured API origin; dedicated cross-origin token test passes |
| High | RPC framing | Swift assumed a complete JSON response arrived in one socket receive, breaking large snapshots | Added bounded 64 MiB newline framing with repeated 64 KiB receives; fragmented-response test passes |
| Medium | Filesystem safety | A stale-socket cleanup could remove a non-socket file at a caller-provided path | Engine now rejects non-socket paths; preservation regression test passes |
| Medium | Data completeness | Labels, assignees and milestones were discarded | Added backward-compatible Rust/Swift fields, fixture assertions and native detail rendering |
| Informational | UI automation | Computer Use clicks emitted paired AppKit negative-geometry faults | The same Release app auto-selected the large repository without Computer Use and emitted zero geometry faults; the evidence identifies this as an automation-bridge artifact, not an ordinary runtime defect |
| Medium | Packaging smoke | The build script could leave its bundled helper alive and report a Launch Services false negative | Script terminates only the exact bundle-owned helper, waits, and verifies both processes |
| Blocker | Code signing | Copying helpers and rewriting `Info.plist` invalidated the linker signature; a bundle staged inside the Documents workspace could also regain File Provider/Finder metadata immediately after cleanup, and macOS killed some launches with `Taskgated Invalid Signature` | Stage and sign in the user-only `~/.patchwright` directory, expose a stable `dist/Patchwright.app` symlink, and run strict verification before and after launch; two rebuild/launch cycles and zero new crash reports prove the fix |
| Medium | Availability | GitHub requests had no explicit deadline | Added 10-second connect and 30-second request timeouts |
| Medium | Sync latency | Per-PR review/check requests created a several-minute sequential tail that outlived the native RPC connection | Added bounded eight-way per-PR fetching; full 51-repository replay fell to 40 seconds |
| Polish | Toolbar clarity | Icon-only actions lacked explicit hover descriptions | Added outcome-oriented help for create, inspector and sync actions |

## Security boundary review

- `gh auth token` is obtained through an argv-safe `Command`, redacted from `Debug`, never serialized, and never stored in SQLite.
- Pagination is same-origin and bounded. Repository fan-out is capped at 100, per-resource ingestion at 1,000, and repository concurrency at four.
- GitHub text is decoded as data and rendered as SwiftUI `Text`; it is not treated as policy or executable markup.
- Snapshot writes use parameterized SQLite queries and transactionally replace one complete repository snapshot at a time.
- The app-owned state directory is `0700`; the database is forced to `0600` on open.
- The local RPC socket has no separate application-layer authentication. Its current control is the user-only parent directory; it must not be exposed or moved to a shared directory.
- No GitHub write operation is reachable from this ingestion UI. Merge remains disabled by policy.

## Visual and performance review

- Native `NavigationSplitView`, sidebar, toolbar, searchable work-item column and semantic system icons are used.
- Repository and work-item panes have adjustable min/ideal/max widths; the detail remains readable at the staged default size.
- The busy overlay reports the number of locally available repositories and disables duplicate sync requests.
- Four-repository engine concurrency keeps the 51-repository sync bounded without unbounded request fan-out.
- The selected repository refreshes after sync, so visible detail does not silently remain stale.
- No custom animation or global implicit animation was introduced; Reduce Motion behavior is therefore native.

## Remaining risks and boundaries

- A long sync is not yet cancellable from the UI. The engine remains fail-safe and snapshots are committed per completed repository, but a user must wait or stop the local engine.
- The read-only CLI credential bridge is suitable for the local operator workflow. GitHub App installation-token brokering is still required before write-capable Stage 3 automation.
- The staged app is ad-hoc signed and not notarized. Packaging, signing and clean-machine validation remain separate release gates.
- The ingestion cap is deliberately 1,000 records per resource per repository. Repositories exceeding that cap report a bounded snapshot rather than attempting unbounded local ingestion.

## Final readiness label

- Label: **repo-ready for read-only GitHub ingestion**
- Not claimed: package-ready, notarized release-candidate, or write-capable GitHub lifecycle
- Next product slice: turn a selected ingested issue or PR into a typed Patchwright task while preserving the existing approval and no-merge boundaries
