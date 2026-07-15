# App Store Checklist

## Repository and build

- [x] Version `0.1.0` and build `1` are present in `Packaging/Info.plist`.
- [x] Bundle identifier is `ai.patchwright.app`.
- [x] Minimum system version is macOS 26.0.
- [x] Clean release assembly records `dirty=false` and `candidate=true`.
- [x] Swift and Rust release builds pass.
- [ ] Create an Xcode macOS app project/target suitable for Archive and App
  Store Connect upload.
- [ ] Configure Apple Distribution signing and a Mac App Store provisioning
  profile for `ai.patchwright.app`.
- [ ] Enable App Sandbox and define the smallest viable entitlement set.
- [ ] Prove user-selected repository access, helper execution, networking,
  Git/Codex integration, and persistence inside the sandbox.
- [ ] Produce and validate an App Store archive/export.

The current SwiftPM/direct-bundle route is not an App Store upload pipeline.
Apple documents Xcode, Transporter, `altool`, and the App Store Connect API as
supported upload paths: [Upload builds](https://developer.apple.com/help/app-store-connect/manage-builds/upload-builds/).

## Product and review

- [ ] Obtain an owner-approved app icon and add the required icon assets.
- [ ] Capture owner-approved Mac screenshots at accepted sizes.
- [ ] Finalize name, subtitle, description, keywords, category, copyright,
  support URL, marketing URL, and privacy-policy URL.
- [ ] Create the App Store Connect app record and confirm the bundle ID/SKU.
- [ ] Complete the age-rating questionnaire.
- [ ] Complete App Privacy answers from `PRIVACY_DATA_MAP.md` after owner/legal
  review.
- [ ] Decide DSA trader status and provide any required verification material.
- [ ] Decide pricing, territories, availability, and release mode.
- [ ] Provide App Review notes and any required review account or fixture.
- [ ] Confirm whether invoking external Codex/Git tools and executing generated
  development commands is acceptable under Guideline 2.5.2; redesign or seek
  Apple clarification if needed.

Current official references:

- [Submitting apps to the App Store](https://developer.apple.com/app-store/submitting/)
- [App information reference](https://developer.apple.com/help/app-store-connect/reference/app-information/app-information)
- [Set an app age rating](https://developer.apple.com/help/app-store-connect/manage-app-information/set-an-app-age-rating/)
- [EU Digital Services Act trader requirements](https://developer.apple.com/help/app-store-connect/manage-compliance-information/manage-european-union-digital-services-act-trader-requirements/)
- [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)

## Security and release operations

- [x] Dependency audit found no Rust advisories and no external SwiftPM dependencies.
- [x] Dirty assemblies are rejected before the production signing boundary.
- [x] Git clone/push credential use is bound to the approved repository origin.
- [x] Review submissions carry the approved pull-request head SHA.
- [x] Production smoke deny-list comparison is case-insensitive.
- [ ] Add server-side client/operator authentication for protected Unix-socket RPC methods.
- [ ] Resolve or explicitly accept the seven remaining low-severity scan findings.
- [ ] Complete the mandatory security write-up phase and canonical scan projection.
- [ ] Complete Developer ID signing, notarization, stapling, Gatekeeper, and clean-machine proof for the direct-distribution lane.
- [ ] Add CI that runs `./script/verify.sh` from a clean checkout.
