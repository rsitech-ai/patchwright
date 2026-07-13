# Release checklist

- [ ] `./script/verify.sh` passes from final source.
- [ ] `./script/smoke.sh` passes with a disposable local database/socket.
- [ ] `./script/build_and_run.sh --verify` launches the exact staged app.
- [ ] GitHub App webhook and API flows pass against a non-production test repository.
- [ ] Bundle identifier, signing, sandbox, privacy manifest, icon, and accessibility audit are complete.
- [ ] App Store Connect privacy, category, age rating, URLs, screenshots, and review notes are owner-approved.
- [ ] Merge remains disabled and the kill switch is exercised.
