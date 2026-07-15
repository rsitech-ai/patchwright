# Patchwright Direct Open-Source Release Design

## Outcome

Patchwright 0.1.0 ships outside the Mac App Store as a free technical beta. The official Apple-silicon macOS 26+ build is assembled from the public `v0.1.0` source tag, signed with Developer ID Application, notarized and stapled by Apple, distributed in an immutable GitHub Release, and updateable through Sparkle 2.

The complete Patchwright source needed to build the desktop app, engine, relay, packaging scripts, and release metadata is public under `MIT OR Apache-2.0`. Publisher credentials, private signing keys, notary credentials, update-signing keys, and GitHub App private keys are operational secrets and are never published.

## Product and Distribution Boundary

- The release lane is direct Developer ID distribution. Mac App Store, App Sandbox, App Store Connect, and StoreKit are out of scope for this version.
- The official artifact is `Patchwright-0.1.0.dmg`. A privileged installer package is not added.
- The supported matrix is macOS 26.0 or newer on Apple silicon. Intel and older macOS releases are explicit non-goals for 0.1.0.
- The application remains local-first. It stores state under `~/.patchwright`, launches the bundled engine, and discovers a separately installed Codex CLI.
- Read-only GitHub sync may use the user's existing `gh` authentication. GitHub mutations remain disabled until the user configures a bring-your-own GitHub App. The publisher's GitHub App private key is never distributed.
- The repository and binary release become public only after complete-history secret scanning, source verification, notarized-artifact verification, and exact-digest promotion gates pass.

## Open-Source Contract

- Add `LICENSE-MIT` and `LICENSE-APACHE` and declare `MIT OR Apache-2.0` consistently in Cargo and public documentation.
- Add contribution, security-reporting, code-of-conduct, privacy, support, and build-from-source documentation.
- Replace misleading `All rights reserved` bundle copy with clear copyright and license wording.
- Generate a machine-readable SPDX 2.3 SBOM and a dependency-derived third-party notice for every release candidate. The generated documents cover Rust, Swift, bundled binaries, and Sparkle.
- Scan the complete Git object graph, all local refs, and the release directory for credential-shaped content. Publication fails closed on a finding.
- Public documentation states that Patchwright is open source while macOS, Apple Foundation Models, GitHub, and Codex are external platforms or services with their own terms.

## Update Architecture

- Pin Sparkle 2.9.2 through Swift Package Manager and link the `Sparkle` product into `PatchwrightApp`.
- Add a single updater controller owned by the application lifecycle and a `Check for Updates...` command.
- Configure `SUFeedURL` as `https://github.com/s1korrrr/patchwright/releases/latest/download/appcast.xml`.
- Embed only the Sparkle Ed25519 public key in `Info.plist`. Keep the private key in the release operator's protected Keychain and never accept it through command-line arguments, environment files, repository files, or release assets.
- Require HTTPS, Ed25519-signed update archives, pre-extraction verification, and a signed feed.
- Sign and verify all Sparkle frameworks, XPC services, applications, and helper executables inside-out before signing Patchwright.app.
- Allow only the known Sparkle framework symlinks required by the pinned dependency; continue rejecting escaping or undeclared bundle symlinks.

## GitHub Credential Model

- The technical beta keeps the existing bring-your-own GitHub App model for mutation workflows.
- Setup copy must say that the user creates and owns the GitHub App and private key.
- Read-only sync remains usable without a GitHub App when `gh` authentication is available.
- The app must never imply that a publisher credential is bundled, and mutation controls remain fail-closed until repository access is verified.
- A hosted official relay, OAuth/device authorization, team administration, billing, and managed execution are future service work and are not part of 0.1.0.

## Release Architecture

The release workflow is split into two digest-bound phases.

### Package

