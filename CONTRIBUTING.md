# Contributing to de-shell

All phases use outside-in TDD, including contracts, runtime boundaries,
automation, packaging, and documentation.

1. **Red:** add the smallest failing public contract, golden case, or minimized
   bug reproduction and run it to confirm the intended failure.
2. **Green:** implement only enough behavior to satisfy that test.
3. **Refactor:** improve the design while the focused test and every affected
   gate remain green.
4. Add negative, boundary, and failure-path coverage before calling the change
   complete.

Do not update golden output implicitly. A changed snapshot needs an explicit,
reviewable reason. Make unstable behavior deterministic with an injected
filesystem, process backend, clock, adapter, observer, fixture, or replay tape;
never skip a flaky test.

## Local workflow

```console
mise install
mise run setup
mise run lint
cargo test --locked --workspace
mise run test:contract
mise run fmt:check
mise run package
```

Use the narrowest relevant test during Red/Green, then run the complete suite
before handoff. Bug fixes begin with a minimized regression. Rewrites require
positive, negative, idempotence, source-span, transactional-failure, and
behavioral-equivalence coverage. Protocol changes require version, malformed
message, duplicate-key, unknown-field, disconnect, timeout, size-limit, and ID
mismatch cases.

Rust 1.98 and all supporting tools are pinned through mise. Keep Cargo commands
locked. The public package contains one `deshell` binary; do not expose Rust
library APIs, standalone agent executables, legacy shims, or OCaml install
artifacts.

The OCaml tree is an unpublished reference implementation. Work on it is
explicit through `mise run reference:build` and `mise run reference:test`; it
must not become a dependency of the Rust CLI, CI default, or release archives.

## Pull requests and repository policy

Pull requests use the repository template and must pass `Required gate`.
Reviews are dismissed after new commits; code-owner review, last-push approval,
resolved threads, signed commits, and linear history are enforced on the
default branch.

Repository settings and Rulesets live under `.github/settings` and
`.github/rulesets`. Maintainers can reconcile them with:

```console
mise run github:apply
mise run github:verify
```

The apply task mutates remote GitHub settings and requires an authenticated
administrator. The required CI gate separately enforces the 10 MiB tracked-file
and 240-character path limits.

Publishing is irreversible and requires release-environment owner approval.
Release candidates must pass the three-OS gates, six-archive smoke tests,
signature/provenance verification, and declared corpus comparison before the
final tag is published.
