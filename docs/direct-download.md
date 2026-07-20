# Direct download and installation

Patchwright is distributed outside any app marketplace. Official binaries are
published only through [GitHub Releases](https://github.com/rsitech-ai/patchwright/releases).
Each release must be signed with an Apple **Developer ID Application** identity,
accepted by **Apple notarization**, and stapled before publication.

## Install

1. Download `Patchwright-<version>.dmg` and its matching `.sha256` file from the
   same GitHub release.
2. Verify the digest from Terminal:

   ```sh
   shasum -a 256 -c Patchwright-<version>.dmg.sha256
   ```

3. Open the DMG, drag Patchwright to Applications, and launch it normally.

Do not bypass Gatekeeper with `xattr`, ad-hoc re-signing, or a right-click
workaround. If macOS reports an unidentified or damaged app, delete that copy
and report the release URL and macOS version through the project support or
security channel.

## Verify the publisher and ticket

Advanced users can verify the downloaded DMG before installation:

```sh
codesign --verify --deep --strict --verbose=2 Patchwright-<version>.dmg
xcrun stapler validate Patchwright-<version>.dmg
spctl --assess --type open --context context:primary-signature --verbose=4 Patchwright-<version>.dmg
```

The release page also publishes `appcast.xml` for signed Sparkle updates. The
feed and archive signatures are checked by Patchwright before extraction.

## Community prerelease

The repository may publish a ZIP labeled `community` for source review and
evaluation when Developer ID or notarization credentials are unavailable. A
community prerelease is ad-hoc signed and is **not Developer ID signed or Apple notarized**.
It is not included in the signed Sparkle update feed and is not an
official trusted binary distribution.

Download the ZIP, its `.sha256` file, and its JSON manifest into the same
directory, then verify the archive before expanding it:

```sh
shasum -a 256 -c Patchwright-<version>-community.<n>-macos-<architecture>.zip.sha256
```

The manifest binds the archive digest to the exact Git commit and tag and
states the signing and notarization status. Gatekeeper may block an unnotarized
archive. Do not disable Gatekeeper or strip quarantine metadata to work around
that warning; build the tagged source locally if a community prerelease does
not meet your trust requirements.

## Source builds

Source builds are supported for development and review, but they are not
official notarized downloads. Follow the commands in the repository README and
keep locally built apps separate from release verification.
