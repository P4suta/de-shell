# Roadmap

The compiler remains executable at every stage through explicit residual
capsules. “Implemented” below means the code contract exists in this repository;
it does not substitute for the external hardware, signing, and corpus evidence
required to call a release 1.0.

## 0.1 Foundation — implemented

- black-box CLI acceptance tests and versioned JSON schemas
- hierarchical typed Effect IR, validation, canonical codec, and schema migration
- content hashing and atomic multi-file create/patch transactions
- JSON-RPC handshake, malformed/disconnect/timeout/size contracts
- internal runner, project-scoped process agent, and strict capability policy
- mise-pinned OCaml, Rust, PowerShell, and lint toolchains
- verified package payload for all bundled executables, adapters, and schemas

## 0.2 POSIX vertical slice — implemented core

- conservative POSIX sh/Bash lowering with precise source maps
- literal commands, pipelines, sequences, `&&`, simple `if`, and static `for`
- equivalent rewrite rules and separate modernization profiles
- command-effect model, bounded concolic scenarios, and differential engine
- disposable rootless OCI specification and observer agent
- strict Dagger export with immutable base-image reference

Broader POSIX grammar coverage is driven by minimized corpus regressions; syntax
outside the proven subset remains an executable residual rather than an unsafe
guess.

## 0.3 Repository and Windows migration — implemented core

- syntax-aware embedded-shell inventory and call graph
- exact-call-only transactional callsite replacement
- official PowerShell AST oracle contract and conservative cmd frontend
- Windows Sandbox hardening, agent mapping, lifecycle, and result decoding
- Hyper-V guest-agent protocol
- Nushell and CWL exporters with strict capability rejection

The signed Hyper-V launcher and official CWL runner certification remain release
artifacts/gates.

## 0.4 Breadth — implemented core

- contract suites for zsh, fish, PowerShell, cmd, and Nushell literal subsets
- pinned official Nushell parser adapter and unknown-interpreter trace fallback
- replay tape codec and network backend, including secret-safe tape recording
- time/random/network exchange types and isolated replay-network launch contracts
- Virtualization.framework guest-agent protocol

Full time/random syscall interception, a signed macOS launcher, and physical
macOS guest execution remain release gates.

## 0.9–1.0 Hardening — release gates

- collect and minimize failures from real automation projects before each fix
- validate generated Dagger and CWL artifacts with their official current tools
- publish a digest-pinned multi-shell lab image and record its digest in locks
- build and sign native binaries/installers for Linux, macOS, and Windows
- supply signed Hyper-V and Virtualization.framework launcher helpers
- demonstrate zero unexplained differences in the declared scenario corpus
- demonstrate at least 95% non-residual semantic-node coverage
- demonstrate 100% executable non-interactive scripts with residuals included
- demonstrate 100% inventory of the declared embedded formats
- pass the complete 7-shell x 3-OS release matrix on appropriate hardware

The `Release_gate` evaluator encodes these quantitative criteria and refuses an
empty corpus or an incomplete matrix. A release is not 1.0 until real evidence,
not placeholder values, makes that evaluator pass.
