#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
bash -n script/smoke_github_app.sh
Tests/PackagingTests/github_app_smoke_contract.sh
Tests/PackagingTests/release_contract.sh
cargo test -p patchwright-core --test sorting_contract
cargo test -p patchwright-engine --test task_conversion
cargo test -p patchwright-engine --test rpc_conversion
cargo test -p patchwright-engine --test rpc_socket
cargo test -p patchwright-engine --test codex_protocol --test codex_process --test codex_session --test codex_rpc --test codex_approvals --test codex_cancellation
cargo test --workspace
swift test --filter WorkspaceSortingTests
swift test --filter ConversionStoreTests
swift test --filter WorkspacePresentationTests
swift test
swift build -c release -Xswiftc -warnings-as-errors
