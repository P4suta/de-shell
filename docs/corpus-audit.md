# Corpus audit

`deshell-audit-corpus.ps1` provides a reproducible, non-executing audit of the
immediate repository children under a corpus directory. It inventories every
supported embedded format and analyzes shell files on isolated temporary
copies. The report conforms to `schema/corpus-audit.schema.json`.

## Run it

Build the local CLI first, then run the audit through the mise-managed
PowerShell. Quote the complete comma-separated exclusion value: an unquoted
list can be split by the caller before it reaches the script.

```console
mise run build
mise run corpus:audit -- -CorpusRoot .. -ExcludeRepository 'de-shell,workflow-verifier,beamtrace' -ExcludePattern 'cargo-mutants-wt-*' -DeshellExecutable _build/default/bin/main.exe -Format Json -OutputPath _build/local-corpus-audit.json
```

Exact exclusions must name an immediate child of `-CorpusRoot`; a typo fails
closed instead of silently broadening the audit. The JSON records the normalized
exact exclusions, patterns, selected repositories, and `source_execution=false`
so the selection can be reviewed with the result. Use `-Format Human` for a
concise terminal summary. `_build/local-corpus-audit.json` is a local evidence
artifact and is not committed.

The auditor:

- invokes `deshell scan` only to inventory each source repository;
- resolves every reported shell path below its repository root and verifies its
  SHA-256 content hash after the scan;
- copies each shell file into a uniquely named, verified system-temporary
  directory;
- runs `deshell init` and `deshell analyze` only on that copy;
- never invokes `deshell run` or the source script;
- omits source bodies from the report and removes only a verified audit temp
  directory.

## Local snapshot: 2026-08-24

The command above was run on the current Windows development machine, excluding
the actively changing `workflow-verifier` and `beamtrace` repositories and the
de-shell repository itself.

| Measure | Result |
| --- | ---: |
| Repositories scanned | 48 |
| Inventory locations | 1,457 |
| Shell files | 47 |
| Embedded shell locations | 1,244 |
| Conservative candidates | 166 |
| Analysis failures | 0 |
| Fully non-residual shell files | 2 / 47 |
| Formal IR nodes | 47 |
| Residual IR nodes | 45 |
| Exhaustive / differential nodes | 0 / 0 |

The two fully non-residual files were:

- `film-frame/package.sh` (27 formal nodes)
- `terminfokit/scripts/fetch-ncurses-oracle.sh` (20 formal nodes)

The 45 residual files were grouped by their first atomic residual reason:

| Files | Interpreter | Reason |
| ---: | --- | --- |
| 16 | PowerShell | typed parameter blocks need typed input lowering |
| 6 | PowerShell | assignment expressions exceed the immutable literal subset |
| 6 | POSIX sh | assignment after control flow needs chronological shell state |
| 5 | cmd | dynamic expansion in generated Gradle launchers |
| 5 | POSIX sh | redirection/control effects in generated Gradle launchers |
| 7 | Bash/cmd/fish/sh/zsh | seven distinct singleton syntax or runtime boundaries |

This is deliberately a raw shell-file audit. Its denominator includes five
copies each of the generated `gradlew` and `gradlew.bat` launchers and four
interactive completion definitions. Embedded locations are inventoried but are
not lowered by this shell-file analysis pass. A `formal` node identifies its
static semantic basis; this snapshot contains no scenario-based exhaustive or
differential evidence.

Consequently, this snapshot does not certify de-shell 1.0 and is not the
release-gate corpus. A release corpus must explicitly declare non-interactive
entrypoints and scenarios, then pass differential observation on the required
OS/shell matrix. The current result is useful local evidence: it demonstrates
safe inventory and two end-to-end static slices while identifying typed
PowerShell inputs, expression/state lowering, POSIX chronological state, and
generated launcher semantics as the highest-impact remaining work.
