# Test Evidence

## Environment

- Date: 2026-07-15
- Host: Apple silicon, macOS 27.0 beta (`26A5378j`)
- Xcode: 26.6 (`17F113`)
- macOS SDK: 26.5
- Swift: 6.3.3
- Rust/Cargo: 1.91.0
- Codex CLI: 0.144.2, signed in
- Release commit tested: `53aa639`

The host is newer than the declared macOS 26 minimum, so this is launch proof,
not minimum-OS compatibility proof.

## Automated verification

`./script/verify.sh` passed after the hardening changes.

- Rust: 104 unique tests passed; 1 real-Codex test intentionally ignored
  because it consumes a live signed-in model session.
- Swift: 30 tests passed.
- Packaging: release contract and GitHub App smoke safety contract passed.
- `cargo check`, formatting/clippy gates, debug builds, focused integration
  suites, doc tests, and Swift production build passed.
- Zero test failures.

Focused regression coverage added or strengthened:

- dirty/non-candidate assembly rejection before signing;
- mixed-case production repository deny-list enforcement;
- managed-clone URL binding;
- push-origin binding before installation-token use;
- exact review `commit_id` propagation;
- relay health success and bounded timeout behavior;
- accessibility labels for indeterminate progress and delivery message input.

## Assembly evidence

Clean unsigned candidate:

`/Users/s1kor/.patchwright/release-work/Patchwright-0.1.0-1.gTCdAd/Patchwright.app`

Assembly record:

`/Users/s1kor/.patchwright/release-work/Patchwright-0.1.0-1.gTCdAd/evidence/assembly.json`

The record states version `0.1.0`, build `1`, `dirty=false`, and
`candidate=true`.

## Runtime evidence

The clean unsigned candidate was launched with Launch Services. Process 33875
ran the expected executable, connected to the local engine over its Unix
socket, rendered the repository and pull-request workspace, remained alive
during inspection, and shut down cleanly on SIGTERM.

Inspection screenshot:

`/var/folders/g6/mrhqfgk15_d2gjj52991r1jr0000gn/T/codex-shot-2026-07-15_15-04-35.png`

Observed host-log qualifications:

- local Unix-socket `TCP_INFO` queries emitted unsupported-operation noise;
- AppKit emitted one future-facing reentrant `NSTableView` delegate warning;
- CoreSpotlight donation emitted a non-fatal system-service error.

These did not crash or block the inspected workflow, but the AppKit warning
should be investigated before a release-candidate label.

## Signing attempt

Developer ID signing was attempted on an earlier clean assembly. The Keychain
operation did not complete after the first helper-signing command, and the
artifact failed strict verification. That artifact is rejected and must not be
distributed. A fresh unsigned assembly was produced for runtime proof.

No notarization, stapling, DMG distribution, App Store upload, or external
GitHub mutation was performed.
