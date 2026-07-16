# Patchwright Direct Open-Source Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish Patchwright 0.1.0 as a fully open-source, Developer ID-signed, Apple-notarized, self-updating macOS technical beta through an immutable GitHub Release.

**Architecture:** Keep source verification, candidate packaging, and public promotion as separate fail-closed boundaries. The app integrates pinned Sparkle 2.9.2 for signed updates; release scripts assemble and sign all nested code, generate license/SBOM evidence, and produce a digest-bound candidate manifest; promotion consumes exact-digest clean-machine and integration evidence without rebuilding.

**Tech Stack:** Swift 6.2+/SwiftPM, SwiftUI/AppKit, Sparkle 2.9.2, Rust 1.85+/Cargo, Bash, Python 3 standard library, Developer ID Application, Hardened Runtime, `notarytool`, `stapler`, `spctl`, GitHub CLI and Releases.

## Global Constraints

- Distribution is direct Developer ID only; do not add StoreKit, App Sandbox, App Store Connect, or a privileged installer package.
- Patchwright 0.1.0 supports macOS 26.0+ on Apple silicon only.
- License the full repository as `MIT OR Apache-2.0`; never publish private keys, tokens, Keychain exports, certificate exports, or raw notary credentials.
- The publisher GitHub App private key is never bundled. Read-only `gh` sync remains available; mutations require a user-owned GitHub App.
- Sparkle uses `https://github.com/s1korrrr/patchwright/releases/latest/download/appcast.xml`, an embedded public Ed25519 key, HTTPS, pre-extraction verification, and signed feeds.
- Official artifacts must map to public tag `v0.1.0` and one commit. Promotion consumes the notarized candidate rather than rebuilding it.
- Preserve unrelated user files and never stage the pre-existing untracked monetization documents.
- Use test-first red-green-refactor for behavior changes and keep release actions fail-closed.

---

### Task 1: Establish the open-source repository contract

