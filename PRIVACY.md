# Privacy

Patchwright is a local-first macOS application. Its engine, task state,
repository snapshots, approval records, and runtime evidence are stored on the
user's Mac under `~/.patchwright` by default. Repository worktrees and commands
operate on paths selected or approved by the user.

## Network activity

Patchwright connects to external systems only when a feature requires them:

- GitHub sync and approved GitHub operations connect to the configured GitHub
  API. Read-only sync obtains the current `gh auth token` in memory and does not
  store that token in Patchwright's database or logs.
- A configured GitHub App relay exchanges repository-scoped credentials and
  GitHub data with GitHub. GitHub App private keys remain operator-managed in
  Keychain or an owner-only protected file and must never be bundled with the
  app.
- Codex actions start the separately installed Codex CLI. Its network use and
  data handling depend on the user's Codex configuration and provider terms.

GitHub and any configured model provider process data under their own privacy
terms. Repository content, issue and pull-request data, prompts, diffs, command
output, and other task context may be sent to those services when the user
invokes the corresponding feature.

Patchwright's beta does not include a Patchwright-operated analytics or
advertising service. Local diagnostic logs may contain repository names, file
paths, command output, or task metadata; inspect and redact logs before sharing
them.

## User control

Quit Patchwright before removing its local data directory. Removing
`~/.patchwright` deletes Patchwright's local database, task state, evidence,
configuration, and staged artifacts but does not delete data already held by
GitHub, Codex, a model provider, or a repository remote. Revoke GitHub CLI or
GitHub App access through the relevant GitHub account settings.

This disclosure describes the current beta. Material changes to data handling
should update this document in the same public source revision as the change.
