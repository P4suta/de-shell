# Corpus audit

`deshell-audit-corpus.ps1` provides a reproducible, non-executing audit of the
immediate repository children under a corpus directory. It inventories every
supported embedded format and analyzes shell files on isolated temporary
copies. The report conforms to
`contracts/schema/corpus-audit-v1.schema.json`.

## Run it

Run the audit through the mise-managed PowerShell. The task depends on `build`,
so it cannot analyze with a stale compiler binary. Quote the complete
comma-separated exclusion value: an unquoted list can be split by the caller
before it reaches the script.

```console
mise run corpus:audit -- -CorpusRoot .. -ExcludeRepository 'de-shell,workflow-verifier,beamtrace' -ExcludePattern 'cargo-mutants-wt-*' -DeshellExecutable target/debug/deshell -Format Json -OutputPath target/local-corpus-audit.json
```

Exact exclusions must name an immediate child of `-CorpusRoot`; a typo fails
closed instead of silently broadening the audit. The JSON records the normalized
exact exclusions, patterns, selected repositories, and `source_execution=false`
so the selection can be reviewed with the result. Use `-Format Human` for a
concise terminal summary. `target/local-corpus-audit.json` is a local evidence
artifact and is not committed.

The auditor:

- invokes `deshell scan` only to inventory each source repository;
- limits structured-host parsing to known automation paths/names or documents
  containing conservative shell-bearing keys; unrelated binary and data files
  are outside Inventory v1 rather than reported as shell scan failures;
- treats every Inventory v1 `skipped` or `errors` entry as an audit failure;
- accepts position-preserving JSONC comments and trailing commas, but rejects
  duplicate JSON or YAML mapping keys and malformed in-scope host documents;
- resolves every reported shell path below its repository root and verifies its
  SHA-256 content digest after the scan;
- copies each shell file into a uniquely named, verified system-temporary
  directory;
- runs `deshell init` and `deshell analyze` only on that copy;
- never invokes `deshell run` or the source script;
- resolves content-addressed Evidence through `.deshell/manifest.json` and
  requires every analyzed node to be `native` or explicitly `delegated`;
- omits source bodies from the report and removes only a verified audit temp
  directory.

## Rust implementation, 2026-09-19

The first run of this auditor against the Rust implementation. It could not run
before: `deshell scan --format json` emits a Scan Report v1 and the auditor read
`$report.findings`, which no report has. A missing property is `$null` in
PowerShell and `@($null)` is an array holding one null, so every repository
produced one finding whose every field was empty and the run died on the first
of them with `Cannot bind argument to parameter 'Kind'`. The same shape made
`errors` and `skipped` report 29 failures that each said nothing. Nothing
noticed, because the 2026-08-25 baseline was produced by the OCaml
implementation on a different machine.

The report could not carry a location's content digest either — the auditor
verifies it after the scan — because a Scan Report is built by re-reading the
command's human output, and that line did not print one. It does now.

    mise run corpus:audit -- -CorpusRoot .. -ExcludeRepository 'de-shell,workflow-verifier' -DeshellExecutable target/debug/deshell -Format Human

| Measure | Result |
| --- | ---: |
| Repositories scanned | 14 |
| Inventory locations | 1,006 |
| Shell files | 98 |
| Embedded shell locations | 587 |
| Conservative candidates | 321 |
| Analysis failures | 0 |
| Fully non-residual shell files | 98 / 98 |
| IR nodes: native / delegated / residual | 20 / 94 / 0 |

The selection, which the 2026-08-25 one never recorded anywhere a reader could
reach:

`ginary`, `gleam-mutants`, `go-mutants`, `goatest`, `http3`, `jlreq`, `mylist`,
`njutest`, `njutest-assurance`, `release-glz`, `simple-blog`, `ssocks`,
`storage-scout`, `swift-mutants`.

`workflow-verifier` is excluded for the reason the baseline excluded it, and
also because it vendors a third-party CircleCI configuration whose `release`
jobs carry two `<<` merge keys in one mapping. YAML parsers do not agree on what
a repeated merge key means, so the scanner refuses the document rather than
picking one reading — which is the right answer and does make the repository
unauditable under a rule that counts every scan error as a failure.

