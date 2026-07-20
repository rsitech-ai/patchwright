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
8. After publication, follow the signed-out and settings verification in the
   [GitHub publication plan](github-publication-plan.md) and test installation
   using the public DMG and checksum, not local build output.

Do not publish an artifact assembled from a dirty tree, a different commit, or
evidence copied from another candidate. Do not bypass Gatekeeper, notarization,
the clean-machine gate, or independent promotion evidence.
