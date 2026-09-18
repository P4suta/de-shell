# Roadmap

The first public release is the Rust `deshell` 0.1.0 CLI. Work is accepted in
Red, Green, Refactor increments, and a checked box means the repository contains
the implementation and deterministic test—not that an external release or
hardware gate has already run.

## 0.1.0 migration oracle — implementation in progress

Version 0.1.0 must not be published until representative end-to-end retirement
passes for all seven interpreters (POSIX/Bash, zsh, fish, PowerShell, cmd, and
Nushell), with both official Rust and Go generators where applicable.

- [x] Repository-wide audit, scenario draft, migration plan/verify/evidence
  import/apply/status, harden, and shell-free gate command surfaces.
- [x] Strict v1 Generator Protocol, Migration Request, Proposal, Migration Plan,
  Migration Evidence, Archive Manifest, Audit Finding, and harden contracts.
- [x] Content-addressed plans, full-cell Evidence keys, repeat comparison,
  coverage checks, staged shell-free scan, atomic retirement, rollback, and
  archive/Evidence integrity verification.
- [x] Official Rust, Go, Docker/Python/JavaScript/GitHub-host generators and a
  digest-pinned isolated external-generator bridge.
- [x] Complete project-native Make/package/task interface rewrites and resolve
  every supported static call site in the proposal rather than blocking it.
- [x] Add enforced network record/replay observations and the complete
  seven-interpreter, two-generator E2E corpus.

## Foundational contract and compiler substrate — implemented

- [x] Language-neutral Inventory v1, manifest, Effect IR v1, Evidence v1,
  diagnostic, project, scenario, lock, replay, audit, and JSON-RPC contracts
  under `contracts/`.
- [x] Explicit text expressions and the restricted value model, with duplicate
  name rejection and normalized project-relative paths.
- [x] Domain-separated deterministic node IDs and Unicode-scalar source
  coordinates over half-open byte spans.
- [x] Strict JSON decoding, deterministic pretty persistence, canonical digest
  bytes, and no pre-v1 migration surface.
- [x] Conservative POSIX, zsh, fish, PowerShell, cmd, and Nushell frontends with
  lossless pinned delegation; unknown source is a non-executable residual.
- [x] Byte-safe scanner, content-addressed lifecycle, transactional
  rewrite/modernize, strict export, replay, supervised concurrent pipelines,
  disposable-lab launch contracts, and Evidence-only differential observation.
- [x] Single Rust multicall executable with hidden process, observer, and
  Nushell adapter modes.
- [x] Fixed exit categories and stderr-only human/JSONL diagnostics.
- [x] Shared frontend golden corpus, CLI cases, schema byte checks, agent
  handshakes, unit/property/security tests, independent JSON Schema validation
  of generated documents, and a private conformance runner.

## Rust default and distribution — implemented in repository

- [x] Rust 1.98 / edition 2024 root package and private `xtask` workspace member.
- [x] Rust-first `mise run deshell`, build, test, lint, conformance, and package
  tasks; OCaml commands are explicit `reference:*` tasks.
- [x] Linux, macOS, and Windows CI for the Rust test, conformance, and package
  gates.
- [x] A tag workflow defining six release archives across Linux musl, macOS,
  and Windows on x86_64 and Arm64.
- [x] SHA-256 manifests, keyless signature bundle, GitHub build provenance, and
  protected final-tag crates.io publication steps.
- [x] Embedded v1 schemas and PowerShell adapter in the Cargo package payload.

## 0.1.0 release evidence — must pass before publication

- [ ] Pass `scenario -> plan -> matrix verify -> evidence import -> apply ->
  scan zero` for shell files and embedded sources across the seven interpreters,
  including input, environment, branch, failure, and parser-blocker cases.
- [ ] Run every fast, contract, platform, differential, security, package,
  official-exporter, and workflow gate from `v0.1.0-rc.1`, including the
  required three-operating-system matrix.
- [x] Enforce measured line coverage at 90% overall and at least 90% in scanner,
  frontend, runner, protocol, lab, and patch as a 0.1.0 release gate.
