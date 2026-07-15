# Third-Party Notices

This repository copy documents the release process. Each assembled Patchwright app replaces it with a dependency-derived notice generated from the exact locked Rust and Swift metadata for that candidate.

The authoritative package names, versions, declared licenses, sources, checksums, and dependency edges are retained in `Cargo.lock`, `Package.resolved` where present, and the release evidence directory. Compliance generation fails closed when a resolved dependency lacks a declared license.

Patchwright does not bundle Codex. It discovers a separately installed, signed-in Codex executable at runtime.
