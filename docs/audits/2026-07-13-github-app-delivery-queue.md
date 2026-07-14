# GitHub App delivery and queue audit

Audit refreshed: 2026-07-14.

## Result

**Repo-ready; GitHub integration blocked:external.** The local implementation and mock/restart boundaries pass. A production GitHub App, Keychain private key, installation, and authorized disposable-repository mutation run do not exist on this machine yet.

## Verified locally

- GitHub ingestion discovers repositories, issues, pull requests, discussions, checks, and workflow runs through the authenticated `gh` read-only development fallback. Synchronization is a durable single-flight job with cancellation during discovery and fan-out; completed repository snapshots survive cancellation.
- Issue and pull-request snapshots convert idempotently into typed task contracts bound to repository identity and exact source SHAs.
- The relay mints scoped, cached, short-lived installation tokens from an RSA GitHub App key stored in Keychain or a protected file. Secret material is redacted and tokens are never persisted.
- Branch, comment, review, check, draft-PR, update, enqueue, and exact-SHA merge payloads have typed previews and stable idempotency identities.
- Delivery and merge require preview, a separate matching approval, a fresh precondition check, a single-use claim, and explicit execution. Merge uses its own approval class.
- Twelve deterministic workflow presets persist their ordering, reasons, dependencies, overlap findings, and input hashes.
- Durable monitoring covers pending/success/failure, requested changes, dismissed approval, new head/base SHA, conflicts, inaccessible forks, rate limits, network loss, bounded exponential backoff with jitter, repair-budget exhaustion, webhook wakeup, cancellation, restart, and approval invalidation.
- The native queue and detail pane were exercised in the running release build. Sorting includes latest commit and latest update. The workflow menu exposed all twelve presets, and CI Rescue was applied successfully.

## Current production boundary

- Initial repository discovery still uses `gh` as an explicitly labeled development/read-only fallback. Production writes use the GitHub App broker. App-authenticated account-wide discovery remains an integration follow-up after the App is installed.
- `~/.patchwright/github-app.json` is missing and the expected Keychain item is absent.
- No remote qualification write or merge was attempted against `s1korrrr/patchwright`.
- `script/smoke_github_app.sh` rejects the production repository, requires an exact disposable-repository allowlist and one-shot confirmation, validates file and Keychain boundaries, verifies repository identity, runs all local suites, and exits `78` until the remote mutation sequence has been performed and recorded.

## Required external sequence

1. Create the prepared GitHub App with webhook disabled and repository permissions: Actions read, Administration read, Checks write, Contents write, Issues write, Pull requests write, Workflows write, Metadata read.
2. Generate its private key and import it through Patchwright Settings. Only the Keychain reference is persisted.
3. Install the App on one disposable repository, not the Patchwright repository.
4. Set the disposable target variables printed by `script/smoke_github_app.sh` and run the authorized sequence.
5. Record remote IDs and URLs, verify no secret appears in the database, logs, process table, Git configuration, or bundle, then change `integration_ready.github_delivery_merge` only after the full sequence passes.
