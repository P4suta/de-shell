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

### Changed

- Expanded strict POSIX lowering with immutable assignment dataflow and
  homogeneous `||` semantics while preserving mixed or unsupported forms as
  residual capsules.
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

[Unreleased]: https://github.com/P4suta/de-shell/commits/main
