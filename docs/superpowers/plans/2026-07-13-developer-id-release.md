# Developer ID Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce a reproducible, credential-free Patchwright release bundle and close Developer ID signing, notarization, Gatekeeper, install/update, and clean-machine validation without overstating external readiness.

**Architecture:** Shell scripts assemble immutable Release outputs outside File Provider storage, validate metadata/entitlements, sign nested helpers inside-out, create a DMG, submit/staple through a named Keychain notary profile, and emit machine-readable evidence. All credentials stay in Keychain; unsigned/ad-hoc and Developer ID paths are explicit and cannot be confused.

**Tech Stack:** SwiftPM, Cargo, `/usr/bin/codesign`, `/usr/bin/security`, `xcrun notarytool`, `xcrun stapler`, `spctl`, `hdiutil`, shellcheck-compatible zsh/bash, SHA-256 tooling.

## Global Constraints

- Depend on completed source/integration plans; packaging never substitutes for behavioral verification.
- Direct distribution outside the Mac App Store; macOS 26+ Apple silicon; no App Sandbox if it breaks approved orchestration.
- Release identity must be `Developer ID Application`. Apple Development, Apple Distribution, or ad-hoc signatures cannot produce a notarized-candidate label.
- No Hardened Runtime exception entitlement without a proven failing behavior and documented review.
- Never place certificate exports, passwords, App Store Connect keys, notary credentials, GitHub keys/tokens, or user state in the repository or reproducibility bundle.
- Existing `dist` artifacts are replaced only after the new candidate passes its current gate; use `trash`, not permanent deletion.

---

## Task 1: Define bundle metadata and minimal entitlements

**Files:**
- Add: `Packaging/Info.plist`
- Add: `Packaging/Patchwright.entitlements`
- Add: `Packaging/patchwright-engine.entitlements`
- Add: `Packaging/patchwright-relay.entitlements`
- Add: `script/validate_bundle.sh`
- Add: `Tests/PackagingTests/validate_bundle.bats`
- Modify: `Package.swift`

- [ ] Add fixture tests rejecting missing/mismatched bundle ID/version/build/minimum system, writable/executable anomalies, symlinks escaping bundle, forbidden quarantine/FinderInfo/File Provider xattrs, unsigned helpers, resource-envelope drift, and unreviewed entitlement keys.
- [ ] Observe RED.
- [ ] Move generated Info.plist values into a versioned template with `ai.patchwright.app`, macOS 26 minimum, semantic marketing version, monotonic build, copyright, and document/privacy declarations actually used.
- [ ] Define minimal app/helper entitlements and a checked allowlist; enable Hardened Runtime at signing time, not through an exception blanket.
- [ ] Implement structural validation before and after signing, including nested helper architecture and executable paths.
- [ ] Run fixture matrix and commit: `Define Patchwright release bundle metadata`.

## Task 2: Build immutable Release components and reproducibility metadata

**Files:**
- Add: `script/build_release_components.sh`
- Add: `script/generate_release_metadata.sh`
- Add: `script/verify_reproducibility_bundle.sh`
- Modify: `script/build_and_run.sh`
- Add: `Packaging/THIRD_PARTY_NOTICES.md`
- Modify: `docs/release-checklist.md`

- [ ] Add shell tests proving clean output root, Release configuration, warnings-as-errors, pinned Cargo lock use, arm64 architectures, helper placement, deterministic manifest ordering, credential exclusion, and failure before replacing a prior candidate.
- [ ] Observe RED.
- [ ] Build Swift and Rust Release outputs into a fresh temporary root outside synced/File Provider locations; copy only declared artifacts.
- [ ] Emit version/build/git commit/dirty-state/toolchain/dependency/license metadata and SHA-256 manifests. Dirty release builds are rejected unless an explicit local-debug mode labels them non-candidate.
- [ ] Package source/lockfiles/scripts/manifests needed to reproduce without including `.git`, databases, logs, worktrees, Keychain exports, environment dumps, or credentials.
- [ ] Keep `build_and_run.sh` as an ad-hoc developer path and label it clearly; route distribution through new scripts.
- [ ] Run release-component tests and commit: `Build reproducible Patchwright release components`.

## Task 3: Sign nested code inside-out with Developer ID

**Files:**
- Add: `script/sign_release.sh`
- Add: `script/verify_signing.sh`
- Add: `Tests/PackagingTests/signing.bats`
- Modify: `docs/release-checklist.md`

- [ ] Add tests that reject missing identity, ambiguous identities, wrong identity class, absent secure timestamp, missing Hardened Runtime, helper/app Team ID mismatch, forbidden entitlements, ad-hoc residue, and altered post-sign content.
- [ ] Observe RED using the current machine's absence of `Developer ID Application` as the expected external-blocker case.
- [ ] Resolve exactly one identity from `PATCHWRIGHT_DEVELOPER_ID` or an exact `Developer ID Application:` match; never fall back to Apple Development/Distribution/ad-hoc.
- [ ] Strip xattrs, sign `patchwright-engine` and `patchwright-relay` with their entitlements, then app executable/bundle with secure timestamp and runtime options.
- [ ] Verify `codesign --verify --deep --strict --verbose=4`, designated requirement, Team ID, entitlements, runtime flags, timestamp, nested identities, and `spctl --assess --type execute` with truthful pre-notarization handling.
- [ ] Commit scripts/tests even when real signing remains `blocked:external`; record that no Developer ID candidate exists yet.
- [ ] Commit: `Automate Developer ID signing verification`.

