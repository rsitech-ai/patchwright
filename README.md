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

The Codex Run action executes `./script/build_and_run.sh`. It signs the app in the user-only `~/.patchwright/staged` directory and exposes it at `dist/Patchwright.app` through a stable symlink, avoiding File Provider metadata races in Documents workspaces.

## Ingest your GitHub workspace

Patchwright can build a read-only local snapshot from the GitHub account already authenticated by the GitHub CLI. Confirm the account once:

```bash
gh auth status
```

Launch the app and choose **Sync GitHub** in the toolbar or **Task → Sync GitHub** (`⌘⇧G`). The default sync discovers up to 100 accessible repositories and, per repository, ingests up to 1,000 records from each paginated resource:

- issues and pull requests;
- issue comments, pull-request review comments, and submitted reviews;
- check runs for ingested pull-request head commits;
- GitHub Actions workflow runs.

Repository snapshots are replaced atomically in `~/.patchwright/patchwright.sqlite3`; a failed repository refresh preserves its previous complete snapshot. The app shows partial failures and the latest local snapshot time. The database is restricted to the current user, and the `gh auth token` value exists only in engine memory—it is not stored in SQLite or logs.

This ingestion surface is read-only. It does not post comments, change labels, push branches, submit reviews, rerun workflows, or merge pull requests. GitHub mutations remain behind the separate approval-gated GitHub App lifecycle.

## Local services

```bash
cargo run -p patchwright-engine -- serve \
  --socket "$HOME/.patchwright/engine.sock" \
  --database "$HOME/.patchwright/patchwright.sqlite3"

PATCHWRIGHT_GITHUB_WEBHOOK_SECRET='runtime-secret' \
cargo run -p patchwright-relay -- --address 127.0.0.1:8787
```

The relay binds to loopback by default. GitHub App metadata and a Keychain or owner-only protected-key reference are operator configuration and are never committed. The engine brokers repository-scoped, short-lived installation tokens for installed-repository ingestion and every approved mutation.

## Direct macOS distribution

`script/build_and_run.sh` is the ad-hoc local-development path. Direct distribution uses `script/release.sh`, which refuses dirty source and refuses Apple Development, Apple Distribution, or ad-hoc signing identities. See `docs/release-readiness.md` and `docs/clean-machine-test-plan.md` for the Developer ID, notarization, Gatekeeper, and clean-machine gates.

## Safety

Merge is disabled by default and can execute only for a typed pull-request task after an exact action preview, a separate merge-class approval, a fresh exact-head-SHA precondition, and a single-use execution claim. GitHub writes, network access, dependency installation, and workflow changes require action-specific approval. Set `PATCHWRIGHT_AUTOMATION_DISABLED=1` to fail closed for every mutating capability while retaining read-only inspection.

See [the product design](docs/superpowers/specs/2026-07-13-patchwright-stages-1-3-design.md), [production plan](docs/production-plan.md), and [security operations](docs/security.md).
