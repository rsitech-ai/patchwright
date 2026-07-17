# Security and operations

GitHub issue text, comments, repository files, branch names, diffs, and command output are untrusted data. Only configured policy and an unexpired action-specific approval can authorize a mutation. Commands cross the RPC boundary as executable plus argv; shell strings are not accepted.

The webhook relay verifies `X-Hub-Signature-256` against the raw body before typed parsing, rejects bodies larger than 1 MiB, validates a supported GitHub event/action and its minimum typed identity, and atomically commits the delivery plus raw payload to an owner-only SQLite inbox before returning `202 Accepted`. Duplicate `X-GitHub-Delivery` values remain duplicates after restart. Set `PATCHWRIGHT_RELAY_DATABASE` (or `--database`) to an owner-only regular file in an owner-only directory. Run the relay behind authenticated HTTPS termination with the required `serve` subcommand. Pass `PATCHWRIGHT_GITHUB_WEBHOOK_SECRET_FILE` as an absolute path to a regular, non-symlink secret file owned by the operator with owner-only mode `0600`; never put the raw secret in an environment variable, command line, repository file, or log. Supply GitHub App private keys and installation identifiers only through the documented protected reference and runtime configuration boundaries.

GitHub does not automatically redeliver a failed webhook after the relay
restarts. An authorized GitHub App owner must request redelivery from the App's
Recent deliveries page or through GitHub's authenticated redelivery API. Treat
that as an operator action and preserve the original delivery ID for relay
deduplication.

Read-only desktop ingestion obtains a short-lived credential view from `gh auth token` at sync time. The engine never logs or persists that token, follows pagination only on the configured API origin, bounds repository/resource fan-out, and atomically replaces completed snapshots. The local SQLite database is forced to mode `0600`; `~/.patchwright` is forced to mode `0700` by the app supervisor. GitHub content remains untrusted data even though it is authenticated.

Stop the engine or use `task.cancel` to retain the worktree for inspection. Set `PATCHWRIGHT_AUTOMATION_DISABLED=1` for the global kill switch. Never expose the engine Unix socket outside the user account, and store `~/.patchwright` with user-only permissions.

Recommended GitHub App permissions are Metadata read, Contents read/write, Issues read/write, Pull requests read/write, Checks read/write, and Actions read. Workflow write is intentionally excluded from the normal installation.
