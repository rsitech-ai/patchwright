#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p patchwright-core --test sorting_contract
cargo test -p patchwright-engine --test task_conversion
cargo test -p patchwright-engine --test rpc_conversion
cargo test -p patchwright-engine --test rpc_socket
cargo test --workspace
swift test --filter WorkspaceSortingTests
swift test --filter ConversionStoreTests
swift test --filter WorkspacePresentationTests
swift test
swift build -c release -Xswiftc -warnings-as-errors
