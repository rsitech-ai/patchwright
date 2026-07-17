# Patchwright Product Quality Hardening

Date: 2026-07-17

## Scope and authority

This pass remediated the four findings from the end-to-end native product audit:

1. Historical version-1 task contracts failed to decode after evidence fields became mandatory.
2. Completed tasks still exposed delivery and merge workbench controls.
3. The populated nine-column pull-request queue emitted negative AppKit geometry faults during accessibility enumeration.
4. Task-preview copy implied that local contract review required GitHub App installation.

The pass was authorized to change local product code and tests. It did not create credentials, mutate GitHub work items, or rewrite historical SQLite rows.

## Official-documentation alignment

- Apple documents `Table` as the native SwiftUI container for tabular data. The implementation keeps that semantic control and changes only the column set at a tested content-width boundary: <https://developer.apple.com/documentation/swiftui/table>.
- Apple documents custom decoding as the compatibility boundary for external serialized representations. Swift independently validates supported versions, required content, and integrity evidence instead of accepting a structurally decodable payload: <https://developer.apple.com/documentation/swift/encoding-decoding-and-serialization>.
- Serde documents that untagged variants are attempted in order and that `deny_unknown_fields` rejects fields not declared by a wire shape. The Rust snapshot decoder uses an explicit strict-first/read-only-second flow, with a denied-field legacy shape so malformed or partially upgraded payloads cannot silently downgrade: <https://serde.rs/enum-representations.html> and <https://serde.rs/container-attrs.html>.
- GitHub documents that installation permissions govern GitHub API capabilities and recommends minimum required permissions. The UI now describes GitHub App access only at the remote-mutation boundary, while local contract review remains available: <https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app>.

## Findings and remediation

### Historical contract compatibility

- Category: objective defect
- Severity: high
- Evidence before: `task.contract` returned RPC error `-32000 persistence failure` with `decode task contract` for both completed local records.
- Root cause: evidence-bound fields were added while both the prior and current serialized shapes remained version `1`.
- Remediation:
  - New evidence-bound contracts serialize as version `2`.
  - Evidence-bound version-1 records remain executable for compatibility.
  - Pre-evidence version-1 records decode through a validated `TaskContractSnapshot` as read-only audit evidence.
  - Execution, preparation, verification, and delivery continue using the strict `TaskContract` decoder; no hashes or verification commands are fabricated.
  - Swift models independently reject malformed partial integrity evidence.
- Runtime verification: direct RPC calls for the two existing completed records now return their original version, goal, criteria, and empty historical command list without an error. The running UI labels them `Version 1 · Read only` and explains that they cannot prepare, deliver, or merge changes.
- Status: fixed and verified.

### Terminal task immutability

- Category: objective defect
- Severity: medium
- Evidence before: a completed pull-request task still displayed the workbench and an enabled `Preview Exact Merge` button.
- Root cause: presentation policy handled cancelled and blocked tasks but treated completed tasks as generally interactive.
- Remediation: `TaskSurfaceState` now resolves completed tasks to a dedicated read-only surface. The workbench picker and all preparation, delivery, and merge controls are absent while overview, lifecycle, source, and contract evidence remain visible.
- Runtime verification: accessibility inspection of the completed task showed the full audit trail and historical contract, with no workbench tabs or mutation controls.
- Status: fixed and verified.

### Pull-request table accessibility geometry

- Category: objective defect
- Severity: high
- Evidence before: accessibility enumeration of the populated queue emitted `Invalid view geometry: width is negative` and `height is negative`. Empty, filtered-small, Issues, and Repositories tables did not reproduce the fault.
- Root cause: the normal 870.5-point content split attempted to expose nine columns whose aggregate mandatory width exceeded the available layout while both axes scrolled.
- Remediation: the queue now selects a three-column compact table below 1,050 points and retains the expanded nine-column table for genuinely wide content panes. Compact rows preserve priority, PR identity, repository, author, CI, review, and update time.
- Runtime verification: the populated 32-row queue exposed three semantic columns at 870.5 points. Repeated accessibility captures completed successfully, and the final Patchwright process emitted no negative-geometry messages, engine failures, or product-owned errors/faults. Unified logging still contains macOS-owned App Intents, Spotlight, and network-metadata diagnostics when their host services are unavailable; these are not emitted by Patchwright code and did not affect health or interaction.
- Status: fixed and verified.

### GitHub App boundary copy

- Category: refinement opportunity
- Severity: polish
- Evidence before: `Preview Task` succeeded locally while its tooltip said it would verify GitHub App access, alongside an installation warning.
- Remediation: the tooltip now says `Preview the local task contract`. The warning explicitly distinguishes available local/read-only behavior from GitHub App access required for remote mutation previews.
- Runtime verification: the corrected tooltip and explanatory warning were present together in the running issue detail.
- Status: fixed and verified.

## Regression coverage

- Rust core fixture for legacy read-only snapshot decoding and version-2 contract creation.
- Engine Unix-socket RPC fixture proving a real legacy payload is returned for audit.
- Existing strict persistence test proving evidence-free contracts remain rejected by execution paths.
- Swift decoding fixtures for legacy records and malformed partial integrity evidence.
- Workspace-store fixture proving legacy contract refresh no longer creates a lifecycle error.
- Presentation-policy coverage for completed tasks and compact/expanded queue density.
- Source contract coverage for the local-preview versus remote-mutation copy boundary.

## Runtime interaction evidence

- Populated Pull Request Queue: verified.
- Completed historical task evidence: verified.
- Terminal mutation-control removal: verified.
- Issue detail and task-preview copy: verified.
- Engine health after rebuild: `ok`, version `0.1.0`.
- Existing SQLite records: preserved; no in-place migration or rewrite performed.
- External GitHub writes: not performed.

## Verification evidence

- `SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" ./script/verify.sh`: passed after the final code change.
- Rust workspace tests, clippy/format gates, Swift tests (64 tests, 0 failures), production builds, release/security/compliance checks, and the 29-category promotion matrix: passed.
- `./script/build_and_run.sh --verify`: passed for the final staged bundle.
- `codesign --verify --deep --strict`: passed for the staged app and bundled helpers.
- Direct `system.health` RPC: `ok`, version `0.1.0`.
- Direct `task.contract` RPC: all three persisted records returned successfully, including historical version-1 records.
- One real Codex disposable lifecycle test remains intentionally ignored because it requires a signed-in Codex account and available external quota; all offline and deterministic coverage passed.

## Readiness

The changed surfaces are interaction-clean and runtime-proven for the reproduced findings. Repository-wide local verification and the final diff review passed. Hosted PR/CI evidence remains the final merge gate and is recorded by GitHub rather than this local report.
