# Changelog

All notable changes will be documented here. The project follows Semantic
Versioning once compatibility commitments are published; development versions
before 1.0 may change with explicit migration notes.

## [Unreleased]

### Added

- Initial behavioral compiler, typed Effect IR, adapters, observer/runtime
  boundaries, equivalent rewrite and modernization flows, differential
  verification, and strict exporters.
- Reproducible mise toolchain and complete fast, contract, platform,
  differential, security, packaging, and repository-policy gates.
- GitHub repository governance, supply-chain workflows, security reporting,
  immutable Action references, and canonical Ruleset configuration.
- Git-aware scanning that respects ignored generated artifacts, filters
  structured metadata noise, and distinguishes language attributes and
  non-shell shebangs from shell files.
- Conservative embedded-shell inventory contracts across major JVM, .NET,
  native, BEAM, and scripting-language families, including dynamic-expression
  fallback and source-patching safety refusal.
- A digest-pinned official `cwltool` validation and checksum-locked,
  mise-managed Dagger v0.21.8 execution gate for representative generated
  artifacts.
- A schema-backed, non-executing corpus auditor that records its repository
  selection, verifies post-scan content hashes, analyzes isolated copies, and
  groups inventory and residual evidence deterministically.

### Changed

- Expanded strict POSIX lowering with immutable assignment dataflow and
  homogeneous `||` semantics, strict `pipefail` handling, command-local
  environments, script-relative/static-subshell working directories, and
  expansion-safe literal heredoc writes while preserving mixed or unsupported
  forms as residual capsules.
- Expanded PowerShell, fish, and cmd literal frontends with static
  multi-statement files, immutable PowerShell string/environment dataflow,
  help/header handling, quote-aware boundaries, and honest cmd echo semantics.
- Made canonical template escaping, environment/secret declaration, nested-task
  inheritance, and input rejection explicit in the internal runner.
- Inventoried multiple embedded-shell callsites on one line with stable columns,
  stronger JavaScript provenance checks, and fewer member-method false
  positives.

### Fixed

- Kept opaque residual source byte-for-byte outside canonical IR template
  expansion.
- Rejected unsupported parameter operators and oversized positional references
  deterministically instead of mis-expanding or raising an internal exception.
- Preserved the source line when parsing new line-and-column host callsite
  locators in downstream discovery logic.
- Prevented assignments inside POSIX control flow and heredoc data lines from
  being hoisted as unconditional immutable bindings.
- Preserved PowerShell doubled single quotes and normal `.ps1` completion status,
  and rejected native expression/escape syntax that the literal subsets cannot
  model soundly.
- Bounded every official-exporter subprocess and terminate its process tree on
  timeout instead of allowing a stalled Docker or Dagger engine to hang the
  validation gate indefinitely.

[Unreleased]: https://github.com/P4suta/de-shell/commits/main
