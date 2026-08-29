# de-shell

[![CI](https://github.com/P4suta/de-shell/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/P4suta/de-shell/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/P4suta/de-shell/badge)](https://securityscorecards.dev/viewer/?uri=github.com/P4suta/de-shell)

`de-shell` is a Rust shell-retirement migration oracle. It inventories standalone
and embedded shell, lowers only the behavior it can prove into Effect IR v1,
obtains ordinary project-native replacement proposals, and independently
compares the original, IR, and replacement before an atomic retirement. It is
not a production runtime or a new automation DSL, and generated code has no
de-shell runtime dependency.

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

- repository-wide `audit`, scenario synthesis, content-addressed migration
  plans, matrix-keyed Evidence import, atomic apply, archive integrity, status,
  a shell-free CI gate, and a separate hardening approval/evidence series;
- official Rust, Go, and structured-host generators plus a digest-pinned,
  bounded JSON-RPC generator bridge. Generators may propose create/update
  patches and exact argv, but only core may archive or delete a live source;
- Git-aware, byte-safe, bounded-worker Inventory v1 scanning of shell files and
  conservative host-format-specific embedded shell candidates. Unrelated data
  and binary files remain outside that inventory scope; read, size, encoding,
  and structural JSON/JSONC, TOML, YAML, or Dockerfile failures for in-scope
  hosts are recorded explicitly. JSONC comments and trailing commas are parsed
  without changing reported source-byte spans;
- literal and explicitly modelled subsets of POSIX shell, zsh, fish,
  PowerShell, cmd, and Nushell, plus a policy-controlled unknown-interpreter
  non-executable residual;
- typed `text`, `bool`, signed 64-bit `int`, normalized `path`, and `secret<T>`
  values, with explicit `literal`, `variable`, and `argument` text parts;
- deterministic node IDs derived from normalized paths, half-open byte spans,
  operation kinds, and preorder positions;
- static `native`, `delegated`, and `residual` node guarantees, with observed
  comparisons and their scenario/provider/runtime keys stored separately in
  Evidence v1;
- strict duplicate-key and unknown-field rejection for persisted JSON,
  RFC 8785-style canonical bytes for digests, and deterministic pretty files;
- transactional equivalent rewrites that protect comments and heredoc data,
  opt-in modernization proposals, strict exporters, replay-only network access,
  and project-confined filesystem boundaries;
- disposable-lab launch contracts with fail-closed provider selection: Podman,
  rootless Docker, Windows Sandbox/Hyper-V, or the signed
  Virtualization.framework helper; no path silently falls back to local;
- raw-byte process output internally, UTF-8 validation at text capture
  boundaries, bounded execution, and secret-safe diagnostics and traces;
- one multicall executable with hidden process-agent, observer-agent, and
  Nushell-adapter modes using bounded newline-delimited JSON-RPC v1 and ordered
  256 KiB stream chunks;
- embedded schemas retrievable with `deshell schema NAME`.

Unsupported behavior is never silently discarded. Known interpreters produce
an `interpreter_call` with an exact source span, exact UTF-8/base64 bytes,
required capabilities, reason, and interpreter pin. `opaque_capsule` is
reserved for residual source and cannot execute. Observation is evidence for a
specific scenario and provider/runtime key; it is not a proof for arbitrary
inputs.

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
| `mise run test:supply-chain` | RustSec advisory, license, duplicate-dependency, and source-policy checks |
| `mise run test:schema-validator` | Independent meta-schema and generated-document validation |
| `mise run coverage` | LLVM line coverage with the v0.1 90% release floor |
| `mise run lint` | Clippy, repository guardrails, and workflow validation |
| `mise run performance` | Record fixed-corpus scan, simple local run, and release binary metrics as JSON |
| `mise run package` | Verify the crates.io payload |
| `mise run reference:test` | Test the unpublished OCaml reference explicitly |

Every implementation phase follows Red, Green, Refactor: first add a failing
public contract or focused reproduction, implement the smallest correct change,
then refactor while the relevant and full gates stay green. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Performance measurements

`mise run performance` builds the release executable, creates an isolated fixed
corpus of 4,096 Python and JavaScript host files plus a one-command project, and
prints one JSON report. Each scan and `run --backend local` metric uses
five warm-up runs and twenty measured runs; the report contains median and p95
milliseconds, release binary bytes, operating system, and architecture.

An already-built executable can be measured directly, which is useful for
same-machine before/after comparisons:

```console
cargo run --locked -p xtask -- performance /path/to/deshell
```

The report is recording evidence only. The task has no time-based CI failure
threshold and does not complete the separate release-runner regression gate in
the roadmap.

## First retirement

```console
mise run deshell -- init --entry scripts/build.sh
mise run deshell -- audit --format human
mise run deshell -- scenario synthesize
# Review the draft, fill its boundaries, then set approval = "approved".
mise run deshell -- migrate plan
mise run deshell -- migrate verify --plan PLAN_DIGEST --cell CELL --output evidence.json
mise run deshell -- migrate evidence import --plan PLAN_DIGEST evidence.json
mise run deshell -- migrate apply --plan PLAN_DIGEST
mise run deshell -- migrate status --format human
mise run deshell -- verify --require shell-free
```

Scenario synthesis and migration planning are preview-first. A scenario is not
retirement evidence until a human changes it to `approval = "approved"`.
Start a repository proposal with `deshell migrate plan`; after retirement, CI
must keep `deshell verify --require shell-free` enabled.
`migrate apply` has no partial mode: one blocker, missing cell, stale digest,
difference, nondeterminism, delegated/residual byte, validation failure, or
remaining shell candidate refuses the whole repository transaction. Intentional
behavior changes use `deshell harden`, never migration equivalence. Project
files are fresh v1 contracts; 0.1.0 intentionally provides no legacy project,
IR, or lock migration path.

`deshell init` creates:

- `.deshell/project.toml`, containing entrypoints and policy;
- `.deshell/scenarios/default.toml`, containing named inputs and expectations;
- `.deshell/manifest.json`, initially containing no analyzed entries;
- `deshell.lock`, containing protocol, interpreter, and provider pins.

The lower-level `deshell analyze` command writes immutable plan/evidence pairs below
`.deshell/artifacts/<source-sha256>/<plan-sha256>/` and atomically updates the
manifest. Reanalysis never overwrites Evidence for older content. Name/value
arrays reject duplicates, paths are normalized project-relative paths, and
persisted JSON rejects unknown fields.

`export --mode strict` uses only target runtime pins from `deshell.lock` (for
example `targets.dagger_image`) and rejects unconfigured or unrepresentable
targets. The current external-target strict contract accepts one literal `Exec`
with no hidden task interface, environment, or working-directory effect; it
rejects multi-command sequence status semantics instead of approximating them.
`export --mode bundle --output PATH` additionally requires an exact runtime
asset for the current OS/architecture in `lab.assets`; its deterministic tar
contains the exact `deshell` binary, every active manifest entry and its source,
plan and Evidence, the lock, scenarios, assets at their exact locked project
paths, minimal capabilities, and a digest-bound Bundle v1 manifest. The manifest
also fixes the selected entrypoint and extraction-relative run command.

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
| 5 | Observed difference or nondeterminism |
| 6 | Provider unavailable |
| 70 | Internal invariant violation |

Once `run` starts the selected plan, it returns the plan's exit code unchanged.

## Runtime boundary

After retirement, de-shell remains a development/CI dependency only, to reject
shell reintroduction and archive/Evidence tampering. It is not linked into or
invoked by the generated production program.

`run` defaults to a private snapshot in a disposable provider. Missing provider
features, an unconfigured image, or a pin mismatch fail with code 6. The local
backend is not an operating-system sandbox: it starts host processes with exact
argv and therefore trusts the selected plan and every program it launches. It
is available only when the project sets `sandbox.allow_local = true` and the
caller also supplies `--backend local`. Residual nodes never execute, and
delegation is available only inside a pinned disposable runtime.

This source tree directly connects the supervised Podman and rootless-Docker
process transports. Windows Sandbox/Hyper-V and Virtualization.framework have
validated launch contracts, but `doctor` reports them unavailable until their
signed helper transport is installed and connected; they never fall back to a
host shell.

Internal agents and adapters use a 4 MiB-per-frame JSON-RPC v1 protocol.
stdout/stderr use ordered canonical-base64 chunks of at most 256 KiB when a
result does not fit one frame; sequence, total bytes, and SHA-256 are verified.
RPC objects ignore unknown fields for forward compatibility while still
rejecting duplicate keys, invalid IDs, malformed UTF-8, disconnects, and
protocol-version mismatches.

## Conformance and release

The language-neutral golden corpus covers POSIX, zsh, fish, PowerShell, cmd,
Nushell, unknown interpreters, non-UTF-8 delegated source, and integer bounds.
Run it together with the CLI cases and internal-agent handshakes using:

```console
cargo build --locked -p deshell
cargo run --locked -p xtask -- conformance target/debug/deshell
```

The release workflow defines six archives: Linux musl, macOS, and Windows for
x86_64 and Arm64. It generates a CycloneDX SBOM, SHA-256 checksums covering the
SBOM and archives, a keyless signature bundle, and build provenance, and
publishes to crates.io only for the final `v0.1.0` tag in the protected release
environment. CI and release candidates enforce at least 90% measured line
coverage. The final tag additionally checks at least 90% in scanner, frontend,
runner, protocol, lab, and patch, and blocks publication unless the full
retirement workflow succeeds with both official Rust and Go generators for
POSIX sh, Bash, zsh, fish, PowerShell, cmd, and Nushell. A candidate passing
does not authorize publication.

The safe corpus auditor never executes source scripts. It inventories source,
rechecks each content digest, and analyzes isolated temporary copies. Its
selection and report contract are documented in
[`docs/corpus-audit.md`](docs/corpus-audit.md). A 0.1.0 release requires the
declared 2026-08-25 48-repository selection to have zero scanner errors/skips,
unclassified shell files, residual executable coverage, or unexplained
inventory, IR, guarantee, diagnostic, observation, patch, or export differences.

## OCaml reference

OCaml is retained only for deterministic contract comparison. It is not the
default CLI, an install target, or a release artifact. Reference setup and tests
are opt-in through the `reference:*` mise tasks; operating-system integration,
packaging, and future runtime work belong to Rust.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
