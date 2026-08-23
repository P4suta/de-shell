# Security policy

## Supported versions

Before 1.0, security fixes are made on `main` and on the latest tagged
pre-release when one exists. Older development snapshots are not maintained.
The project will publish a versioned support table before its first stable
release.

## Reporting a vulnerability

Do not open a public issue, discussion, pull request, or test fixture containing
an exploitable vulnerability or secret. Use GitHub's
[private vulnerability reporting](https://github.com/P4suta/de-shell/security/advisories/new)
instead. Include:

- affected commit or version and platform;
- the smallest safe reproduction and the relevant shell/interpreter;
- expected impact, required capabilities, and known mitigations;
- whether the report or proof contains credentials or private source.

The maintainer aims to acknowledge a complete report within seven days, provide
a status update at least every fourteen days, and coordinate disclosure after a
fix is available. A 90-day disclosure target is preferred, but severity,
downstream coordination, and reporter safety may require a different schedule.

Security-sensitive fixes receive a regression test before implementation where
the test can be published safely. Reporters may request credit or anonymity.

## Security boundaries

The local `deshell run` backend starts host processes and is not an OS sandbox.
Untrusted automation must use the disposable observer path described in the
README. A result marked `residual` is executable coverage, not a formal proof of
unobserved behavior. See the documented guarantee level and residual reason
before relying on a migration in a security boundary.