Zero residual nodes across 98 shell files is not the same claim the 2026-08-25
snapshot made with 2 of 47: that one counted the obsolete pre-v1 vocabulary, and
94 of the 114 nodes here are `delegated`, which is a pinned interpreter running
the exact bytes and not a translation. What it does say is that every file
reached a guarantee rather than an unexamined remainder.

## Historical pre-cutover baseline: 2026-08-25

This snapshot was produced under the obsolete pre-v1 guarantee vocabulary by
the unpublished OCaml implementation on a Windows
development machine. It is retained only as the selection and comparison
baseline. The same declared 48-repository selection must be rerun with the Rust implementation,
and unexplained differences must be zero before `0.1.0` is
released. The run excluded the actively changing `workflow-verifier` and
`beamtrace` repositories and the de-shell repository itself.

| Measure | Result |
| --- | ---: |
| Repositories scanned | 48 |
| Inventory locations | 1,457 |
| Shell files | 47 |
| Embedded shell locations | 1,244 |
| Conservative candidates | 166 |
| Analysis failures | 0 |
| Legacy fully non-residual shell files | 2 / 47 |
| Legacy formal IR nodes | 47 |
| Legacy residual IR nodes | 45 |
| Legacy exhaustive nodes / observations | 0 / 0 |

The two fully non-residual files were:

- `film-frame/package.sh` (27 formal nodes)
- `terminfokit/scripts/fetch-ncurses-oracle.sh` (20 formal nodes)

The 45 residual files were grouped by their first atomic residual reason after
typed PowerShell parameters, typed POSIX branch state, safe static unquoted
fields, and simple/quoted-nested command capture were implemented:

| Files | Interpreter | Reason |
| ---: | --- | --- |
| 11 | PowerShell | expression/state assignments exceed the immutable scalar subset |
| 8 | POSIX sh / Bash | redirection and asynchronous-process semantics |
| 5 | cmd | dynamic expansion in generated Gradle launchers |
| 3 | PowerShell | effectful `ValidateScript(Test-Path …)` input contracts |
| 3 | POSIX sh | special parameters such as `$?`/`$@` need explicit typed semantics |
| 2 | PowerShell | parameter-set selection semantics |
| 2 | PowerShell | non-literal text defaults |
| 11 | Bash/cmd/fish/PowerShell/sh/zsh | eleven distinct singleton syntax or runtime boundaries |

This is deliberately a raw shell-file audit. Its denominator includes five
copies each of the generated `gradlew` and `gradlew.bat` launchers and four
interactive completion definitions. Embedded locations are inventoried but are
not lowered by this shell-file analysis pass. The old `formal` count is
historical data only and must not be interpreted as current `native` coverage;
the snapshot contains no current scenario/provider/runtime-keyed Evidence.

Consequently, this snapshot does not certify de-shell 1.0 and is not the
release-gate corpus. A release corpus must explicitly declare non-interactive
entrypoints and scenarios, then record matching observations on the required
OS/shell matrix. The unchanged whole-file count does not mean the compiler made
no progress. The former 16-file typed-parameter blocker and six-file POSIX
control-assignment blocker are gone; the unquoted-expansion first-boundary group
fell from four to one; and the five files formerly stopped at command
substitution now reach special-parameter or redirect semantics. The historical
result documents the earlier inventory and two static slices while
identifying PowerShell expression/object state, redirects, special shell
parameters, parameter sets, effectful validation, and generated launcher
semantics as the next major compiler work.

For the 0.1.0 gate, rerun the fixed 48-repository selection with the Rust
binary and the current schema. Acceptance requires zero audit failures,
scanner panics/errors/skips, and residual shell files. Every recognized source
byte must be classified as native or delegated; each delegated node must carry
its exact bytes, span, reason, capabilities, and interpreter pin. Scenario
results are reported separately: stale observations do not fail the current
run, while a current difference or nondeterministic key does.
