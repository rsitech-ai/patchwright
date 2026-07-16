# Patchwright clean-machine validation

Run this only with the final notarized DMG in a disposable Apple-silicon macOS 26 or newer VM. The VM must begin without a source checkout, developer toolchain, Patchwright state, GitHub CLI/session, Codex installation/session, or running Patchwright process.

1. Snapshot the pristine VM and record macOS build, hardware/VM identity, network mode, and snapshot identifier.
2. Copy in only the DMG, its `.sha256`, and `script/clean_machine_probe.sh` through a trusted read-only transfer.
3. Verify the checksum, then run `PATCHWRIGHT_CLEAN_MACHINE=1 ./clean_machine_probe.sh <dmg> <evidence-dir>`.
4. Confirm first launch explains missing Codex and GitHub App installation without crashing or silently using `gh`.
5. Install Codex from its official source, sign in, start one disposable task thread, quit, relaunch, and verify the thread resumes.
6. Install the Patchwright GitHub App only on the allowlisted disposable repository. Verify ingestion without `gh`, then test offline, expired-token, revoked-installation, and missing-permission states.
7. Complete one approval-bound disposable branch/draft-PR/check/review flow. Change the head SHA and prove the old delivery and merge approvals invalidate.
8. Exercise the exact-SHA merge approval in the disposable repository or its required native merge queue. Verify post-merge queue advancement.
9. Install the prior schema fixture, create task/queue/thread state, replace the app with the final DMG build, and prove migration plus relaunch are idempotent.
10. Remove the app while retaining `~/.patchwright`, then separately verify explicit data removal on a copied disposable home directory.

Redact private repository content and all identifiers that could act as credentials. Keep exact notarization, Gatekeeper, app version/build, disposable repository URL, PR number, and merge SHA in the private evidence bundle.