- [ ] Run the fixed 2026-08-25 48-repository audit selection through both
  deterministic implementations and record zero scanner errors/skips,
  unclassified files, residual executable coverage, nondeterminism, or
  unexplained differences in inventory, IR, diagnostics, patches, and exports.
- [ ] Pass the self-hosted rootless-Linux, Windows Sandbox/Hyper-V, and signed
  macOS Virtualization.framework execution gates with no local fallback.
- [ ] Add and pass saved-corpus/PR fuzz smoke, nightly scanner/parser/protocol/
  schema fuzzing, Miri, ASan/UBSan, and validator/policy mutation thresholds.
- [ ] Record scan, simple-run, and release-size baselines on the release runner
  and require review for regressions greater than 20 percent.
- [ ] Install and smoke all six archives, including `--version`, every embedded
  schema, and all three internal-agent handshakes.
- [ ] Verify the CycloneDX SBOM, SHA-256 checksums, Sigstore bundle, and
  provenance for every archive.
- [ ] Confirm crates.io package-name ownership before the irreversible publish
  operation. If `deshell` is unavailable, publish package `deshell-cli` while
  retaining binary name `deshell`.
- [ ] Obtain the release-environment owner approval and publish `v0.1.0`.

## Blocking the claim that de-shell retires real shell — found by dogfooding

Running `deshell scan` and `migrate plan` against OComment's `action.yml`
retired nothing: all five `run:` steps came back as whole-file delegation, each
for the same reason.

```
blocker DESHELL_BLOCKER_UNIMPLEMENTED_SEMANTIC action.yml@3963..6698:
  shell builtin set requires pinned interpreter delegation
```

- [ ] Split `set` by option instead of refusing the builtin by name. Every shell
  builtin is delegated today, `set` among them, and `set -euo pipefail` opens
  approximately every CI step that exists. The same corpus with `set` removed
  lowers 48% of its bytes natively, so this is a granularity problem rather than
  a capability one. Start with `-o pipefail`: it decides a pipeline's exit
  status, which is local and static. `-f` (field splitting) and `-x` (tracing)
  stay delegated.
- [ ] Do not treat `-e` as the easy one. Measured against bash 3.2.57, `set -e`
  stops on a command that is *not tested*, where tested means the left of
  `&&`/`||`, the condition of `if`/`while`/`until`, the operand of `!`, and every
  element of a pipeline but the last. It is also a property of the call site
  rather than of the code: a function body whose `false` aborts when called
  directly runs to completion when called as `f || true`. Lowering it to `?` per
  statement is observably wrong, and `set +e` … `set -e` pairs are how callers
  express "a non-zero exit is not a failure here".
- [ ] Pin the bash version in `deshell.lock` the way `nu` already is. macOS ships
  3.2 and Linux runners ship 5.x; the CI matrix spans both. A tool that claims
  equivalence has to say which interpreter it is equivalent to, and the `set -e`
  rules above were only measured on 3.2.
- [ ] Separate "no decision" from `delegated`. A node delegated because `set` is
  unmodelled is a decision: the source was read and isolation was chosen. A node
  delegated because the parser timed out is the absence of one, and it is
  retryable where the first is not. Both currently surface as `delegated` with a
  blocker, so a plan cannot be read to tell them apart.
- [ ] Model `case`, redirection, and `2>/dev/null`. These blocked three of six
  steps in a corpus with no `set` in it, and unlike the above they are missing
  implementation rather than unsettled semantics.
- [ ] Carry the thirteen `set` semantics cases into the golden corpus. The corpus
  contained no `set` at all, which is why none of this surfaced until the tool
  was pointed at a repository that was not its own.

## After 0.1.0

- Keep the unpublished OCaml reference aligned for deterministic IR, analysis,
  transformation, and export checks; do not add OCaml runtime or distribution
  work.
- Expand native parser coverage only through minimized corpus reproductions.
  Recognized behavior remains explicitly delegated until its semantics are
  proven; unknown residual source remains non-executable.
- Extend disposable-lab and physical OS integration in Rust.
- Defer a public Rust SDK, GUI, parser replacement, OS-specific installers, and
  the complete 1.0 21-cell hardware certification until their own versioned
  contracts exist.
