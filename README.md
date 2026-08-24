# de-shell

[![CI](https://github.com/P4suta/de-shell/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/P4suta/de-shell/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/P4suta/de-shell/badge)](https://securityscorecards.dev/viewer/?uri=github.com/P4suta/de-shell)

`de-shell` is an OCaml behavioral compiler for migrating shell automation. It
inventories shell files, embedded shell, and their callers; lowers supported
behavior into a typed Effect IR; and preserves unsupported behavior as explicit,
executable residual capsules.

Behavior preservation and improvement are separate operations. Equivalent
rewrites can be applied transactionally, while portability, security, and
reproducibility changes are emitted as proposals and require an explicit
`--apply`.

## Status

This repository is a production-oriented 0.1.0 pre-release, not a certified
1.0 release. Its implemented subset is intended for real migrations: accepted
behavior carries an explicit guarantee, unsupported behavior remains an exact
executable capsule, and behavior-changing improvements require approval. The
following compiler and runtime foundations are implemented and covered by the
repository's test gates:

- Git-aware recursive inventory of shell files plus Make, Dockerfile, GitHub
  workflows and composite actions, GitLab CI, Azure Pipelines, CircleCI,
  `package.json`, and VS Code shell tasks;
- conservative embedded-shell detection across JVM, .NET, native, BEAM, and
  scripting-language source families, including multiple callsites on one line,
  import-aware JavaScript APIs, and candidate fallback for dynamic host
  expressions;
- executable-field candidate reporting for unrecognized YAML, JSON, and TOML
  hosts without treating lockfiles and descriptive metadata as commands;
- a call graph and content-hash-guarded, all-or-nothing callsite migration for
  exact standalone calls;
- POSIX sh/Bash lowering for literal commands, immutable assignment dataflow,
  command-local environments, pipelines, sequences, `&&`, homogeneous `||`,
  simple conditions, static loops, script-relative working directories,
  literal heredoc writes, and static subshell working directories;
- conservative literal subsets for zsh, fish, PowerShell, cmd, and Nushell,
  including immutable PowerShell scalar/environment dataflow, typed script
  parameters, validation attributes, `CmdletBinding` common parameters, and
  static multi-statement files, with trace-only residual fallback for unknown
  or dynamic behavior;
- versioned Effect IR, JSON schemas, v0/v1-to-v2 migration, and `formal`,
  `exhaustive`, `differential`, and `residual` evidence;
- official PowerShell AST and pinned `nu-parser` JSON-RPC adapter contracts;
- equivalent rewrites, separate modernization proposals, concolic scenario
  generation, command-effect models, and differential comparison;
- a policy-gated internal runner with declared environment inheritance,
  project-scoped filesystem backend, secret redaction in traces and failure
  diagnostics, fail-fast template validation, and deterministic replay tapes;
- rootless OCI launch specifications, a Windows Sandbox launcher, a portable
  observer agent, and Hyper-V/Virtualization.framework agent protocols;
- strict Internal, Dagger, Nushell, and CWL 1.2 exporters, with explicit
  `--bridge` fallback only;
- an installable package containing the CLI, process/observer agents, schemas,
  PowerShell adapter, and Rust Nushell adapter;
- a machine-checkable 1.0 release-gate evaluator for the 95% corpus, complete
  residual execution, embedded inventory, and 7-shell x 3-OS requirements.

Unsupported input is never silently discarded. It is either rejected by policy
or represented as a residual capsule with its interpreter, source, reason, and
source map.

### Host-language inventory boundaries

The source scanner has contract fixtures for Java, Kotlin, Scala, Groovy,
Python, JavaScript/TypeScript, Go, Rust, C/C++/Objective-C, C#, F#, VB, OCaml,
Haskell, Elixir, Erlang, Lua, Perl, Ruby, PHP, R, Nim, D, Clojure, Dart, Julia,
Zig, and Crystal. It recognizes explicit shell APIs or explicit `sh -c`,
`pwsh -Command`, and `cmd /c` launcher shapes. Host-language interpolation,
concatenation, dynamic command expressions, and extra command arguments are
reported as candidates rather than being presented as static shell.

Each detected callsite carries a line-and-column-stable locator. Multiple direct
or argv-shaped shell calls on the same source line are inventoried separately,
and JavaScript's unqualified `exec`/`execSync` forms require an explicit
`child_process` import or binding.

This is conservative lexical inventory, not a claim that de-shell contains a
complete parser for every host language. Commented examples, string literals
containing API names, non-shell shebangs, and direct process APIs without a
shell launcher are excluded by contract. Source-language callsites are not
automatically rewritten yet: `--apply` refuses them until that language has a
syntax-aware patcher.

### Honest runtime boundaries

The local `deshell run` backend is not an OS sandbox: `Exec` nodes start host
processes. File effects remain project-scoped and residual/network effects are
denied unless explicitly allowed, but untrusted automation should be run through
the disposable observer path.

`deshell init` intentionally writes `lab.image = "unconfigured"` to
`deshell.lock`. Observation stays unavailable until the project selects a real,
digest-pinned lab image containing `deshell-observer-agent`. Linux supports
rootless Podman or rootless Docker and Windows supports Windows Sandbox. The
Hyper-V and macOS Virtualization.framework protocols are implemented, but their
signed platform launcher helpers and a published lab image are release
artifacts, not files fabricated by this repository.

The repository validates generated artifacts structurally and exercises the
Nushell output with the pinned official parser. The optional
`mise run test:official-exporters` gate additionally validates a representative
CWL artifact with a digest-pinned official `cwltool` image and executes the
corresponding module with Dagger v0.21.8. Final 1.0 certification still requires
that official-tool evidence across the release corpus, signed
binaries/installers and platform launchers, and the complete physical 21-cell
OS/shell matrix.

## Toolchain with mise

The development toolchain is managed by
[mise](https://mise.jdx.dev/). `mise.toml` and `mise.lock` pin opam 2.5.2,
Rust 1.98.0, PowerShell 7.6.5, actionlint 1.7.12, and Dagger 0.21.8. Opam
creates the local `_opam` switch with OCaml 5.5.0 and Dune 3.24.2; no global
OCaml installation is used. Rust builds are limited to one job for predictable
memory use.

```console
mise trust
mise install
mise run setup
mise run build
mise run test
mise run package
```

After changing `de-shell.opam`, rerun `mise run setup`. `mise run package`
builds the Rust adapter through Dune and verifies every required installed file.

| Task | Purpose |
| --- | --- |
| `mise run build` | Build the OCaml project and Rust adapter |
| `mise run deshell -- ARGS` | Run the project-local CLI |
| `mise run lint` | Validate opam metadata and GitHub Actions |
| `mise run test:fast` | Deterministic unit and property tests |
| `mise run test:contract` | CLI, schema, adapter, and exporter contracts |
| `mise run test:adapters` | Pinned parser-adapter contracts |
| `mise run test:official-exporters` | Validate and execute representative CWL/Dagger exports with pinned official tools |
| `mise run test:platform` | Scanner, filesystem, process, and lab contracts |
| `mise run test:differential` | Lowering and observation comparisons |
| `mise run test:security` | Traversal, protocol, secret, and transaction regressions |
| `mise run test` | Every repository test gate |
| `mise run corpus:audit -- ARGS` | Inventory repositories and analyze hash-verified temporary shell copies without executing source scripts |
| `mise run fmt` | Format OCaml and Dune sources |
| `mise run fmt:check` | Check formatting without accepting changes |
| `mise run package` | Build and verify the installable package payload |

The official-exporter gate is intentionally separate from `mise run test`
because it requires a running Docker engine. `mise install` supplies the pinned
Dagger CLI; `DESHELL_DAGGER_EXE` can override it for controlled validation. The
validator uses a digest-pinned `cwltool` image without mounting the Docker socket
and removes its isolated temporary project after the run.

## Auditing a local corpus

The bundled corpus auditor makes broad local measurements reproducible without
executing source automation. It records the selected immediate-child
repositories and exclusions, inventories embedded shell, verifies each shell
file hash, and analyzes an isolated temporary copy. See
[docs/corpus-audit.md](docs/corpus-audit.md) for the exact safe command and the
2026-08-25 snapshot: 48 repositories, 1,457 inventory locations, zero analysis
failures, and 2 of 47 raw shell files fully non-residual. That measurement is
local evidence, not 1.0 certification; the document explains the denominator
and the remaining residual groups.

## First migration

```console
mise run deshell -- init
mise run deshell -- scan
mise run deshell -- analyze --entry scripts/build.sh
mise run deshell -- verify
mise run deshell -- rewrite --equivalent --entry scripts/build.sh
mise run deshell -- migrate --target nu --entry scripts/build.sh
mise run deshell -- run -- --original-script-argument
mise run deshell -- export --target dagger
```

Commands that can edit files default to a preview. Pass `--apply` only after
reviewing the patch. Application is rejected if any source hash has changed, and
multi-file migrations either commit in full or leave every file untouched.

`deshell init` creates the canonical project artifacts:

- `.deshell/project.toml`: entrypoints, runtime policy, sandbox, and export policy;
- `.deshell/scenarios/*.toml`: arguments, environment, fixtures, and expectations;
- `.deshell/plan.json`: canonical Effect IR;
- `.deshell/evidence.json`: guarantees, digests, and observation status;
- `deshell.lock`: protocol, adapter, command-model, interpreter, and lab digests.

Use `mise run deshell -- --help` or an installed `deshell COMMAND --help` for the
full CLI contract.

## Guarantee model

Every IR node carries exactly one evidence level:

- `formal`: covered by the named static semantic basis;
- `exhaustive`: covered over a declared finite scenario set;
- `differential`: matched against observations for declared scenarios;
- `residual`: not lowered, with an explicit reason and original capsule.

`deshell verify` audits coverage. It never upgrades static evidence merely
because observation was requested; differential status is recorded only from an
actual comparison.

## Design boundaries

The analysis core is deterministic and platform-neutral. Process, filesystem,
network, and disposable-lab effects live behind runtime boundaries. Adapters use
versioned JSON-RPC 2.0 over stdin/stdout with message limits and forward-compatible
unknown fields. Exporters are strict by default and reject any node they cannot
represent.

Repository governance follows the same explicit-capability rule: branch and
release-tag Rulesets are enforced natively. GitHub does not expose push Rulesets
for this public, personal-account repository, so the required CI gate enforces
the equivalent tracked-file size and path-length guardrails and records the
platform limitation in `.github/settings/capabilities.json`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the TDD contract and
[ROADMAP.md](ROADMAP.md) for the remaining release-only gates. Support questions
belong in [Discussions](https://github.com/P4suta/de-shell/discussions), and
vulnerabilities must follow the private process in [SECURITY.md](SECURITY.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
