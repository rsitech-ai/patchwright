# Patchwright open-source release readiness

## Goal

- User-visible outcome: prepare Patchwright's current source tree for a professional public GitHub release, remediate safe repository-side blockers, and finish with an evidence-backed publication decision.
- How to see it working: a clean clone can follow the documented build path, required checks and release rehearsal pass, security and provenance evidence is recorded, public community files are usable, and remaining owner/external gates are stated exactly.

## Current State

- Relevant paths: Swift package in `Package.swift`, Rust workspace in `Cargo.toml`, runtime crates in `crates/`, Swift sources in `Sources/`, tests in `Tests/` and crate test directories, release tooling in `script/`, packaging inputs in `Packaging/`, public documentation in the repository root and `docs/`, and GitHub configuration in `.github/`.
- Existing behavior: Patchwright is a macOS 26+ local-first GitHub engineering control plane with a SwiftUI app, Rust engine, Rust relay, direct-download packaging, GitHub Actions, tags `v0.1.0` and `v0.1.1`, and an existing dual `MIT OR Apache-2.0` license choice.
- Baseline: `main` at `8e9a08111ed721d80a7059a89bf9326ee3b0540d`, tracking `origin/main`; release work is isolated on `chore/oss-release-readiness`.
- Constraints: preserve three pre-existing untracked monetization-document paths; do not publish a GitHub Release, change visibility/settings/profiles, choose new legal identity, rewrite history, or expose secrets. On 2026-07-20 the user explicitly authorized merging this release-readiness work to `main`, pushing `main`, and cleaning up task-owned temporary artifacts and the merged branch.

## Target State

- Desired behavior: public-source blockers are removed or fail-closed; build, lint, test, package, security, documentation, and clean-clone evidence is reproducible; community and release configuration match actual support boundaries; the final report distinguishes repository readiness from GitHub, signing, notarization, and owner approval gates.
- Non-goals: no repository settings/profile mutation, no profile pinning, no binary/package publication, no license or copyright change, no history rewrite, and no unrelated product redesign.

## Risks and Failure Modes

- Existing untracked user work could be accidentally staged or committed.
- Release claims could exceed current signing, notarization, GitHub settings, or clean-machine evidence.
- A broad cleanup could break exact approval, credential, or release-evidence contracts.
- Security scanners can produce false positives or leak secret material if raw output is handled carelessly.
- Public CI can become unsafe if fork-controlled inputs reach privileged tokens or publishing jobs.
- The local 6.2 GB checkout includes ignored build caches and a release worktree; these must not be mistaken for tracked release contents.

## Milestones

### M1. Baseline and public-surface inventory

- Goal: establish the exact Git, toolchain, tracked-file, build, test, documentation, release, and GitHub baseline before edits.
- Files / systems: repository tree, Git metadata, manifests, workflows, scripts, docs, local GitHub metadata where read-only authenticated access is available.
- Changes: record evidence only; update this plan as facts replace assumptions.
- Verification: `git status --short --branch`, `git ls-files`, tool-version checks, repository-provided verification commands, and read-only GitHub inspection.
- Expected result: pre-existing failures and external limitations are separated from regressions.

### M2. Privacy, history, and provenance closure

- Goal: scan the checkout and reachable history for publication secrets, inventory third-party rights, and close dependency/provenance blockers.
- Files / systems: all tracked source/config/docs/assets plus reachable Git history.
- Changes: apply focused, tested runtime and release-control hardening; do not rewrite history or make legal decisions.
- Verification: repository secret-scan scripts, dependency advisories, license inventory, focused regression tests, and redacted history checks. The user explicitly removed the Codex Security scan from scope on 2026-07-20.
- Expected result: the publication corpus is secret-clean, locked dependencies have no known advisory blocker, focused hardening tests pass, and any historical or IP blocker is explicit.

### M3. Correctness, hygiene, and public CI