1. Require a clean, tagged source commit.
2. Run source tests, strict builds, packaging contracts, secret checks, and dependency/license checks.
3. Assemble the app outside File Provider storage.
4. Sign all nested code and the app with one Developer ID Application identity, Hardened Runtime, and secure timestamps.
5. Create and sign the DMG.
6. Submit the app and DMG with a named `notarytool` Keychain profile, retain the notarization results, and staple both artifacts.
7. Verify code signatures, Team ID consistency, entitlements, notarization tickets, mounted DMG layout, Gatekeeper, appcast signature, SBOM, notices, and checksums.
8. Emit a notarized-candidate manifest containing the source commit, tag, version, build number, artifact path, artifact SHA-256, and evidence paths.

### Promote

1. Consume the notarized-candidate manifest rather than rebuilding.
2. Require clean-machine and integration evidence bound to the candidate DMG SHA-256.
3. Verify that the public tag resolves to the candidate source commit.
4. Create a draft GitHub Release and attach the DMG, checksum, appcast, SBOM, third-party notices, and redacted release evidence.
5. Publish the complete release atomically and enable repository release immutability.
6. Change repository visibility to public only after the publication preflight remains green.

Environment booleans are not sufficient evidence. Every external proof consumed during promotion must name the candidate digest and be represented by a regular JSON evidence file.

## Release Security

- Pull-request and fork CI never receives Developer ID, notary, GitHub App, or Sparkle private credentials.
- The first official release is produced on the trusted local release Mac. Later automation may use a protected, dedicated macOS runner, but importing a `.p12` into ordinary hosted CI is not part of this release.
- The release process uses a named Keychain notary profile and a Keychain-backed Sparkle key. Raw secrets are never accepted as script arguments.
- Complete-history scanning runs before changing repository visibility. Any historical secret requires revocation or rotation before publication; unresolved findings block publication.
- Release evidence may expose the public Team ID, bundle ID, notarization request ID, commit, version, and artifact hash. Local paths, usernames, repository content, tokens, keys, and private GitHub identifiers are redacted.

## User Experience

- Add a production-quality Patchwright app icon and include its editable source asset plus generated `.icns` in the repository.
- The first-launch and settings experience identifies missing Codex, missing `gh` authentication, and missing bring-your-own GitHub App configuration separately.
- The download documentation says macOS 26+ and Apple silicon before the download action, explains drag installation, and never asks users to bypass Gatekeeper.
- The About surface exposes version, build, source URL, licenses, privacy document, and update check.

## Testing and Evidence

- Every behavior change follows red-green-refactor with focused Swift or shell contract tests.
- `./script/verify.sh` remains the source gate and gains updater, license, SBOM, secret-scan, package-manifest, and promotion-contract coverage.
- A release build must pass `codesign`, `stapler`, `spctl`, mounted-DMG validation, and a launch smoke from `/Applications` or an equivalent clean install location.
- The exact notarized DMG must pass the disposable clean-machine matrix covering first launch, missing dependencies, Codex setup, GitHub setup, one disposable approval-gated lifecycle, relaunch, update, offline/revoked states, migration, and uninstall/data-retention behavior.
- Publication is blocked when any required evidence is absent, stale, references a different digest, or contains credential-shaped content.

## Publication and Commercial Position

- Patchwright 0.1.0 is free and has no StoreKit, local license lock, subscription, or paywall.
- Future revenue may come from a managed open-source relay, team controls, hosted execution, stable/LTS support, and deployment assistance. These future services do not narrow the 0.1.0 source license.
- The public release page identifies official signed binaries as maintainer builds and explains that community builds do not carry the maintainer's Developer ID signature.

## Acceptance Criteria

- The public repository contains complete buildable source, explicit dual licenses, contribution/security/privacy documentation, and no unresolved secret-scan findings.
- `Patchwright-0.1.0.dmg` is signed with Developer ID Application, Hardened Runtime, and secure timestamps; the app and DMG have valid notarization tickets and pass Gatekeeper.
- Sparkle can parse the published signed appcast and verifies the release archive with the embedded public key.
- The release assets are traceable to public tag `v0.1.0` and one commit through checksums, SBOM, build metadata, and immutable release attestation.
- Clean-machine and integration evidence is bound to the released DMG digest.
- The repository is public and the immutable GitHub Release is downloadable without Mac App Store access.
