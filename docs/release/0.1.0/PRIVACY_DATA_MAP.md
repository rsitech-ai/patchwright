# Privacy and Data Map

## Product posture

Patchwright is a local-first macOS developer tool. The current repository has
no analytics or advertising SDK and declares no tracking in
`Packaging/PrivacyInfo.xcprivacy`. This map is engineering evidence, not the
owner's final App Privacy declaration or legal advice.

| Data | Source | Purpose | Storage/transfer | User control |
| --- | --- | --- | --- | --- |
| Repository metadata, issues, PRs, reviews, checks, and workflow status | GitHub API/App | Display and task orchestration | Local SQLite; fetched from GitHub | User selects/authorizes repositories and can revoke GitHub access |
| Source files, diffs, task instructions, and command output | User-selected local repositories and Codex task | Review, implementation, verification | Local worktrees/evidence; may be sent to the configured Codex service when the user starts a Codex task | User selects repository and approves protected operations; Codex account/provider terms also apply |
| GitHub credentials and installation tokens | `gh`, GitHub App, Keychain/protected file | Authenticated API and Git operations | Tokens intended to remain in memory; private key stays in Keychain or owner-only file | User controls GitHub authorization and local credential source |
| GitHub webhook payloads and delivery identifiers | GitHub | Lifecycle updates and deduplication | Bounded local relay state/event store | User controls GitHub App/webhook configuration |
| App settings and repository bindings | User/App | Restore local workspace | `~/.patchwright` and app preferences | User can remove local data and revoke access |
| Diagnostic logs | App/OS | Reliability and support | Unified log; intended to contain state/identifiers, not secrets or source contents | User controls local logs and diagnostic sharing |

## Current manifest

The privacy manifest declares:

- tracking: false;
- no tracking domains;
- no collected-data categories;
- no required-reason API categories.

Before submission, re-audit actual binaries and APIs against Apple's current
privacy-manifest requirements: [Privacy manifest files](https://developer.apple.com/documentation/bundleresources/privacy-manifest-files).

## Decisions still required

- Confirm whether data sent to the user's configured Codex service must be
  disclosed in App Privacy answers or privacy-policy language.
- Confirm retention/deletion behavior for SQLite, evidence, logs, worktrees,
  webhook deliveries, and imported private-key files.
- Publish a privacy policy and support contact.
- Confirm no analytics, crash reporting, telemetry, or new SDK is added before
  the final binary audit.
- Complete App Privacy answers in App Store Connect; do not infer them solely
  from the repository manifest.
