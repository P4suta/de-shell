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

## Rust design policy

The Rust implementation is deliberately stricter than idiomatic defaults. A
review preference is not a policy: every rule below is enforced by rustc,
Clippy, `clippy.toml`, or `cargo xtask rust-policy`, and `mise run lint` runs all
of them.

- Trait objects are forbidden, including `Box<dyn Trait>`, borrowed trait
  objects, aliases, and `dyn` hidden in macro input. Use a generic when the set
  of implementations is open and an exhaustive enum when it is closed. `Box`
  remains appropriate solely to give recursive data a finite size.
- Production code does not use `unwrap`, `expect`, `panic!`, `unreachable!`,
  `todo!`, or `unimplemented!`. A fallible boundary returns a typed error; a
  state claimed to be impossible is represented so that the compiler checks
  it. Tests may panic because that is their assertion mechanism.
- Enum matches name every variant. A wildcard arm makes a future variant
  inherit behavior without review, so `wildcard_enum_match_arm` is denied.
- Potentially truncating, wrapping, sign-losing, or precision-losing numeric
  casts are denied. Use `From` for infallible conversions and `TryFrom` with an
  explicit failure or saturation policy otherwise.
- Ignored `Result` and other `must_use` values are named. A best-effort
  boundary uses a purpose-specific name such as `_stdout_delivery`; anonymous
  discards and `map_err(|_| ...)` are denied.
- Nested `Option`, boolean bags, accidental double allocation, boxed
  collections, boxed vector elements, and implicit or redundant clones are
  denied. Model states with enums and make ownership changes visible.
- Unsafe operations stay at the smallest OS boundary, one per block, with a
  `SAFETY` invariant. Unsafe operations inside an unsafe function are still
  forbidden unless placed in such a block.
- Filesystem mutation goes through `patch`, ambient time and environment reads
  go through `host`, and process launches go through `host`. The raw APIs are
  mechanically unavailable everywhere else.
- `#[allow]` is forbidden. A genuinely necessary exception uses the narrowest
  `#[expect]` on the affected item with a reason that states the invariant; an
  obsolete expectation then becomes a warning and fails CI.

The complete Clippy `restriction` group is intentionally not enabled as a
single switch. It contains mutually exclusive style rules and rules that are
wrong for this security model—for example, replacing exclusive `create_dir`
with recursive creation, or treating every non-directory filesystem object as
a regular file. Rules are adopted individually only when their required
rewrite preserves the contract on every supported platform.

Test code has only the boundaries needed to test failure: it may unwrap,
expect, panic, index fixtures, build large stack fixtures, and call raw
filesystem APIs to construct races and corrupt shapes. Those exemptions are in
`clippy.toml` or a reasoned test-module `expect`; they do not apply to product
code.

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
