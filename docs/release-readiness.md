# Patchwright release readiness

Patchwright uses independent release gates. A green source build is not a
notarized release candidate, and a notarized candidate is not automatically a
published release.

Run `./script/release.sh` only from a clean commit. It builds immutable Swift and Rust Release components, validates the app, signs nested helpers and the app with Developer ID plus Hardened Runtime, notarizes and staples the app, creates/signs/notarizes/staples the DMG, verifies the mounted payload and Gatekeeper, and emits checksums plus JSON evidence.

## Current external prerequisites

- Exactly one `Developer ID Application: ...` identity must be available in the selected signing Keychain. Apple Development and Apple Distribution identities are deliberately rejected.
- `PATCHWRIGHT_SIGNING_KEYCHAIN` may point to an owner-only, absolute dedicated release Keychain. Packaging adds it to the user Keychain search list only for the release process and restores the prior list on exit.
- `PATCHWRIGHT_NOTARY_PROFILE` must name a `notarytool` Keychain profile. Raw Apple credentials are never accepted by the release scripts.
- Final clean-machine evidence must come from the notarized DMG on the documented disposable VM, not from a source-built or ad-hoc app.

After installing the Developer ID Application certificate, verify it with `security find-identity -p codesigning -v "$PATCHWRIGHT_SIGNING_KEYCHAIN"`. Unlock a dedicated release Keychain before packaging; its password belongs in operator-local secret storage, never in the repository or shell history.

Create the notary profile interactively without putting secrets in shell history:

```sh
xcrun notarytool store-credentials Patchwright
export PATCHWRIGHT_NOTARY_PROFILE=Patchwright
```

Then run:

```sh
export PATCHWRIGHT_DEVELOPER_ID='Developer ID Application: Exact Name (TEAMID)'
export PATCHWRIGHT_SIGNING_KEYCHAIN='/absolute/path/to/Release.keychain-db'
export PATCHWRIGHT_NOTARY_PROFILE=Patchwright
PATCHWRIGHT_VERSION=0.1.0 PATCHWRIGHT_BUILD=1 ./script/release.sh
```

The output remains under `~/.patchwright/release-work` until every active gate
succeeds. Packaging emits a digest-bound `notarized-candidate` manifest. It does
not upload files, create a GitHub release, or promote anything automatically.

After the exact candidate passes the documented clean-machine test, run the
separate `script/promote_release.sh` flow. Promotion revalidates the frozen
checksums and evidence, records the reviewer-owned gate, and produces
`promoted-release` status without rebuilding or changing the DMG. Only that
promoted artifact set is eligible for GitHub Releases.
