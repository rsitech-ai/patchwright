# Patchwright

Patchwright is a local-first, macOS-native engineering control plane for GitHub. It separates private language-model assistance from a durable coding-agent runtime and makes every approval, command, diff, test, and remote mutation auditable.

Build Patchwright from [source](#build-and-verify). Official Developer ID-signed
and Apple-notarized downloads are published through
[GitHub Releases](https://github.com/s1korrrr/patchwright/releases). See the
[direct-download guide](docs/direct-download.md) for installation and
verification.

Project policies: [Contributing](CONTRIBUTING.md) ·
[Security](SECURITY.md) · [Privacy](PRIVACY.md) · [Support](SUPPORT.md) ·
[Code of Conduct](CODE_OF_CONDUCT.md)

Patchwright is available under your choice of the
[MIT License](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE).

This repository contains the Stage 1–3 MVP:

- `Patchwright`: native SwiftUI task, GitHub inspection, approval, and Codex-session client.
- `patchwright-engine`: Rust task state, policy, instruction resolution, worktrees, argv-safe commands, SQLite recovery, and Unix-socket JSON-RPC.
- `patchwright-relay`: signature-verifying GitHub webhook ingress plus draft-PR and check-run API adapters.

## Build and verify

Source builds require macOS 26+, Xcode 26+, Swift 6.2+, Rust 1.91.0, and Git.
The repository's `rust-toolchain.toml` pins Rust for local and CI builds. GitHub
CLI and Codex are optional runtime integrations described below; neither is
bundled.

```bash
./script/verify.sh
./script/smoke.sh
./script/build_and_run.sh --verify
```

The Codex Run action executes `./script/build_and_run.sh`. It signs the app in the user-only `~/.patchwright/staged` directory and exposes it at `dist/Patchwright.app` through a stable symlink, avoiding File Provider metadata races in Documents workspaces.

## Choose only the access you need

| Capability | Prerequisite | Boundary |
| --- | --- | --- |
| Read-only GitHub sync | Install `gh`, sign in with your own GitHub account, and make sure `gh` is on PATH. | No GitHub App or private key is required. Patchwright's `gh` ingestion path does not issue GitHub mutations, even if the credential has broader scopes. |
| Coding-agent sessions | Install the Codex CLI separately, sign in, and make sure `codex` is on PATH before launching Patchwright. | Codex is not bundled. GitHub sync and review remain available without it. Relaunch Patchwright after installing or updating Codex. |
| GitHub mutations | Create and own a GitHub App, install it only on selected repositories, and configure its App ID, Client ID, and owner-only private-key reference in Settings. | Official downloads and source contain no publisher App credential or private key. Every write requires an exact preview, a short-lived matching approval, and a separate Execute action. |

Never copy, request, or share the project publisher's GitHub App private key.
Create a separate App under an account or organization you control.

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

PATCHWRIGHT_GITHUB_WEBHOOK_SECRET_FILE="$HOME/.patchwright/webhook-secret" \
PATCHWRIGHT_RELAY_DATABASE="$HOME/.patchwright/relay.sqlite" \
PATCHWRIGHT_ENGINE_SOCKET="$HOME/.patchwright/engine.sock" \
cargo run -p patchwright-relay -- serve --address 127.0.0.1:8787
```

The relay accepts only IPv4 or IPv6 loopback addresses; terminate authenticated
HTTPS or a tunnel in front of it. Create the webhook secret file outside the
checkout with owner-only mode `0400` or `0600`; the file contains the raw webhook
secret and must never be committed or passed as a command-line argument. The
relay durably retries accepted sanitized events to the owner-only engine socket;
an engine outage does not require GitHub redelivery. GitHub
App metadata and a Keychain or owner-only protected-key reference are your
configuration and are never committed. See [Choose only the access you
need](#choose-only-the-access-you-need). The engine brokers repository-scoped,
short-lived installation tokens for installed-repository ingestion and every
approved mutation.

## Direct macOS distribution

`script/build_and_run.sh` is the ad-hoc local-development path. Direct
distribution uses `script/release.sh`, which refuses dirty source and refuses
Apple Development, Apple Distribution, or ad-hoc signing identities. Packaging
creates a digest-bound `notarized-candidate`; publication is a separate,
explicit promotion step. See the [direct-download guide](docs/direct-download.md),
[release readiness](docs/release-readiness.md), and
[clean-machine test plan](docs/clean-machine-test-plan.md).

## Safety

Merge is disabled by default and can execute only for a typed pull-request task after an exact action preview, a separate merge-class approval, a fresh exact-head-SHA precondition, and a single-use execution claim. GitHub writes, network access, dependency installation, and workflow changes require action-specific approval. Set `PATCHWRIGHT_AUTOMATION_DISABLED=1` to fail closed for every mutating capability while retaining read-only inspection.

See [the product design](docs/superpowers/specs/2026-07-13-patchwright-stages-1-3-design.md), [production plan](docs/production-plan.md), and [security operations](docs/security.md).