**Files:**
- Create: `LICENSE-MIT`
- Create: `LICENSE-APACHE`
- Create: `CONTRIBUTING.md`
- Create: `SECURITY.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `PRIVACY.md`
- Create: `SUPPORT.md`
- Modify: `Cargo.toml`
- Modify: `Packaging/Info.plist`
- Modify: `README.md`
- Test: `Tests/PackagingTests/release_contract.sh`

**Interfaces:**
- Produces: consistent `MIT OR Apache-2.0` repository metadata and public contribution/security/privacy boundaries consumed by release validation.

- [ ] **Step 1: Add failing release-contract assertions**

Require both license files, required public documents, Cargo `license = "MIT OR Apache-2.0"`, non-`All rights reserved` bundle copy, and README links. Run `Tests/PackagingTests/release_contract.sh`; expect failure on missing files and old metadata.

- [ ] **Step 2: Add the minimal legal and community files**

Use the canonical MIT and Apache-2.0 texts, a Developer Certificate of Origin contribution rule, private vulnerability-reporting guidance, Contributor Covenant 2.1, a local-first privacy disclosure, and best-effort beta support boundaries. Do not promise warranties, response times, or services.

- [ ] **Step 3: Align metadata and documentation**

Set Cargo to `MIT OR Apache-2.0`, update bundle copyright, and add build/download/license/security/privacy links to README.

- [ ] **Step 4: Verify and commit**

Run `Tests/PackagingTests/release_contract.sh`, `cargo metadata --locked --format-version 1`, and `git diff --check`. Expected: all pass. Commit only Task 1 paths.

### Task 2: Generate SBOM, third-party notices, and publication secret evidence

**Files:**
- Create: `script/generate_release_compliance.py`
- Create: `script/scan_publication_secrets.sh`
- Create: `Tests/PackagingTests/compliance_contract.sh`
- Modify: `script/build_release_components.sh`
- Modify: `script/verify_reproducibility_bundle.sh`
- Modify: `Packaging/THIRD_PARTY_NOTICES.md`
- Modify: `script/verify.sh`

**Interfaces:**
- Produces: `evidence/sbom.spdx.json`, `evidence/third-party-notices.md`, and `evidence/secret-scan.json` for a release root.
- Consumes: `cargo metadata --locked --format-version 1`, `swift package show-dependencies --format json`, and a Git repository path.

- [ ] **Step 1: Write failing compliance fixtures**

Create a temporary Cargo/Swift metadata fixture and assert SPDX 2.3 identity, package names/versions/licenses, deterministic ordering, dependency-derived notices, clean scan JSON, and rejection of token/private-key fixtures. Run `Tests/PackagingTests/compliance_contract.sh`; expect missing-command failure.

- [ ] **Step 2: Implement deterministic compliance generation**

Use Python 3 standard-library JSON parsing and hashing. Fail when a dependency lacks a declared license or the metadata is malformed. Include Patchwright, Rust packages, Swift packages, Sparkle, the app, engine, and relay as SPDX packages or files.

- [ ] **Step 3: Implement complete-history and artifact scanning**

Scan tracked files, all reachable Git blobs across `git rev-list --objects --all`, and the candidate release directory. Detect common GitHub, Apple/private-key, generic PEM, Codex/OpenAI, and webhook-secret patterns; record only redacted path/object identifiers and rule names. Exit nonzero on findings.

- [ ] **Step 4: Wire compliance artifacts into assembly**

Generate compliance evidence before final checksums; require nonempty valid JSON/Markdown and include their hashes in candidate metadata.

- [ ] **Step 5: Verify and commit**

Run the compliance contract, release contract, and secret scan against the current repository. Inspect every candidate finding rather than suppressing broadly. Commit only Task 2 paths.

### Task 3: Integrate signed Sparkle updates

**Files:**
- Create: `Sources/PatchwrightApp/Services/UpdateController.swift`
- Create: `Tests/PatchwrightCoreTests/UpdateConfigurationTests.swift`
- Modify: `Package.swift`
- Create/Modify: `Package.resolved`
- Modify: `Sources/PatchwrightApp/App/PatchwrightApp.swift`
- Modify: `Sources/PatchwrightApp/Support/AppCommands.swift`
- Modify: `Packaging/Info.plist`
- Modify: `Tests/PackagingTests/release_contract.sh`

**Interfaces:**
- Produces: `@MainActor UpdateController.checkForUpdates()` backed by `SPUStandardUpdaterController` and a user-visible `Check for Updates...` command.
- Consumes: Sparkle product pinned exactly to `2.9.2` and a Keychain-generated Ed25519 public key.

- [ ] **Step 1: Add failing update-configuration tests**

Assert exact dependency pin, feed URL, 32-byte base64 public key, `SUVerifyUpdateBeforeExtraction=true`, `SURequireSignedFeed=true`, and command visibility. Run focused Swift and packaging tests; expect failure because Sparkle and keys are absent.

- [ ] **Step 2: Resolve Sparkle and create the Keychain update key**

Fetch Sparkle 2.9.2 from its official repository. Run its `generate_keys` tool once so the private key stays in login Keychain; capture only the printed public key. Never export the private key into the workspace.

- [ ] **Step 3: Implement the updater lifecycle**

Own one `SPUStandardUpdaterController` for the app lifetime, expose a main-actor update action, and inject it into app commands without duplicating controllers or starting updates in tests.

- [ ] **Step 4: Configure the signed feed**

Add exact feed/public-key and signed-feed keys to the packaged Info.plist. Preserve monotonically increasing `CFBundleVersion` behavior.

- [ ] **Step 5: Verify and commit**

Run focused Swift tests, `swift build -c release -Xswiftc -warnings-as-errors`, and the release contract. Expected: all pass with Sparkle resolved at 2.9.2.

### Task 4: Package and sign Sparkle nested code safely

**Files:**
- Modify: `script/build_release_components.sh`
- Modify: `script/sign_release.sh`
- Modify: `script/validate_bundle.sh`
- Modify: `script/verify_signing.sh`
- Modify: `Tests/PackagingTests/release_contract.sh`

**Interfaces:**
- Produces: a complete `Contents/Frameworks/Sparkle.framework` bundle with all nested code signed inside-out by the same Team ID.

- [ ] **Step 1: Add failing nested-bundle fixtures**

Assert the framework is copied, missing nested helpers fail validation, escaping symlinks fail, declared internal framework symlinks pass, signing order is deepest-first, and every nested code object has Developer ID/Hardened Runtime/timestamp/Team ID verification.

- [ ] **Step 2: Copy the resolved Sparkle framework**

Discover the framework from SwiftPM build metadata rather than a user-specific absolute path. Copy it with `ditto` into `Contents/Frameworks` and preserve required internal structure.

- [ ] **Step 3: Sign nested code inside-out**

Explicitly enumerate Sparkle XPC services, applications, frameworks, dylibs, and executable helpers in depth order; sign each before the Patchwright engine, relay, and app. Do not use `codesign --deep` for signing.

- [ ] **Step 4: Tighten validation**

Allow only nonescaping symlinks within the declared Sparkle framework. Verify every nested signature, secure timestamp where applicable, Team ID, Hardened Runtime, and absence of unreviewed entitlements.

- [ ] **Step 5: Verify and commit**

Run fixture contracts, build an unsigned bundle, inspect framework layout, and run `codesign --verify` against a local signed fixture when available.

### Task 5: Add release-quality app identity and safe GitHub setup copy

**Files:**
- Create: `Assets/PatchwrightIcon-source.png`
- Create: `Packaging/Patchwright.icns`
- Modify: `Packaging/Info.plist`
- Modify: `Sources/PatchwrightApp/Views/SettingsView.swift`
- Modify: `README.md`
- Modify: `Tests/PackagingTests/release_contract.sh`
- Modify: `Tests/PatchwrightCoreTests/ModelsTests.swift` or a new focused copy test

**Interfaces:**
- Produces: packaged `Patchwright.icns`, explicit BYO GitHub App language, and separate read-only/Codex/mutation prerequisites.

- [ ] **Step 1: Add failing icon and copy assertions**

Require `CFBundleIconFile`, packaged `.icns`, nonempty icon representations, and copy that says the GitHub App/private key is user-owned and required only for mutations.

- [ ] **Step 2: Produce the icon source and `.icns`**

Create one original Patchwright identity optimized for Apple app-icon masks, retain a 1024x1024 editable source, generate the full iconset with `sips`, and compile with `iconutil`.

- [ ] **Step 3: Revise setup copy**

Keep read-only `gh` sync useful, make publisher-key absence explicit, and fail closed without suggesting broad or shared credentials.

- [ ] **Step 4: Verify and commit**

Run icon inspection, focused tests, release contract, and a local app launch to confirm the icon and settings render.

### Task 6: Split notarized packaging from digest-bound promotion

**Files:**
- Create: `script/package_release.sh`
- Create: `script/promote_release.sh`
- Create: `script/verify_release_evidence.py`
- Modify: `script/release.sh`
- Modify: `script/release_readiness.sh`
- Modify: `script/generate_release_metadata.sh`
- Modify: `script/notarize_release.sh`
- Modify: `Tests/PackagingTests/release_contract.sh`
- Create: `Tests/PackagingTests/promotion_contract.sh`

**Interfaces:**
- Produces: `evidence/notarized-candidate.json` and promotion readiness JSON bound to `artifact_sha256`, `git_commit`, `tag`, `version`, and `build`.
- Consumes: regular JSON evidence documents for repository, Codex, GitHub, clean-machine, secret-scan, and compliance gates, each naming the same artifact digest.

- [ ] **Step 1: Add failing package/promotion matrix tests**

Cover missing evidence, malformed JSON, digest mismatch, commit/tag mismatch, stale files, symlinks, nonregular files, unsigned candidate, notary rejection, Gatekeeper rejection, and a complete fixture promotion.

- [ ] **Step 2: Implement candidate packaging**

Move current build/sign/notarize/DMG/appcast/compliance logic into `package_release.sh`. Always print candidate and release-root paths even when later promotion gates remain pending.

- [ ] **Step 3: Generate and sign the appcast**

Use Sparkle's pinned `generate_appcast` tool with its Keychain key, point the enclosure at the immutable versioned GitHub asset URL, require the Ed25519 signature and signed feed, and include `appcast.xml` in candidate checksums.

- [ ] **Step 4: Implement evidence validation and promotion preparation**

Validate exact digest and commit bindings without accepting environment booleans. Produce a machine-readable release-asset manifest ready for GitHub upload but do not perform a public write in unit tests.

- [ ] **Step 5: Keep a compatibility wrapper**

Make `release.sh` run package mode and report `notarized candidate` with the exact follow-up promotion command; never label it release-ready before external evidence.

- [ ] **Step 6: Verify and commit**

Run the full fixture matrix, shell syntax checks, and release contracts.

### Task 7: Add credential-free CI and public release documentation

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/release.yml`
- Create: `docs/direct-download.md`
- Modify: `docs/release-checklist.md`
- Modify: `docs/release-readiness.md`
- Modify: `docs/production-plan.md`
- Modify: `README.md`
- Test: `Tests/PackagingTests/release_contract.sh`

