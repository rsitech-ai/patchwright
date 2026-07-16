# GitHub App delivery and queue audit

Audit refreshed: 2026-07-14.

## Result

**Integration-ready.** The local implementation, mock/restart boundaries, production App identity, exact-repository installation-token ingestion, and authorized disposable-repository delivery/merge lifecycle all pass.

## Verified locally

- GitHub ingestion discovers repositories, issues, pull requests, discussions, checks, and workflow runs through the authenticated `gh` read-only development fallback. Synchronization is a durable single-flight job with cancellation during discovery and fan-out; completed repository snapshots survive cancellation. Installed repositories can also be synchronized exactly through a repository-scoped GitHub App installation token with read-only permissions and no account-wide discovery.
- Issue and pull-request snapshots convert idempotently into typed task contracts bound to repository identity and exact source SHAs.
- The relay mints scoped, cached, short-lived installation tokens from an RSA GitHub App key stored in Keychain or a protected file. Secret material is redacted and tokens are never persisted.
- Branch, comment, review, check, draft-PR, update, enqueue, and exact-SHA merge payloads have typed previews and stable idempotency identities.
- Delivery and merge require preview, a separate matching approval, a fresh precondition check, a single-use claim, and explicit execution. Merge uses its own approval class.
- Twelve deterministic workflow presets persist their ordering, reasons, dependencies, overlap findings, and input hashes.
- Durable monitoring covers pending/success/failure, requested changes, dismissed approval, new head/base SHA, conflicts, inaccessible forks, rate limits, network loss, bounded exponential backoff with jitter, repair-budget exhaustion, webhook wakeup, cancellation, restart, and approval invalidation.
- The native queue and detail pane were exercised in the running release build. Sorting includes latest commit and latest update. The workflow menu exposed all twelve presets, and CI Rescue was applied successfully.

## Live production-App qualification

- `Patchwright s1korrrr` is installed only on the private disposable repository `s1korrrr/patchwright-e2e-sandbox`. The production `s1korrrr/patchwright` repository remained outside the installation and was rejected by the qualification guard.
- `~/.patchwright/github-app.json` has owner-only mode `600`, references App ID `4294269`, and resolves to an owner-only protected private-key file. `patchwright-relay github-app-health` authenticated to GitHub and returned the expected `patchwright-s1korrrr` identity without printing key or JWT material.
- `script/smoke_github_app.sh` rejects the production repository, requires an exact disposable-repository allowlist and one-shot confirmation, validates metadata plus Keychain or protected-file boundaries, authenticates the configured App identity, verifies repository identity through the installation token, and runs the local suites before mutation. It supports either an authenticated `gh` fixture or a narrowly manual UI fixture when the developer credential is unavailable; all Patchwright ingestion and mutation steps still use the App token.
- Issue [#3](https://github.com/s1korrrr/patchwright-e2e-sandbox/issues/3) was ingested through the read-only installation token and converted to typed task `13a09acc-115e-42e7-bf05-e3d9f41e675a`.
- Patchwright created branch `patchwright/e2e-20260714T104059Z`, a successful check run, an App-authored comment, and draft PR [#4](https://github.com/s1korrrr/patchwright-e2e-sandbox/pull/4) through distinct preview, approval, and execute calls.
- PR #4 was re-ingested and converted to typed task `2d501038-9741-4ae5-ac16-3b13fe485bcc`. Patchwright posted an App-authored review and performed a separately approved squash merge bound to head `8205a0cc63801525c2202d8101b2136a2d057ffe`; GitHub produced merge commit `e0c4157ea9d616053d6c3cd26f2ce2b2f8d8b231`.
- App-token reconciliation observed the comment, review, check, and closed PR. The durable database and engine log scan found no private key or installation token. The owner-only evidence is `/Users/s1kor/.patchwright/evidence/github-app-e2e-20260714T104059Z.json` (directory `700`, file `600`).
- Delivery preview now rejects any GitHub action whose capability is absent from the typed task contract. Issue and pull-request conversion declare their lifecycle capabilities explicitly.
- Delivery action JSON now uses the Swift-facing camelCase field contract while continuing to decode legacy snake_case records.

## Remaining boundary

Account-wide discovery remains an explicitly labeled local-development `gh` fallback because GitHub App installations are repository-scoped rather than user-account enumerators. Production installed-repository ingestion and all writes use the App broker. Developer ID signing, notarization, Gatekeeper, and clean-machine distribution remain independent Apple-controlled release gates.
