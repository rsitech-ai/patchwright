# Releasing Patchwright

Patchwright separates source readiness, signed candidate creation, independent
promotion, and public publication. A green source build is not a release.

1. Start from a clean, reviewed commit and complete
   [release readiness](release-readiness.md).
2. Run the complete source gate:

   ```sh
   ./script/verify.sh
   ./script/smoke.sh
   ```

3. Build the Developer ID-signed and notarized candidate with the owner-local
   Keychain configuration documented in
   [release readiness](release-readiness.md). `script/release.sh` does not
   upload or publish anything.
4. Execute the [clean-machine test plan](clean-machine-test-plan.md) against the
   exact notarized DMG and record every required receipt.
5. Promote the frozen candidate with `script/promote_release.sh`. Promotion
   revalidates candidate digests and independent clean-machine evidence; it
   does not rebuild the app.
6. Review the [release checklist](release-checklist.md), release notes, asset
   manifest, checksums, SBOM, third-party notices, appcast, and rollback plan.
7. Request approval for the exact push, pull request, merge, tag, GitHub Release,
   repository settings, and profile actions. These are separate external
   changes.
8. After publication, inspect the repository signed out and verify the README,
   licenses, security form, issue forms, pull-request template, release assets,
   checksums, topics, description, branch rules, and Actions permissions. Test
   installation using the public DMG and checksum, not local build output.

Do not publish an artifact assembled from a dirty tree, a different commit, or
evidence copied from another candidate. Do not bypass Gatekeeper, notarization,
the clean-machine gate, or independent promotion evidence.

## Community prerelease path

The community path is a separate, lower-trust lane for review and evaluation.
It never replaces or relaxes the Developer ID path above.

1. Merge the reviewed release change through a pull request and verify exact
   local `main` equals `origin/main`.
2. Create a versioned community tag on that exact commit.
3. Run `./script/verify.sh`, `./script/smoke.sh`, and
   `./script/build_and_run.sh --verify` from the clean tagged checkout.
4. Build and package the app from that exact checkout with:

   ```sh
   ./script/package_community_release.sh \
     --output "$PWD/dist/community" \
     --version 0.2.0 \
     --build 3 \
     --tag v0.2.0-community.1
   ```

5. Verify the ZIP checksum and expanded app signature from a separate temporary
   directory, then publish it only as a GitHub prerelease with the manifest,
   SBOM, third-party notices, project license, project notice, and an explicit
   not-notarized warning.

Community artifacts must not include `appcast.xml`, use the GitHub `latest`
release designation, or claim Gatekeeper, Developer ID, notarization,
clean-machine, or promoted-release status.
