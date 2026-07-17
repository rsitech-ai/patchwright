# Release and security audit remediation

## Goal

- User-visible outcome: Patchwright release verification is read-only, candidate bytes are bound to an exact clean Git state and source archive, notarization logs fail closed, live-smoke evidence is created safely, promotion evidence is reviewer/digest bound, CI scans history and Rust advisories, and service smoke shutdown is exact and bounded.
- How to see it working: every packaging contract passes, targeted tamper cases fail for the intended reason, shell/Python syntax checks pass, and only the assigned release/docs paths are committed with DCO sign-offs.

## Current State

- Relevant paths: `script/`, `Tests/PackagingTests/`, `README.md`, `docs/`, and `.github/`.
- Existing behavior: `verify_distribution.sh` overwrites the checksum sidecar; packaging checks Git state only before a long build; notarization accepts an `Accepted` submission without parsing its log; live-smoke evidence uses permissive path creation and a shell redirection; clean-machine gates contain booleans without reviewer or per-check evidence provenance; CI does not run the repository secret scanner or an advisory audit; `smoke.sh` does not wait for the exact engine PID to exit.
- Constraints: no Rust or Swift changes, no writes outside the isolated `audit-release` worktree, no live Apple/GitHub mutations, strict red-green contract tests, and DCO-signed commits.

## Target State

- Desired behavior: release and promotion evidence fail closed on symlink, digest, dirty-state, identity, reviewer, and tamper mismatches; scripts do not overwrite verification inputs; operator docs match the actual relay CLI and GitHub redelivery semantics.
- Non-goals: no network signing service, no live notarization, no GitHub release upload, and no changes to application/runtime source.

## Risks and Failure Modes

- Shell portability or macOS-specific tool assumptions could make contract fixtures misleading.
- Stronger promotion schemas could reject older evidence; the schema change must be explicit and tested.
- Evidence digest circularity could make manifests unverifiable; manifests must bind leaf evidence, while the promotion envelope separately binds the gate and candidate.
- CI advisory tooling can drift; use a pinned local tool/version and document that advisory data is fetched at CI runtime.

## Milestones

### M1. Read-only distribution and exact release source binding

- Goal: verify an explicit existing sidecar without mutation and reject source/tag/index/worktree/archive tampering.
- Files / systems: release/candidate scripts and packaging contracts.
- Changes: add no-follow checksum verification, final Git-state verification, source-archive digest provenance, and `dirty == false` enforcement.
- Verification: targeted release and candidate/promotion contract cases, first RED then GREEN.
- Expected result: every sidecar/source/repository tamper is explicitly rejected.

### M2. Notary and evidence-file hardening

- Goal: reject problematic accepted notary logs and publish live-smoke evidence without path races or overwrite.
- Files / systems: notarization and GitHub smoke scripts plus contracts.
- Changes: sanitized log summary parser; canonical owner-only evidence directory and exclusive atomic writer; move `fail` before use.
- Verification: offline JSON fixtures and filesystem adversarial tests, first RED then GREEN.
- Expected result: errors/default-policy warnings fail and evidence paths cannot be symlinked or overwritten.

### M3. Clean-machine and promotion provenance

- Goal: require independent reviewer identity and digest/path evidence for every documented clean-machine check.
- Files / systems: promotion verifier/generator, clean-machine docs, promotion contract.
- Changes: schema-v2 clean-machine checks, evidence manifest binding, promotion manifest output, expanded matrix tests.
- Verification: happy path plus reviewer/path/digest/tamper rejection cases.
- Expected result: promotion succeeds only with a complete reviewer-owned evidence set.

### M4. CI, runtime shutdown, and operator docs

- Goal: scan full Git history/advisories, stop the exact smoke process, and correct relay/redelivery instructions.
- Files / systems: CI, smoke scripts/contracts, README and docs.
- Changes: full-depth checkout, existing secret scanner, pinned local cargo advisory tool, bounded TERM-to-KILL helper with socket cleanup, accurate `serve --webhook-secret-file` examples and manual-redelivery truth.
- Verification: contract tests, shell syntax, Python compile, workflow/doc assertions.
- Expected result: CI and operator instructions enforce the same release/security boundaries as local scripts.

## Verification

- `Tests/PackagingTests/release_contract.sh`
- `Tests/PackagingTests/candidate_evidence_contract.sh`
- `Tests/PackagingTests/promotion_contract.sh`
- `Tests/PackagingTests/compliance_contract.sh`
- `Tests/PackagingTests/github_app_smoke_contract.sh`
- `bash -n script/*.sh Tests/PackagingTests/*.sh`
- `PYTHONDONTWRITEBYTECODE=1 python3 -m py_compile script/*.py`
- `git diff --check`
- `git status --short --branch`

## Decision Log

- 2026-07-17: Keep public provenance local and SHA-256 based; do not invent remote signing infrastructure.
- 2026-07-17: Bind the archived committed source plus final repository state instead of changing the compiler/build checkout in this bounded remediation; final HEAD/index/worktree/tag checks preserve current local release ergonomics while making the remaining build-time mutation window explicit in evidence.
- 2026-07-17: Treat all notarization warnings as release-blocking by default, with an explicit operator policy switch for accepted warnings.
- 2026-07-17: Root coordination narrowed this commit to the already-green M1–M3 work and required docs. CI history/advisory scanning and exact `smoke.sh` shutdown remain deferred rather than being rushed into the signed handoff.

## Progress Log

- 2026-07-17: Completed repository/contract inventory and confirmed a clean isolated branch.
- 2026-07-17: Completed M1–M3 with recorded RED signals and GREEN targeted contracts: release security, candidate evidence, and the 29-category promotion matrix.
- 2026-07-17: Corrected relay `serve`/secret-file instructions and GitHub's operator-driven redelivery truth.
- 2026-07-17: Deferred M4 CI and exact smoke-process shutdown changes by root direction; they remain explicit follow-up work.

## Rollback / Recovery

- If this fails: stop at the failing contract, preserve its exact signal, and revert only the coherent branch-local commit that introduced the regression.
- Safe fallback: retain the prior release flow as blocked rather than weakening digest, identity, notarization, or clean-machine checks.
