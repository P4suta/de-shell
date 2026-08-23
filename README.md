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

This repository is an experimental 0.1.0 implementation, not a certified 1.0
release. The following compiler and runtime foundations are implemented and
covered by the repository's test gates:

- recursive inventory of shell files plus Make, Dockerfile, GitHub Actions,
  GitLab CI, Azure Pipelines, CircleCI, `package.json`, and VS Code shell tasks;
- candidate-only reporting for unrecognized YAML, JSON, and TOML string hosts;
- a call graph and content-hash-guarded, all-or-nothing callsite migration for
  exact standalone calls;
- POSIX sh/Bash lowering for literal commands, pipelines, sequences, `&&`,
  simple conditions, and static loops;
- conservative literal subsets for zsh, fish, PowerShell, cmd, and Nushell,
  with trace-only residual fallback for unknown or dynamic behavior;
- versioned Effect IR, JSON schemas, v0-to-v1 migration, and `formal`,
  `exhaustive`, `differential`, and `residual` evidence;
- official PowerShell AST and pinned `nu-parser` JSON-RPC adapter contracts;
- equivalent rewrites, separate modernization proposals, concolic scenario
  generation, command-effect models, and differential comparison;
- a policy-gated internal runner, project-scoped filesystem backend, secret
  redaction in traces and failure diagnostics, and deterministic replay tapes;
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
Nushell output with the pinned official parser. Final 1.0 certification still
requires official Dagger/CWL runner validation, signed binaries/installers, and
the complete physical 21-cell OS/shell matrix.

## Toolchain with mise

The development toolchain is managed by
[mise](https://mise.jdx.dev/). `mise.toml` and `mise.lock` pin opam 2.5.2,
Rust 1.98.0, PowerShell 7.6.5, and actionlint 1.7.12. Opam creates the local
`_opam` switch with OCaml 5.5.0 and Dune 3.24.2; no global OCaml installation is
used. Rust builds are limited to one job for predictable memory use.

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
| `mise run test:platform` | Scanner, filesystem, process, and lab contracts |
| `mise run test:differential` | Lowering and observation comparisons |
| `mise run test:security` | Traversal, protocol, secret, and transaction regressions |
| `mise run test` | Every repository test gate |
| `mise run fmt` | Format OCaml and Dune sources |
| `mise run fmt:check` | Check formatting without accepting changes |
| `mise run package` | Build and verify the installable package payload |

## First migration

```console
mise run deshell -- init
mise run deshell -- scan
mise run deshell -- analyze --entry scripts/build.sh
mise run deshell -- verify
mise run deshell -- rewrite --equivalent --entry scripts/build.sh
mise run deshell -- migrate --target nu --entry scripts/build.sh
mise run deshell -- run
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
