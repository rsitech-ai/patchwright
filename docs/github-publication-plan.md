# GitHub publication plan

This file records proposed external repository and profile changes. It does not
authorize or apply them. Export the current settings immediately before any
change and apply the items only after explicit approval.

## Repository identity

- Description: `Local-first macOS control plane for auditable GitHub engineering workflows, approval-gated mutations, and embedded Codex sessions.`
- Homepage: leave unset until a maintained public documentation site exists.
- Topics: `macos`, `swiftui`, `rust`, `github`, `developer-tools`,
  `code-review`, `local-first`, `automation`.
- Default branch: keep `main`.
- Issues: keep enabled. Discussions, Wiki, Projects, and Pages: keep disabled
  until a maintainer commits to operating them.
- Merge policy: keep merge, squash, and rebase available initially; keep
  automatic deletion of merged branches enabled.

## Default-branch ruleset

Apply only after the readiness pull request proves the CI check name is stable.

- Require a pull request before merging.
- Require the `verify` CI job and require the branch to be current before merge.
- Require conversation resolution.
- Block force pushes and branch deletion.
- Do not require signed commits initially; Patchwright already requires DCO
  sign-off and mandatory signatures would add contributor friction.
- Keep bypass actors explicit and minimal. Record any owner emergency bypass in
  the repository settings export.

Protect release tags matching `v*` from deletion or update after the first
ruleset-backed release.

## Security and Actions settings

- Enable the dependency graph, vulnerability alerts, and Dependabot security
  updates.
- Enable secret scanning and push protection if GitHub offers them for the
  repository.
- Enable private vulnerability reporting and verify that the link in
  `SECURITY.md` works while signed out.
- Keep the default Actions token read-only. Do not allow fork workflows to
  receive secrets or approve pull requests.
- Retain the repository-local CI secret scan and `cargo audit`; consider CodeQL
  default setup only after confirming its Swift build is stable on this
  repository.

## Labels

Keep a small taxonomy: `bug`, `enhancement`, `documentation`, `security`,
`dependencies`, `good first issue`, `help wanted`, `breaking change`, and
`needs reproduction`. Do not delete existing labels without reviewing open
issues and pull requests that use them.

## Personal profile

After release approval, pin `s1korrrr/patchwright`. A profile README entry can
describe Patchwright as a local-first macOS engineering control plane and link
to the repository and latest verified release. Do not change the account bio,
location, company, email visibility, social links, or other pinned repositories
without a separate review of the exact profile patch.

## Post-change verification

Inspect the repository signed out. Confirm the README, licenses, security form,
issue forms, pull-request template, release assets, checksums, topics,
description, rulesets, Actions permissions, and security settings. Recheck
rulesets and security features after any visibility change.