**Interfaces:**
- Produces: fork-safe source CI and a public download/verification contract. Signing and notarization remain local protected release actions.

- [ ] **Step 1: Add failing workflow/docs checks**

Require pinned third-party actions, read-only default permissions, no pull-request secrets, exact source checks, direct-download platform disclosure, checksum verification, and no Gatekeeper-bypass commands.

- [ ] **Step 2: Add CI**

Use the current official GitHub macOS runner/Xcode combination that supports Swift 6.2/macOS 26, pin action revisions, cache only dependency/build inputs without credentials, and run `./script/verify.sh` plus smoke where supported.

- [ ] **Step 3: Rewrite direct-release documentation**

Remove App Store Connect readiness vocabulary for this version. Document DMG verification, drag installation, BYO dependencies, update behavior, source build, official signature verification, support scope, and uninstall/data retention.

- [ ] **Step 4: Verify and commit**

Run workflow syntax validation, release contracts, link checks, and `git diff --check`.

### Task 8: Whole-branch verification and security/publication audit

**Files:**
- Modify only when verification exposes an in-scope defect.
- Create locally ignored evidence under `.release-audit/` or the candidate release root.

**Interfaces:**
- Produces: fresh source, security, license, runtime, and package evidence for the exact release commit.

- [ ] **Step 1: Run full source verification**

