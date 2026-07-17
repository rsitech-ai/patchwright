# Direct release checklist

## Repository gate

- [ ] Final source is committed, tagged as `v<version>`, and the tag, HEAD, index, tracked worktree, and untracked-file set remain exactly clean at the final candidate boundary.
- [ ] The dedicated release Keychain is unlocked, owner-only, and selected through `PATCHWRIGHT_SIGNING_KEYCHAIN`; the previous user Keychain search list is restored after packaging.
- [ ] CI, `./script/verify.sh`, `./script/smoke.sh`, and `./script/smoke_codex.sh` pass.
- [ ] Publication secret scan, license inventory, SBOM, notices, and reproducibility evidence pass.
- [ ] The approval-gated GitHub lifecycle and automation kill switch are exercised only in an authorized disposable repository.

## Candidate gate

- [ ] `PATCHWRIGHT_VERSION=<version> PATCHWRIGHT_BUILD=<build> ./script/release.sh` completes with status `notarized-candidate`.
- [ ] The app and DMG have Developer ID signatures, Hardened Runtime, one Team ID, accepted notarization tickets, staples, and successful Gatekeeper assessments.
- [ ] `appcast.xml`, its EdDSA signatures, the existing read-only DMG checksum sidecar, SBOM, notices, committed-source archive digest, `build_metadata.dirty == false`, and every evidence digest pass `verify_release_evidence.py candidate`.
- [ ] The exact candidate DMG passes `docs/clean-machine-test-plan.md`, including install, relaunch, update, offline/revoked states, and recovery.

## Promotion gate

- [ ] A second operator or reviewer checks the immutable candidate manifest and supplies the schema-v2 reviewer identity, per-check evidence paths/digests, and clean-machine evidence manifest.
- [ ] `script/promote_release.sh` accepts that exact digest and emits status `promoted-release` without changing candidate bytes.
- [ ] `promotion-manifest.json` binds the candidate manifest, every gate, `release-evidence.json`, and `release-assets.json`; `promotion-readiness.json` binds the promotion manifest digest.
- [ ] The promoted release is uploaded to GitHub Releases with the DMG, `.sha256`, and `appcast.xml`; the public URLs and updater feed are rechecked.
- [ ] The release report lists any remaining external gate independently and never upgrades a partial result to ready.
