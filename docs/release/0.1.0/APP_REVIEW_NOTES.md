# Draft App Review Notes

These notes are a draft. Do not paste them into App Store Connect until the
sandboxed App Store build and review fixture exist.

Patchwright is a local-first macOS developer tool for inspecting authorized
GitHub repositories, converting issues and pull requests into local tasks,
reviewing proposed changes, and executing explicitly approved repository
operations. It does not merge automatically. Remote writes require an exact
preview, a short-lived matching approval, and a separate Execute action.

The app needs a user-authorized GitHub account or GitHub App installation to
show real repository data. For review, provide a dedicated disposable GitHub
organization/repository and non-production credentials with the minimum
permissions required by the submitted build. Include exact setup steps and
sample tasks. Do not provide production credentials.

Codex functionality requires the configured Codex CLI/account. If review must
exercise that path, provide a dedicated review account or a deterministic local
fixture accepted by App Review. Clearly distinguish unavailable Foundation
Models/Codex states from product failures.

Before submission, add:

- sandbox file-selection steps and expected permission prompts;
- the exact review account/fixture;
- a walkthrough of read-only browsing, task creation, approval, execution,
  cancellation, and recovery;
- a plain-language explanation of external tools/processes and why they comply
  with App Review Guideline 2.5.2;
- support contact and any non-obvious hardware/OS requirements.