- Goal: fix validated release blockers and tighten repository hygiene, CI permissions, test coverage, and deterministic release behavior without scope creep.
- Files / systems: implementation, tests, `.gitignore`, `.gitattributes`/`.editorconfig` where justified, `.github/`, package metadata, and release scripts.
- Changes: small reviewable edits grouped by cause, with tests first for behavior fixes.
- Verification: Swift and Rust formatting, linting, unit/integration tests, build, workflow-policy checks, `git diff --check`, and targeted regression smokes.
- Expected result: required checks pass locally or have a precise accepted environmental blocker.

### M4. Documentation, community, and release package

- Goal: make installation, safety boundaries, support, contribution, security reporting, release steps, and GitHub intake accurate and executable.
- Files / systems: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `docs/`, `.github/`, packaging metadata and notices.
- Changes: correct unsupported claims and stale commands; add only community/release files that serve a real workflow.
- Verification: execute every documented command where practical, validate links and metadata, inspect package contents, and compare release claims to evidence.
- Expected result: a new public contributor can build and verify without private dependencies or unstated credentials.

### M5. Fresh-clone rehearsal and final gate

- Goal: prove the prepared tree from a fresh temporary clone and produce the required GO/NO-GO report plus exact approval request.
- Files / systems: clean temporary clone, built artifacts, security bundle, release dossier, Git status and commit set.
- Changes: finalize evidence, logical local commits containing only intentional release work, and no remote writes.
- Verification: clean-clone setup/build/tests/quickstart/package/install smoke, secret rescan, documentation/link checks, package inspection, and final `git diff --check`/status review.
- Expected result: `READY AFTER LISTED APPROVALS` only if repository-side evidence passes; otherwise `NOT READY FOR PUBLICATION` with blocking next actions.

## Verification

- `./script/verify.sh`
- `./script/smoke.sh`
- `./script/build_and_run.sh --verify`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features`
- `swift test`
- `./script/scan_publication_secrets.sh`
- Release/package commands discovered from `script/release_readiness.sh`, `script/package_release.sh`, and release documentation.
- Fresh-clone manual smoke from a temporary path using only documented prerequisites.

## Decision Log

- 2026-07-20: Use the user-specified `chore/oss-release-readiness` branch even though the general HQ default is `feat/andrzej_*`; the task brief explicitly names this branch.
- 2026-07-20: Treat the existing dual MIT/Apache-2.0 files and manifest declaration as an already-selected license, not authority to change license or copyright ownership.
- 2026-07-20: Preserve and exclude the pre-existing untracked `docs/monetization/`, `docs/reflections/2026-07-15-patchwright-monetization-assessment.md`, and `docs/superpowers/plans/2026-07-15-patchwright-monetization-assessment.md` paths.
- 2026-07-20: The user explicitly canceled the Codex Security scan deliverable after the code hardening and repository verification had completed; temporary scan artifacts are excluded from the release output and removed during cleanup.
- 2026-07-20: The user explicitly authorized a local merge to `main`, push of `main`, and cleanup of task-owned temporary artifacts and the merged branch. Repository settings, profile changes, and binary release publication remain separate actions.

## Progress Log

- 2026-07-20: Completed session bootstrap, task/authority parsing, Git/manifest/community-file inventory, branch isolation, repository/publication hardening, community and release documentation, and focused regression coverage.
- 2026-07-20: `./script/verify.sh`, `./script/smoke.sh`, `./script/build_and_run.sh --verify`, `cargo fmt --all -- --check`, refreshed `cargo audit --deny warnings`, local-link validation, and a clean temporary-copy `./script/verify.sh` all passed. The only intentionally ignored test requires a signed-in Codex installation and may consume model quota.
- 2026-07-20: Final step: stage only task-owned files, rescan the exact staged publication set, commit, merge to `main`, verify the merged tree, push `main`, and clean task-owned temporary artifacts without touching the three preserved user paths.

## Rollback / Recovery

- If this fails: stop before any external or destructive action, record the exact failing command and affected file, and leave user-owned untracked paths untouched.
- Safe fallback: revert only task-owned hunks with a reviewed patch or commit-level inverse after confirmation; never reset, clean, stash, or overwrite unrelated work.
