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

The local `deshell run --backend local` backend starts host processes and is not
an OS sandbox. It requires both a project opt-in and a CLI opt-in. Untrusted
automation must use the default disposable path described in the README; a
missing or invalid provider never falls back to local execution.

`native` means the node was lowered to the pinned semantic model. `delegated`
means exact source bytes and a lock-matched interpreter identity execute only
inside a digest-pinned disposable runtime. `residual` means unrecognized,
non-executable source.
Scenario observation is evidence for that scenario/provider/runtime digest
only, not a proof of behavior for other inputs. Host materialization and host
execution remain outside the default trust boundary.

The repository build directly executes only supervised rootless OCI providers.
Windows and macOS launch contracts remain unavailable until their signed helper
transport is connected; provider discovery alone is never reported as execution
readiness.
