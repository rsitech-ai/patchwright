# Patchwright clean-machine validation

Run this only with the final notarized DMG in a disposable Apple-silicon macOS 26 or newer VM. The VM must begin without a source checkout, developer toolchain, Patchwright state, GitHub CLI/session, Codex installation/session, or running Patchwright process.

1. Snapshot the pristine VM and record macOS build, hardware/VM identity, network mode, and snapshot identifier.
2. Copy in only the DMG, its existing `.sha256`, and `script/clean_machine_probe.sh` through a trusted read-only transfer.
3. Verify the checksum, then run `PATCHWRIGHT_CLEAN_MACHINE=1 ./clean_machine_probe.sh <dmg> <evidence-dir> <dmg.sha256>`. Verification reads the explicit sidecar and never creates or overwrites it.
4. Confirm first launch explains missing Codex and GitHub App installation without crashing or silently using `gh`.
5. Install Codex from its official source, sign in, start one disposable task thread, quit, relaunch, and verify the thread resumes.
6. Install the Patchwright GitHub App only on the allowlisted disposable repository. Verify ingestion without `gh`, then test offline, expired-token, revoked-installation, and missing-permission states.
7. Complete one approval-bound disposable branch/draft-PR/check/review flow. Change the head SHA and prove the old delivery and merge approvals invalidate.
8. Exercise the exact-SHA merge approval in the disposable repository or its required native merge queue. Verify post-merge queue advancement.
9. Install the prior schema fixture, create task/queue/thread state, replace the app with the final DMG build, and prove migration plus relaunch are idempotent.
10. Remove the app while retaining `~/.patchwright`, then separately verify explicit data removal on a copied disposable home directory.

Redact private repository content and all identifiers that could act as credentials. Keep exact notarization, Gatekeeper, app version/build, disposable repository URL, PR number, and merge SHA in the private evidence bundle.

## Promotion evidence schema

Every numbered check above must have a corresponding regular evidence file and
one schema-v2 `clean_machine` gate entry with `status: "pass"`, a candidate-root
relative `path`, and the file's lowercase SHA-256 digest. The required check
keys are the base checksum/signing/ticket/Gatekeeper/launch/relaunch checks plus
missing-integration guidance, Codex thread resume, GitHub ingestion without
`gh`, offline/expired/revoked/missing-permission states, approval delivery,
stale-head rejection, exact-SHA merge, queue advancement, migration/update,
uninstall retention, and explicit data removal.

The gate must identify an independent reviewer by name and stable operator
identity, set `independent: true`, and bind a separate
`patchwright.clean-machine-evidence-manifest` containing exactly the same
per-check paths and digests. `verify_release_evidence.py promotion` rejects a
missing reviewer, incomplete matrix, symlink, escaping path, digest mismatch,
or manifest mismatch.
