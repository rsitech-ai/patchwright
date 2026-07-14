# GitHub App delivery and queue audit

Audit refreshed: 2026-07-14.

## Result

**Repo-ready; GitHub App authentication verified; repository integration blocked:external.** The local implementation and mock/restart boundaries pass. The production App identity and protected private key exist and authenticate successfully. A repository installation and authorized disposable-repository mutation run do not exist yet.

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
- `~/.patchwright/github-app.json` has owner-only mode `600`, references App ID `4294269`, and resolves to an owner-only protected private-key file. `patchwright-relay github-app-health` authenticated to GitHub and returned the expected `patchwright-s1korrrr` identity without printing key or JWT material.
- No remote qualification write or merge was attempted against `s1korrrr/patchwright`.
- `script/smoke_github_app.sh` rejects the production repository, requires an exact disposable-repository allowlist and one-shot confirmation, validates metadata plus Keychain or protected-file boundaries, authenticates the configured App identity, verifies repository identity, and runs the local suites before mutation. Its remote sequence ingests a fixture issue, converts it to a typed task, creates a branch, check, comment, and draft pull request through exact preview/approval/execute calls, ingests that pull request into a second typed task, posts a review, performs a separately approved exact-SHA merge, reconciles every remote result, scans durable evidence for credentials, and writes an owner-only evidence record.
- Delivery preview now rejects any GitHub action whose capability is absent from the typed task contract. Issue and pull-request conversion declare their lifecycle capabilities explicitly.

## Required external sequence

1. Install `Patchwright s1korrrr` on one disposable repository, not the Patchwright repository. The audited permissions are Actions read, Administration read, Checks write, Contents write, Issues write, Pull requests write, Workflows write, and mandatory Metadata read; the webhook remains disabled.
2. Set the disposable target variables printed by `script/smoke_github_app.sh` and run the authorized sequence.
3. Record remote IDs and URLs, verify no secret appears in the database, logs, process table, Git configuration, or bundle, then change `integration_ready.github_delivery_merge` only after the full sequence passes.
