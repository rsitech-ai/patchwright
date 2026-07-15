# Release Blockers

## P0 — App Store architecture

1. No Xcode project/workspace or Mac App Store archive/export path.
2. App Sandbox is disabled (all entitlements files are empty).
3. No Apple Distribution/Mac App Store provisioning profile for
   `ai.patchwright.app`.
4. The app executes bundled helpers and integrates with external Git/Codex
   tooling; sandbox feasibility and Guideline 2.5.2 compliance are unproven.

Apple's App Sandbox documentation is the implementation reference:
[App Sandbox](https://developer.apple.com/documentation/security/app-sandbox).

## P0 — Submission and product assets

1. No App Store Connect app record or uploaded build.
2. No owner-approved app icon.
3. No App Store screenshots or finalized localized metadata.
4. No support URL or privacy-policy URL confirmed.
5. Age rating, App Privacy answers, export/compliance answers, DSA trader
   status, pricing, territories, and release mode are undecided.

## P1 — Security and runtime

1. Mandatory security write-up agents were blocked by platform policy, so the
   canonical scan is incomplete.
2. Two medium same-user RPC authorization findings and seven low findings
   remain open.
3. Fixed findings require a rescan against commit `53aa639` or later.
4. Runtime logs contain an AppKit reentrant table-delegate warning that should
   be triaged on Xcode 26/macOS 26 and macOS 27.
5. Minimum macOS 26 compatibility has not been tested on a macOS 26 machine.

## P1 — Signing and distribution

1. Developer ID signing did not complete because the Keychain operation
   stalled; the attempted artifact is rejected.
2. Notary profile `Patchwright` is absent.
3. No notarized/stapled DMG, Gatekeeper proof, or clean-machine proof exists.

## Manual owner actions

- Approve or supply the icon and screenshot direction.
- Create/confirm App Store Connect identifiers and legal/commercial metadata.
- Decide the App Store versus direct-distribution architecture after sandbox
  and 2.5.2 feasibility review.
- Unlock/authorize the Developer ID identity and create the notarytool Keychain
  profile if the direct-distribution lane continues.
- Provide final authorization before Submit for Review, public release, merge,
  or tagging. None of those actions were performed.
