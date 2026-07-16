# Direct release checklist

## Repository gate

- [ ] Final source is committed, tagged as `v<version>`, and the tag resolves to HEAD.
- [ ] CI, `./script/verify.sh`, `./script/smoke.sh`, and `./script/smoke_codex.sh` pass.
- [ ] Publication secret scan, license inventory, SBOM, notices, and reproducibility evidence pass.
- [ ] The approval-gated GitHub lifecycle and automation kill switch are exercised only in an authorized disposable repository.

## Candidate gate

- [ ] `PATCHWRIGHT_VERSION=<version> PATCHWRIGHT_BUILD=<build> ./script/release.sh` completes with status `notarized-candidate`.
- [ ] The app and DMG have Developer ID signatures, Hardened Runtime, one Team ID, accepted notarization tickets, staples, and successful Gatekeeper assessments.
- [ ] `appcast.xml`, its EdDSA signatures, the DMG checksum sidecar, SBOM, notices, and every evidence digest pass `verify_release_evidence.py candidate`.
- [ ] The exact candidate DMG passes `docs/clean-machine-test-plan.md`, including install, relaunch, update, offline/revoked states, and recovery.

## Promotion gate

- [ ] A second operator or reviewer checks the immutable candidate manifest and clean-machine report.
- [ ] `script/promote_release.sh` accepts that exact digest and emits status `promoted-release` without changing candidate bytes.
- [ ] The promoted release is uploaded to GitHub Releases with the DMG, `.sha256`, and `appcast.xml`; the public URLs and updater feed are rechecked.
- [ ] The release report lists any remaining external gate independently and never upgrades a partial result to ready.
