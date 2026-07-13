# Security and operations

GitHub issue text, comments, repository files, branch names, diffs, and command output are untrusted data. Only configured policy and an unexpired action-specific approval can authorize a mutation. Commands cross the RPC boundary as executable plus argv; shell strings are not accepted.

The webhook relay verifies `X-Hub-Signature-256` against the raw bounded body before JSON parsing and deduplicates `X-GitHub-Delivery`. Run it behind authenticated HTTPS termination. Supply webhook secrets, GitHub App private keys, and installation identifiers only through a secret manager or process environment.

Stop the engine or use `task.cancel` to retain the worktree for inspection. Set `PATCHWRIGHT_AUTOMATION_DISABLED=1` for the global kill switch. Never expose the engine Unix socket outside the user account, and store `~/.patchwright` with user-only permissions.

Recommended GitHub App permissions are Metadata read, Contents read/write, Issues read/write, Pull requests read/write, Checks read/write, and Actions read. Workflow write is intentionally excluded from the normal installation.

