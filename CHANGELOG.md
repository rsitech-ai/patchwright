# Changelog

All notable user-facing and distribution changes are documented here. Patchwright follows semantic versioning for public releases.

## [0.2.0] - 2026-07-20

### Added

- Add a reproducible community-prerelease packager that binds an ad-hoc-signed
  app archive and checksum to the exact clean Git commit and release tag.
- Publish machine-readable community release metadata that states the app
  architecture, minimum macOS version, signing class, and notarization status.

### Changed

- Move the canonical public repository and release URLs to the RSI Tech
  organization at `rsitech-ai/patchwright`.
- Adopt Apache-2.0 as the sole project license and record Rafal Sikora as the
  copyright owner, with RSI Tech as public maintainer.
- Require the live GitHub App qualification smoke to reject both the canonical
  organization repository and the legacy founder-account redirect.
- Separate community prerelease downloads from the unchanged Developer ID,
  notarization, clean-machine, and independent-promotion release contract.

### Hardened

- Require exact approved head and base identities immediately before supported
  pull-request mutations.
- Bound Codex protocol messages and request-wide resources, redact
  credential-shaped durable content, and bind completion to exact active
  request, thread, and turn identities.
- Apply one global resource budget to nested GitHub snapshot fan-out.

## [0.1.1] - 2026-07-18

### Fixed

- Preserve historical version-1 task contracts as validated, read-only audit evidence while keeping preparation, verification, delivery, and merge execution fail closed without complete integrity evidence.
- Remove preparation, delivery, and merge controls from completed task surfaces.
- Adapt the populated pull-request queue to constrained window widths without negative AppKit geometry faults.
- Clarify that local task preview and read-only GitHub data remain available without GitHub App mutation access.

### Security

- Reject malformed or partial task-contract integrity evidence consistently in the Rust engine and Swift client.

### Distribution

- Synchronize the macOS app, engine, relay, default package version, and build metadata at version 0.1.1 (build 2).

## [0.1.0] - 2026-07-16

- Initial public technical-beta release of the local-first Patchwright app, engine, and relay.

[0.2.0]: https://github.com/rsitech-ai/patchwright/compare/v0.1.1...v0.2.0-community.1
[0.1.1]: https://github.com/rsitech-ai/patchwright/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/rsitech-ai/patchwright/releases/tag/v0.1.0
