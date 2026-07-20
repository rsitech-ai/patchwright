# Security policy

Patchwright is beta software that can run local processes, inspect repositories,
and perform explicitly approved GitHub mutations. Treat it as privileged
developer tooling and review action previews before approving them.

## Report a vulnerability privately

Do not open a public issue for a suspected vulnerability. Use GitHub's
[private vulnerability reporting form](https://github.com/rsitech-ai/patchwright/security/advisories/new).
If that form is unavailable, email the RSI Tech maintainers privately at
[info@rsitech.ai](mailto:info@rsitech.ai).

Include the affected version or commit, impact, reproduction steps, and any
suggested mitigation. Remove tokens, private source, personal data, and other
secrets from reports and attachments.

Maintainers review reports as capacity allows. This project does not promise a
response time, resolution time, disclosure date, bounty, or support service.
Please avoid public disclosure until maintainers have had a reasonable
opportunity to investigate, but do not treat this request as a restriction on
your legal rights.

## Supported versions

Security fixes are considered on a best-effort basis for the latest published
release and the current default branch. Older releases are not supported.

Developer ID signing and Apple notarization identify official macOS artifacts;
they do not guarantee that the software is free of vulnerabilities. Verify
release checksums and obtain downloads only from the repository's official
GitHub Releases page.
