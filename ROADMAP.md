# Roadmap

The first public release is the Rust `deshell` 0.1.0 CLI. Work is accepted in
Red, Green, Refactor increments, and a checked box means the repository contains
the implementation and deterministic test—not that an external release or
hardware gate has already run.

## 0.1.0 contract and compiler — implemented

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

- [ ] Run every fast, contract, platform, differential, security, package,
  official-exporter, and workflow gate from `v0.1.0-rc.1`, including the
  required three-operating-system matrix.
- [ ] Raise measured line coverage from the checked-in 74% regression floor to
  at least 90% overall and at least 90% in scanner, frontend, runner, protocol,
  lab, and patch before treating coverage as a 0.1.0 release gate.
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
