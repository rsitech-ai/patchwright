# Patchwright

Patchwright is a local-first, macOS-native engineering control plane for GitHub. It separates private language-model assistance from a durable coding-agent runtime and makes every approval, command, diff, test, and remote mutation auditable.

This repository contains the Stage 1–3 MVP:

- `Patchwright`: native SwiftUI review and task client with Apple Foundation Models availability handling.
- `patchwright-engine`: Rust task state, policy, instruction resolution, worktrees, argv-safe commands, SQLite recovery, and Unix-socket JSON-RPC.
- `patchwright-relay`: signature-verifying GitHub webhook ingress plus draft-PR and check-run API adapters.

## Build and verify

Requirements: macOS 26+, Xcode 26+, Swift 6.2+, Rust 1.85+, Git, and the Codex CLI for coding-agent sessions.

```bash
./script/verify.sh
./script/smoke.sh
./script/build_and_run.sh --verify
```

The Codex Run action executes `./script/build_and_run.sh` and stages `dist/Patchwright.app`.

## Local services

```bash
cargo run -p patchwright-engine -- serve \
  --socket "$HOME/.patchwright/engine.sock" \
  --database "$HOME/.patchwright/patchwright.sqlite3"

PATCHWRIGHT_GITHUB_WEBHOOK_SECRET='runtime-secret' \
cargo run -p patchwright-relay -- --address 127.0.0.1:8787
```

The relay binds to loopback by default. Production HTTPS termination, GitHub App installation, private keys, and installation-token brokering are operator configuration and are never committed.

## Safety

Merge is disabled in code. GitHub writes, network access, dependency installation, and workflow changes require action-specific approval. Set `PATCHWRIGHT_AUTOMATION_DISABLED=1` to fail closed for every mutating capability while retaining read-only inspection.

See [the product design](docs/superpowers/specs/2026-07-13-patchwright-stages-1-3-design.md), [production plan](docs/production-plan.md), and [security operations](docs/security.md).

