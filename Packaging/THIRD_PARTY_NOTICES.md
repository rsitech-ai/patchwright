# Third-Party Notices

Patchwright includes open-source Swift and Rust dependencies resolved by `Package.resolved` where present and `Cargo.lock`.

The Rust dependency graph contains packages distributed under permissive licenses including MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, and Zlib. The authoritative package names, versions, checksums, and dependency edges are retained in `Cargo.lock` and the generated release metadata.

Patchwright does not bundle Codex. It discovers a separately installed, signed-in Codex executable at runtime.