Run `./script/verify.sh`, `./script/smoke.sh`, `./script/smoke_codex.sh`, strict Swift Release build, locked Rust tests, formatter/lint checks, and the real app launch smoke.

- [ ] **Step 2: Audit the whole branch**

Review the full merge-base diff for scope, secrets, dependency changes, update security, path traversal/symlink handling, release command injection, evidence spoofing, permissions, and accidental publication of user data.

- [ ] **Step 3: Scan complete history and candidate inputs**

Run the repository scanner across every reachable object and local ref. Independently inspect GitHub's current secret-scanning state. Rotate any exposed credential before proceeding.

- [ ] **Step 4: Obtain a clean review**

Use an independent whole-branch reviewer. Fix Critical/Important findings, rerun covering tests, and repeat review until clean.

### Task 9: Package, notarize, and validate the exact 0.1.0 candidate

**Files:**
- Release outputs only under `~/.patchwright/release-work`.
- Evidence updates under `docs/audits/` only after redaction.

**Interfaces:**
- Produces: final `Patchwright-0.1.0.dmg`, checksum, signed appcast, SBOM, notices, candidate manifest, and notarization evidence.

- [ ] **Step 1: Prepare the release commit and tag**

Merge the reviewed branch, verify clean `main`, create annotated `v0.1.0`, and verify the tag resolves to the release commit.

- [ ] **Step 2: Verify protected credentials without exposing them**

Require exactly one selected Developer ID Application identity, a working named `notarytool` Keychain profile, and the Sparkle Keychain key. Print only identity class/team and profile/key availability.

- [ ] **Step 3: Run package mode**

Run `PATCHWRIGHT_VERSION=0.1.0 PATCHWRIGHT_BUILD=1 ./script/package_release.sh`. Require accepted/stapled app and DMG, passing Gatekeeper, valid appcast signature, and complete candidate evidence.

- [ ] **Step 4: Validate from a disposable clean machine**

Run the documented exact-DMG probe on a pristine Apple-silicon macOS 26+ VM and complete the missing-dependency, Codex, BYO GitHub App, lifecycle, update, offline/revocation, migration, relaunch, uninstall, and data-retention matrix. Bind the resulting JSON to the DMG digest.

- [ ] **Step 5: Final promotion preflight**

Run `promote_release.sh` in verify-only mode with exact digest-bound repository, Codex, GitHub, clean-machine, compliance, and secret evidence.

### Task 10: Publish the source and immutable GitHub Release

**Files/Systems:**
- GitHub repository `s1korrrr/patchwright`
- Tag `v0.1.0`
- Release assets from the verified candidate manifest

**Interfaces:**
- Produces: public source repository and downloadable immutable 0.1.0 release.

- [ ] **Step 1: Push reviewed source and tag**

Push the reviewed main commit and `v0.1.0`. Confirm the remote commit and tag match the candidate manifest.

- [ ] **Step 2: Create a complete draft release**

Upload the DMG, checksum, appcast, SBOM, notices, and redacted evidence from the manifest. Verify asset sizes and downloaded hashes before publishing.

- [ ] **Step 3: Perform the final visibility preflight**

Re-run complete-history secret scanning against remote refs, inspect repository rules/settings, verify no private issues/discussions/actions artifacts would be unintentionally exposed, and confirm public README/support/privacy content.

- [ ] **Step 4: Make the repository public and publish atomically**

Change visibility with GitHub's explicit consequence acknowledgement, publish the complete release, enable release immutability for future releases, and verify anonymous source and asset access.

- [ ] **Step 5: Verify the public install/update path**

Download the public DMG without authentication, verify checksum/signature/notarization/Gatekeeper, install and launch, fetch the public signed appcast, and confirm `Check for Updates...` reaches the expected feed.

- [ ] **Step 6: Record final truth**

Update the audit with public URLs, tag/commit/digest, notarization status, clean-machine evidence, remaining external limitations, and rollback/incident procedure. Do not claim support beyond the documented beta boundary.

