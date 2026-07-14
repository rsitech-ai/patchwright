# Release checklist

- [ ] `./script/verify.sh` passes from final clean source.
- [ ] `./script/smoke.sh` and `./script/smoke_codex.sh` pass.
- [ ] The authorized disposable GitHub App delivery/merge smoke passes; no production repository is used for qualification writes.
- [ ] `./script/release.sh` emits an accepted, stapled app and DMG with verified checksums.
- [ ] `codesign`, Hardened Runtime, nested Team IDs, entitlements, notarization tickets, and Gatekeeper are green.
- [ ] The final DMG passes `docs/clean-machine-test-plan.md`, including install, relaunch, update, offline/revoked states, and recovery.
- [ ] The final accessibility, resize, theme, runtime-log, secret, license, and reproducibility audits are green.
- [ ] Approval-gated merge is exact-SHA bound; changed remote state invalidates approval; the automation kill switch is exercised.
- [ ] The release report names every remaining external gate independently.
