# Contributing to Patchwright

Patchwright welcomes focused bug fixes, tests, documentation improvements, and
well-scoped feature proposals. Before starting a substantial change, open an
issue so the intended behavior and boundaries can be agreed before work begins.

## Development workflow

1. Build and run the checks documented in the [README](README.md#build-and-verify).
2. Add focused tests for behavior changes and reproduce fixes with a failing
   test first.
3. Keep commits scoped and do not include credentials, private repository data,
   generated release artifacts, or unrelated changes.
4. Open a pull request describing the problem, the chosen behavior, and the
   verification performed.

Contributions must preserve Patchwright's fail-closed approval boundaries.
Remote GitHub mutations, process execution, credential handling, and release
changes require tests covering the relevant denial and failure paths.

## Developer Certificate of Origin

Patchwright uses the [Developer Certificate of Origin 1.1](https://developercertificate.org/)
instead of a separate contributor license agreement. Every commit must include
a sign-off certifying the Developer Certificate of Origin:

```text
Signed-off-by: Your Name <you@example.com>
```

Create the sign-off with `git commit -s`. By signing off, you certify that you
have the right to submit the contribution under this repository's open-source
license and that the contribution record may be public and retained
indefinitely. Pull requests containing unsigned commits may be asked to add
sign-offs before merge.

## License

Unless you explicitly state otherwise, contributions intentionally submitted
for inclusion in Patchwright are licensed under the repository's
Apache License 2.0 terms, without additional terms or conditions.

Community participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
Security vulnerabilities must follow the private process in
[SECURITY.md](SECURITY.md), not a public issue.
