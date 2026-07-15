# Patchwright 0.1.0 Release Status

## Verdict

**BLOCKED**

Patchwright 0.1.0 is repo-ready for continued release work and has a clean,
unsigned direct-distribution candidate assembly. It is not ready for TestFlight
or App Store Connect submission.

## Current readiness

| Gate | Status | Evidence |
| --- | --- | --- |
| Source and tests | Pass | Commit `2918423`; `./script/verify.sh` passed |
| Clean assembly | Pass | `dirty=false`, `candidate=true` in assembly evidence |
| Runtime launch | Pass with qualification | Unsigned candidate launched on macOS 27 beta; local engine connected and app shut down cleanly |
| Developer ID signing | Blocked | Keychain-backed `codesign` did not complete; the attempted artifact is not usable |
| Notarization | Blocked | Keychain profile `Patchwright` is absent |
| Mac App Store archive/export | Blocked | Repository has no Xcode project/workspace, Mac App Store signing configuration, sandbox entitlement, or provisioning profile |
| Store assets and metadata | Blocked | App icon, screenshots, App Store Connect record, localized metadata, support/privacy URLs, age rating, and privacy answers are not complete |
| Security scan | Blocked | Discovery/validation/attack-path work completed, but platform policy blocked the mandatory dedicated write-up phase and therefore canonical scan completion |
| Owner/legal decisions | Blocked | DSA trader status, final privacy declarations, pricing/availability, and submission authorization remain owner decisions |

## Distribution labels

- `repo-ready`: **yes** for this branch.
- `package-ready`: **no**; no fully Developer ID-signed and verified artifact.
- `release-candidate ready`: **no**; notarization, Gatekeeper, clean-machine,
  and production integration gates are incomplete.
- `ready for App Store Connect upload`: **no**.
- `blocked:external`: Apple signing/notarization credentials, App Store Connect
  configuration, owner-approved icon/metadata, and legal/commercial decisions.

Apple requires an App Store Connect app record before upload, and builds must
be uploaded with an Apple-supported tool. See [Create an app record and upload
builds](https://developer.apple.com/help/app-store-connect/manage-builds/upload-builds/).
Mac App Store apps must meet the sandbox requirement in App Review Guideline
2.4.5. See [App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/).