## Task 4: Create, sign, notarize, staple, and verify the DMG

**Files:**
- Add: `script/create_dmg.sh`
- Add: `script/notarize_release.sh`
- Add: `script/verify_distribution.sh`
- Add: `Tests/PackagingTests/distribution.bats`
- Modify: `docs/release-checklist.md`

- [ ] Add tests for DMG layout, Applications alias, volume name/version, read-only conversion, checksum, signed container, missing/wrong notary profile, rejected/in-progress/accepted submissions, log retention, stapling failure, offline ticket validation, and Gatekeeper rejection.
- [ ] Observe RED.
- [ ] Create the DMG from the already verified signed app, sign the DMG with the same Developer ID identity, and generate checksum metadata only after final stapling.
- [ ] Accept only a named Keychain profile via `PATCHWRIGHT_NOTARY_PROFILE`; never accept raw Apple credentials in arguments/environment files.
- [ ] Submit with `xcrun notarytool submit "$DMG_PATH" --keychain-profile "$PATCHWRIGHT_NOTARY_PROFILE" --wait --output-format json`, retain JSON and notarization log, require `Accepted`, staple app and DMG, then validate both tickets.
- [ ] Verify final mounted payload, signature, Gatekeeper source, checksum, metadata manifest, and absence of credentials/quarantine.
- [ ] Commit: `Automate notarized DMG distribution`.

## Task 5: Add a truthful release driver and readiness report

**Files:**
- Add: `script/release.sh`
- Add: `script/release_readiness.sh`
- Add: `docs/release-readiness.md`
- Modify: `script/verify.sh`
- Modify: `README.md`

- [ ] Add matrix tests for local unsigned validation, ad-hoc developer app, package-ready Developer ID app, notary blocked, notarized candidate, clean-machine blocked, and release-candidate ready.
- [ ] Observe RED.
- [ ] Compose gates without weakening them: verify source → build components → validate → Developer ID sign → verify → DMG → notarize → staple → final verify → report.
- [ ] Emit machine-readable JSON and concise Markdown with independent booleans/evidence paths for repo, integration, package, Developer ID, Hardened Runtime, notarization, Gatekeeper, clean-machine, and external prerequisites.
- [ ] Ensure a blocked external gate exits distinctly and never leaves a misleading final artifact named as a release candidate.
- [ ] Document owner steps for obtaining the Developer ID Application certificate and creating a Keychain notary profile, without collecting credentials.
- [ ] Commit: `Report truthful Patchwright release readiness`.

## Task 6: Validate install, first launch, relaunch, update, and uninstall in a clean environment

**Files:**
- Add: `script/clean_machine_probe.sh`
- Add: `docs/clean-machine-test-plan.md`
- Add: `docs/audits/2026-07-13-clean-machine-validation.md`

- [ ] Define a macOS 26+ clean-machine/VM image with no source checkout, developer toolchain, Patchwright state, `gh`, cached GitHub token, Codex, or engine process. Record OS/build/hardware/VM identity.
- [ ] Add a signed probe that checks DMG checksum/mount, drag-install, Gatekeeper launch, bundled helper health, engine socket permissions, missing-Codex recovery, Codex install/sign-in/app-server connection, GitHub App discovery without `gh`, disposable ingestion, relaunch queue/task/thread recovery, offline/expired/revoked/missing-permission states, and clean quit.
- [ ] Install the prior released schema fixture, ingest data, upgrade in place, and prove task/queue/thread/database preservation and migration idempotency.
- [ ] Verify documented uninstall removes app separately from retained local data and that optional data deletion is explicit and scoped.
- [ ] Capture screenshots/logs with private repository content and credentials redacted; retain exact failures and rollback.
- [ ] Do not mark this task complete until the final notarized DMG, not a source-built app, passes.
- [ ] Commit the plan/probe/evidence only after review: `Validate Patchwright on a clean Mac`.

## Task 7: Final cross-system E2E audit

**Files:**
- Add: `docs/audits/2026-07-13-patchwright-final-e2e.md`
- Modify: `docs/release-checklist.md`

- [ ] From final source, run strict Rust/Swift tests, migration/restart/cancellation fault matrix, native UI interaction/accessibility/resize/theme/log audit, real disposable Codex smoke, and the separately authorized disposable GitHub App delivery/merge workflow.
- [ ] Build the immutable candidate, run signature/Hardened Runtime/entitlement/secret/license/checksum/reproducibility audits, notarize/staple, and run clean-machine install/update workflow.
- [ ] Review final branch diff and draft PR for scope, security, unresolved comments, CI, and accidental credentials/artifacts.
- [ ] Record each gate independently: `repo-ready`, `integration-ready`, `package-ready`, `notarized candidate`, `release-candidate ready`, or exact `blocked:external`.
- [ ] A current expected blocker is the missing `Developer ID Application` identity and notary profile; keep it explicit until live evidence changes.
- [ ] Commit: `Audit Patchwright release end to end`.
