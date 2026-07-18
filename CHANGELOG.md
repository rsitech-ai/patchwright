# Changelog

All notable user-facing and distribution changes are documented here. Patchwright follows semantic versioning for public releases.

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

[0.1.1]: https://github.com/s1korrrr/patchwright/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/s1korrrr/patchwright/releases/tag/v0.1.0
