# de-shell

[![CI](https://github.com/P4suta/de-shell/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/P4suta/de-shell/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/P4suta/de-shell/badge)](https://securityscorecards.dev/viewer/?uri=github.com/P4suta/de-shell)

`de-shell` is a Rust behavioral compiler for shell automation. It inventories
shell entrypoints and embedded shell, lowers the behavior it can prove into
Effect IR v1, and retains unsupported source as a lossless residual capsule.

The first public product is the single `deshell` executable at version 0.1.0.
Its supported interfaces are the CLI, generated files, the JSON Schemas under
[`contracts/`](contracts/), and JSON-RPC v1. The Rust modules and the
unpublished OCaml reference implementation are private, unstable implementation
details.

## Status

The Rust implementation is the repository default and is exercised on Linux,
macOS, and Windows by CI. The repository is release-candidate ready only after
the checked-in gates pass; it does not claim that a tag, crates.io package, or
the 48-repository audit has already been published or rerun.

Implemented behavior includes:

- Git-aware, bounded-worker inventory of shell files and conservative embedded
  shell candidates in common build and repository formats;
- literal and explicitly modelled subsets of POSIX shell, zsh, fish,
  PowerShell, cmd, and Nushell, plus a policy-controlled unknown-interpreter
  residual;
- typed `text`, `bool`, signed 64-bit `int`, normalized `path`, and `secret<T>`
  values, with explicit `literal`, `variable`, and `argument` text parts;
- deterministic node IDs derived from normalized paths, half-open byte spans,
  operation kinds, and preorder positions;
- static `formal`, `exhaustive`, and `residual` node guarantees, with observed
  comparisons stored separately in Evidence v1;
- strict duplicate-key and unknown-field rejection for persisted JSON,
  RFC 8785-style canonical bytes for digests, and deterministic pretty files;
- transactional equivalent rewrites, opt-in modernization proposals, strict
  exporters, replay-only network access, and project-confined filesystem
  boundaries;
- injectable disposable-lab launch contracts with platform-checked rootless
  OCI, Windows Sandbox, Hyper-V, and Virtualization.framework providers;
- raw-byte process output internally, UTF-8 validation at text capture
  boundaries, bounded execution, and secret-safe diagnostics and traces;
- one multicall executable with hidden process-agent, observer-agent, and
  Nushell-adapter modes using newline-delimited JSON-RPC v1;
- embedded schemas retrievable with `deshell schema NAME`.

Unsupported behavior is never silently discarded. Analysis either rejects it
under project policy or emits an opaque capsule carrying one residual reason,
its interpreter, source span, and the exact UTF-8 or base64 source bytes.

## Toolchain and tests

The public implementation uses Rust 1.98 and edition 2024. `mise.toml` pins the
development tools; Cargo uses a locked dependency graph and a bounded build job
count.

```console
mise trust
mise install
mise run setup
mise run build
mise run test
mise run package
```

The main tasks are:

| Task | Purpose |
| --- | --- |
| `mise run deshell -- ARGS` | Run the Rust CLI |
| `mise run test:fast` | Unit, property, and deterministic regression tests |
| `mise run test:contract` | Shared CLI, schema, golden, and agent conformance |
| `mise run test:adapters` | JSON-RPC adapter and internal-agent contracts |
| `mise run test:platform` | Filesystem, process, disposable-lab, scanner, and workspace boundaries |
| `mise run test:differential` | Observation and Evidence v1 comparisons |
| `mise run test:security` | Traversal, duplicate-key, protocol, replay, and transaction regressions |
| `mise run lint` | Clippy, repository guardrails, and workflow validation |
| `mise run package` | Verify the crates.io payload |
| `mise run reference:test` | Test the unpublished OCaml reference explicitly |

Every implementation phase follows Red, Green, Refactor: first add a failing
public contract or focused reproduction, implement the smallest correct change,
then refactor while the relevant and full gates stay green. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## First project

```console
mise run deshell -- init
mise run deshell -- scan --format json
mise run deshell -- analyze --entry scripts/build.sh
mise run deshell -- check
mise run deshell -- verify
mise run deshell -- rewrite --equivalent --entry scripts/build.sh
mise run deshell -- modernize --profile secure
mise run deshell -- run -- --original-script-argument
mise run deshell -- export --target cwl
mise run deshell -- schema effect-ir
```

Commands that can alter source default to a preview and require `--apply`.
Project files are fresh v1 contracts; 0.1.0 intentionally provides no legacy IR
or lock migration path.

`deshell init` creates:

- `.deshell/project.toml`, containing entrypoints and policy;
- `.deshell/scenarios/default.toml`, containing named inputs and expectations;
- `deshell.lock`, containing protocol and provider pins.

`deshell analyze` then writes `.deshell/plan.json` and
`.deshell/evidence.json`. Name/value arrays reject duplicates, paths are
normalized project-relative paths, and persisted JSON rejects unknown fields.

## Diagnostics and exit behavior

`--diagnostics human|jsonl` controls stderr only. It never changes stdout
artifacts or bytes emitted by an executed plan. Normal commands use these fixed
categories:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Execution or I/O failure |
| 2 | CLI usage error |
| 3 | Invalid configuration or IR |
| 4 | Policy refusal |
| 5 | Observed difference |
| 6 | Provider unavailable |
| 70 | Internal invariant violation |

Once `run` starts the selected plan, it returns the plan's exit code unchanged.

## Runtime boundary

The local backend is not an operating-system sandbox. It starts host processes
with exact argv. Project file effects are confined and network access is
replay-only, but untrusted automation still requires an externally isolated
environment. Opaque residual execution is denied unless explicitly enabled.

Internal agents and adapters use a 4 MiB-bounded JSON-RPC v1 protocol. RPC
objects ignore unknown fields for forward compatibility while still rejecting
duplicate keys, invalid IDs, malformed UTF-8, disconnects, and protocol-version
mismatches.

## Conformance and release

The language-neutral golden corpus covers POSIX, zsh, fish, PowerShell, cmd,
Nushell, unknown interpreters, non-UTF-8 residual source, and integer bounds.
Run it together with the CLI cases and internal-agent handshakes using:

```console
cargo build --locked -p deshell
cargo run --locked -p xtask -- conformance target/debug/deshell
```

The release workflow defines six archives: Linux musl, macOS, and Windows for
x86_64 and Arm64. It generates SHA-256 checksums, a keyless signature bundle,
and build provenance, and publishes to crates.io only for the final `v0.1.0`
tag in the protected release environment.

The safe corpus auditor never executes source scripts. It inventories source,
rechecks each content digest, and analyzes isolated temporary copies. Its
selection and report contract are documented in
[`docs/corpus-audit.md`](docs/corpus-audit.md). A 0.1.0 release requires the
declared 2026-08-25 48-repository selection to have zero unexplained inventory,
IR, guarantee, residual-reason, diagnostic, or export differences.

## OCaml reference

OCaml is retained only for deterministic contract comparison. It is not the
default CLI, an install target, or a release artifact. Reference setup and tests
are opt-in through the `reference:*` mise tasks; operating-system integration,
packaging, and future runtime work belong to Rust.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
