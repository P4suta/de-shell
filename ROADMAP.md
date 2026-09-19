# Roadmap

The first public release is the Rust `deshell` 0.1.0 CLI. Work is accepted in
Red, Green, Refactor increments, and a checked box means the repository contains
the implementation and deterministic test—not that an external release or
hardware gate has already run.

## 0.1.0 migration oracle — implementation in progress

Version 0.1.0 must not be published until representative end-to-end retirement
passes for all seven interpreters (POSIX/Bash, zsh, fish, PowerShell, cmd, and
Nushell), with both official Rust and Go generators where applicable.

- [x] Repository-wide audit, scenario draft, migration plan/verify/evidence
  import/apply/status, harden, and shell-free gate command surfaces.
- [x] Strict v1 Generator Protocol, Migration Request, Proposal, Migration Plan,
  Migration Evidence, Archive Manifest, Audit Finding, and harden contracts.
- [x] Content-addressed plans, full-cell Evidence keys, repeat comparison,
  coverage checks, staged shell-free scan, atomic retirement, rollback, and
  archive/Evidence integrity verification.
- [x] Official Rust, Go, Docker/Python/JavaScript/GitHub-host generators and a
  digest-pinned isolated external-generator bridge.
- [x] Complete project-native Make/package/task interface rewrites and resolve
  every supported static call site in the proposal rather than blocking it.
- [x] Add enforced network record/replay observations and the complete
  seven-interpreter, two-generator E2E corpus.

## Foundational contract and compiler substrate — implemented

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

- [ ] Pass `scenario -> plan -> matrix verify -> evidence import -> apply ->
  scan zero` for shell files and embedded sources across the seven interpreters,
  including input, environment, branch, failure, and parser-blocker cases.

  Walked end to end on macOS, following only the argv each step printed:
  `init`, `scenario approve`, `matrix approve`, `migrate plan`,
  `migrate verify`, `migrate evidence import`, `migrate apply`,
  `verify --require shell-free`.

  | interpreter | |
  | --- | --- |
  | `sh`, `bash`, `zsh` | retired, shell-free |
  | `nushell` | retired, shell-free |
  | `powershell` | retired, shell-free |
  | `fish`, `cmd` | plan reached `planned`; neither runtime is on this machine |

  Each ends with the shell file gone and `src/bin/deshell_build.rs` in its
  place. `fish` and `cmd` lower and plan; `migrate verify` runs the original,
  which needs the interpreter, so those two wait for a runner that has one.

  PowerShell was the one that taught something, and then taught it again. Its
  first verification reported `different` — the replacement wrote `one` and exit
  0, the original wrote nothing and exit 1. That was written down here as the
  tool being right twice over: the difference was real, and reporting it rather
  than passing is the point.

  It was right about the bytes and wrong about what they meant, which is the
  more expensive half. The original had not run at all. `pwsh` on this machine
  is a `mise` shim, a shim resolves its version from the configuration nearest
  the working directory, and the comparison happens in a private workspace under
  the system temporary root. A reader following `DESHELL_DIFFERENCE` would go
  looking for a fault in the generated program.

  Two defects, both since fixed. The parsers ran in their own scratch
  directories, which made de-shell unable to use *any* interpreter installed
  through `mise`, `asdf` or `volta` — it reported `runtime unavailable` and
  delegated a block whose runtime was present. And a comparison did not check
  that the baseline had been taken. The original's interpreter is now probed
  with an empty script through the same argv the comparison uses, and a failure
  to start is `unavailable`, exit 6, not `different`, exit 5.

  Walked both ways afterwards: the same source retires from a project whose
  `mise.toml` declares the runtime, and reports `unavailable` from one that does
  not.

  An embedded source walks the same flow. A workflow whose step is

  ```yaml
      - name: build
        run: |
          /bin/echo building
  ```

  reaches `retired` with the step rewritten to
  `uses: ./.github/actions/deshell-f920c0703cce`, a local action holding the
  generated program, and `verify --require shell-free` passing.

  The five remaining cases walked next, and three of them ended somewhere other
  than `retired` for reasons that are the tool working:

  | case | source | outcome |
  | --- | --- | --- |
  | input | `/bin/echo "$1"` | retired, shell-free |
  | environment | `/bin/echo "${HOME}"` | retired, shell-free |
  | branch | `if [ "$1" = a ]` | retired, shell-free, once a second scenario observed the other arm |
  | failure | `set -e` then a command that always fails | refused at `apply`: `DESHELL_BLOCKER_COVERAGE_INCOMPLETE` |
  | parser-blocker | `eval "/bin/echo $1"` | `blocked`; `eval` is a parser blocker and never reaches a plan |

  `branch` first reported `DESHELL_BLOCKER_COVERAGE_INCOMPLETE`, naming the node
  no scenario had entered. A second scenario with `argv = ["a"]` covers it and
  the file retires. `failure` is the same refusal and stays: `set -e` makes the
  statement after the failing one unreachable, so no scenario can observe it,
  and de-shell will not retire a file holding a node nobody has seen run. That
  is the right answer — the alternative is generating a translation of code that
  was never compared against anything.

  Two defects surfaced here, both in the independent IR verifier, and both the
  same shape as ones found earlier on the generation side.

  `Operation::Sequence { nodes, .. }` — the `..` dropped `on_failure`, so the
  verifier ran the statement after a failing one under `set -e`. This is the
  third time that option has been lost to a destructuring that compiled. The
  walk also read `UnsetPolicy::Empty` as a constant, so `set -u` never reached
  the verifier either.

  Behind them was the larger one: the verifier ran four of the IR's thirty-five
  operations, and the rest fell into an `other => Err("does not support {name}")`
  arm. A catch-all made "nobody implemented this" and "this cannot honestly be
  checked here" the same sentence, and nothing counted either. The match is now
  exhaustive with no `_` arm and no `..` in any destructuring, sixteen
  operations run, and `contracts/golden/ir-verifier-coverage-v1.json` records
  what the other nineteen are refused for. Several must stay refused:
  `interpreter_call` would re-run the original shell, and an oracle that
  consults the original is not independent of it.

  A third came from the walk's own scenario. `argv` and `arguments` both name
  `$1`; the verifier read one and the shell read the other, so a scenario that
  disagreed with itself was reported as the replacement disagreeing with the
  original — a real difference, for a reason present in neither program. Saying
  it twice is now fine and saying two different things is refused by name.

  It did not reach `retired` before this round, and the reason was one this
  flow was the only thing that could find: `git ls-files` names a path the
  working tree no longer has after the retirement removes it, and the scanner
  read that as an error. So every retirement of a tracked shell file rolled
  itself back at the post-apply check — which reported "1 errors" and did not
  say which, so nothing pointed at it either.

  The seven interpreters and the embedded-source cases are still to walk.
- [ ] Run every fast, contract, platform, differential, security, package,
  official-exporter, and workflow gate from `v0.1.0-rc.1`, including the
  required three-operating-system matrix.

  Run on macOS aarch64, one at a time, with what each one said:

  | gate | |
  | --- | --- |
  | `test:fast`, `test:contract`, `test:adapters` | pass |
  | `test:platform`, `test:differential`, `test:security` | pass |
  | `test:builtin-semantics`, `test:schema-validator` | pass |
  | `package` | pass |
  | `performance` | pass |
  | `test:supply-chain` | advisories, bans, licenses, sources ok |
  | `test:official-exporters` | needs Docker, absent here |
  | `lint` | pass |

  Two of them failed until this round and both were real: the Effect IR schema
  did not describe six operations the IR produces, and the performance fixture
  measured a run that could not start. `test:supply-chain` needs `--offline`
  on this machine, because `cargo deny` refreshes its advisory database with
  `git reset --hard` and the shell here refuses that; the check itself passes.

  What is left for the release runner is the three-operating-system matrix and
  the exporter gate, neither of which this machine can stand in for.
- [x] Enforce measured line coverage at 90% overall and at least 90% in scanner,
  frontend, runner, protocol, lab, and patch as a 0.1.0 release gate.
- [ ] Retire de-shell's own shell. The nearest repository was the last one it
  was pointed at, and pointing it here found three defects that the corpus had
  not: `deshell init` could not run on this repository at all
  (`DESHELL_IO: duplicate exact location override`, 3793187), every candidate in
  a parsed JSON document reported `@0..1` (233dc5a), and the PowerShell and
  Nushell parsers could not use an interpreter installed through a version
  manager (331bb8e).

  It runs now. The baseline, measured on 2026-09-19:

  | | |
  | --- | --- |
  | shell locations | 101 — 46 embedded bash, 12 embedded PowerShell, 7 shell files, 36 candidates |
  | sources in the plan | 65 |
  | blockers | 118 |
  | retired | 0 |

  The blockers, by code, on 2026-09-19 after the work below:

  | count | code | what it is |
  | --- | --- | --- |
  | 18 | `UNIMPLEMENTED_SEMANTIC` | PowerShell steps and scripts using control syntax outside the modelled subset |
  | 11 | `DYNAMIC_CANDIDATE` | shell in `mise.toml` tasks and a Python contract validator |
  | 9 | `UNRESOLVED_CALL_SITE` | `run: ./scripts/install-nushell.ps1`, whose target is one of those scripts |
  | 6 | `RESIDUAL_SOURCE` | steps holding `${{ }}`, which GitHub substitutes before a shell sees them |
  | 2 | `GENERATOR_UNSUPPORTED` | |
  | 2 | `SCENARIO_INPUT_COVERAGE` | |

  It started at 118. What came off, and what each was:

  | | |
  | --- | --- |
  | 37 | `DUPLICATE_TARGET` — several `run:` blocks in one workflow. Every proposal now carries the same whole-file rewrite, with every block replaced, and identical patches are applied once. Sound because `apply` applies a plan in one transaction. |
  | 25 | `DYNAMIC_CANDIDATE` — the golden corpora, now declared shell rather than shell to retire. |
  | 8 | `GENERATOR_UNSUPPORTED` — a step with several commands now generates a program that runs them in order and stops where the step stops. |

  None of the 48 is a wrong answer. Three things came out of driving the number
  down, and all three were the same shape: reading a host's bytes without the
  host's rules.

  - `${{ }}` is substituted by the runner before a shell sees the text, so those
    bytes are a template. `run: /bin/echo '${{ matrix.os }}'` was lowered
    `native` and would have printed the template where the step printed the
    value.
  - The runner executes `bash -e {0}`, so `set -e` is in effect whether or not
    the step says so. A two-command step lowered to a sequence that carries on
    after a failure, and was claimed `native`.
  - The comparison ran the original as `bash -c <text>`. The baseline was a
    program the runner never runs, so the two agreed when they should not have
    and disagreed when they should not have.

  `pipefail` was the fourth and is closed. The runner's default is `bash -e {0}`
  without it and an explicit `shell: bash` is
  `bash --noprofile --norc -eo pipefail {0}` with it — both known, once somebody
  asks which. Nothing had: `yaml_step_shell` was already consulted to pick the
  interpreter and its answer was thrown away. `Finding` carries
  `host_named_the_shell` now, and `ShellOptions::pipefail_unknown` is gone with
  the refusal it guarded.

  What is left is not this family. It is three pieces of modelling work:

  - **The PowerShell subset** (18). Nine of those are
    `run: ./scripts/install-nushell.ps1`, which is a path invocation rather than
    an explicit `&` call; the other nine are the scripts themselves, which use
    control syntax, cmdlets and variables the frontend does not model. Those
    nine are also the nine `UNRESOLVED_CALL_SITE`, because the call sites cannot
    be resolved until the script they call has a replacement.
  - **A `mise.toml` host generator** (11). Task `run` values are shell in a task
    runner's configuration, and de-shell has no host shape for one.
  - **A GitHub expression model** (6). `${{ }}` would have to become a task
    input the scenario supplies, rather than bytes.

- [ ] Run the fixed 2026-08-25 48-repository audit selection through both
  deterministic implementations and record zero scanner errors/skips,
  unclassified files, residual executable coverage, nondeterminism, or
  unexplained differences in inventory, IR, diagnostics, patches, and exports.

  The auditor had never run against the Rust implementation. It read
  `$report.findings` from a Scan Report v1, which has `details.items`; a missing
  property is `$null` in PowerShell and `@($null)` is an array of one null, so
  every repository produced one finding with every field empty and the run died
  on the first. A gate that has only ever run against the implementation being
  replaced is a gate about the wrong thing. Fixed in 3cbc8e1, along with the
  report not carrying a location's content digest and `init` not being told a
  target for an isolated single-file copy.

  It runs now, and `docs/corpus-audit.md` records the result: 14 repositories,
  1,006 locations, 98 shell files, zero analysis failures, 98 of 98 fully
  non-residual, validated against `corpus-audit-v1.schema.json`.

  **The declared selection is still not reproducible from this repository.** The
  2026-08-25 run named 48 repositories and wrote down only two of their file
  paths and a table of totals; the report format can carry the list and no
  report was committed. So the item as written can be executed only by somebody
  who already has that machine's directory. Either the 48 names go into the
  repository, or the gate's subject becomes a selection that is recorded — the
  2026-09-19 run names its fourteen, which is the first selection anybody else
  could reproduce.
- [ ] Pass the self-hosted rootless-Linux, Windows Sandbox/Hyper-V, and signed
  macOS Virtualization.framework execution gates with no local fallback.
- [ ] Add and pass saved-corpus/PR fuzz smoke, nightly scanner/parser/protocol/
  schema fuzzing, Miri, ASan/UBSan, and validator/policy mutation thresholds.
- [ ] Record scan, simple-run, and release-size baselines on the release runner
  and require review for regressions greater than 20 percent.

  `cargo xtask performance` runs and reports. On this machine — macOS,
  aarch64, a release build — scan over 4096 files is 58 ms median and 61 ms at
  p95, a simple run is 3.3 ms median and 3.6 ms at p95, and the binary is 7.8
  MiB. These are not the baseline: the baseline is the release runner's, and
  what is recorded here is that the harness produces one.

  It did not until now. The fixture ran `/bin/true`, which macOS does not have,
  so the measured run had failed to start; and `deshell init` refuses to choose
  a target for a directory holding one script and nothing else, which the
  benchmark had not said.
- [ ] Install and smoke all six archives, including `--version`, every embedded
  schema, and all three internal-agent handshakes.
- [ ] Verify the CycloneDX SBOM, SHA-256 checksums, Sigstore bundle, and
  provenance for every archive.
- [ ] Confirm crates.io package-name ownership before the irreversible publish
  operation. If `deshell` is unavailable, publish package `deshell-cli` while
  retaining binary name `deshell`.
- [ ] Obtain the release-environment owner approval and publish `v0.1.0`.

## Blocking the claim that de-shell retires real shell — found by dogfooding

Running `deshell scan` and `migrate plan` against OComment's `action.yml`
retired nothing: all five `run:` steps came back as whole-file delegation, each
for the same reason.

```
blocker DESHELL_BLOCKER_UNIMPLEMENTED_SEMANTIC action.yml@3963..6698:
  shell builtin set requires pinned interpreter delegation
```

- [ ] Split `set` by option instead of refusing the builtin by name. Every shell
  builtin is delegated today, `set` among them, and `set -euo pipefail` opens
  approximately every CI step that exists. The same corpus with `set` removed
  lowers 48% of its bytes natively, so this is a granularity problem rather than
  a capability one. Start with `-o pipefail`: it decides a pipeline's exit
  status, which is local and static. `-f` and `-x` stay delegated. Note that
  `-f` is `noglob`, not `nosplit`: measured, word splitting still happens under
  it, so modelling it as suppressing field splitting would be wrong.
- [x] `-e` and `-o pipefail` are modelled. `Operation::Sequence` carries a
  `SequenceFailure`, which `set -e` selects and the runner honours by stopping
  after a failing statement; `PipelineStatus::Pipefail` was already in the IR and
  is now selected by the option that names it. An unmodelled option disqualifies
  the whole `set` statement rather than only itself, so `set -euo pipefail` is
  still delegated: taking its `-e` and `pipefail` while dropping `-u` would
  change what the script does.
- [ ] Model `-u`, which is what OComment's `action.yml` still waits on — all five
  of its `run:` blocks open with `set -euo pipefail`, so the work above moves
  nothing there yet. This is two steps rather than one, and the first is not
  about `-u` at all:
  - [ ] Represent a default expansion. `TextPart` is `Literal | Variable |
    Argument`, so `${VALUE:-fallback}` has nowhere to go and is delegated today
    while `"$VALUE"` lowers natively. The exception table `-u` needs — `${x:-}`,
    `${x+}`, `${x-}`, `${x:?}` — is exactly the syntax that is missing, so the
    option cannot be modelled over an IR that cannot say what it excepts.
  - [x] Represent it. `TextPart::DefaultValue` carries `${name:-fallback}` and
    `${name-fallback}` as separate forms, since `:-` substitutes an empty value
    as well as an unset one.
  - [ ] Then `-u` itself — and the direction is the opposite of what it looks
    like. `TextExpression::evaluate` already fails on an undefined variable, so
    de-shell's default *is* `set -u` and the plain shell behaviour is the one it
    cannot express:

    ```
    bash -c 'echo "[${UNDEFINED}]"'           → []   exit 0
    bash -c 'set -u; echo "[${UNDEFINED}]"'   → unbound variable, exit 127
    de-shell, either way                      → error
    ```

    Being stricter than the source is safer than being looser, but it is still a
    difference, and a tool that reports observed differences should not be one of
    them. The default now substitutes an empty string and `Task::nounset` selects
    the refusing behaviour, carried to the expansion through the run context
    because an expansion is evaluated far from the task that set the option —
    unlike `-e`, which is a property of the statement list and belongs to the
    sequence.

    With `-e`, `-u` and `-o pipefail` all modelled, `set -euo pipefail` is
    accepted whole and no longer blocks anything. Measured against OComment's
    composite action, its five blockers changed from five copies of
    `shell builtin set` to three `dynamic expansion or control syntax` and two
    `shell compound syntax`: the wall that stopped every CI step is gone, and
    what is behind it is ordinary missing frontend coverage rather than
    unsettled semantics.
- [ ] `-e` needs one new IR value, not a call-graph analysis. Which commands
  `set -e` stops on is already the shape of the tree: the left of `&&`/`||`, an
  `if` condition and the operand of `!` each lower into their own node, so the
  only untested position is a statement of a sequence. A `Sequence` currently
  continues past a failure unconditionally; giving it the choice — the same shape
  `PipelineStatus` already has — is what the option selects. Shell function
  definitions, where the meaning would depend on the call site, are delegated
  before they reach the lowering, so the dynamic half does not arise. Measured
  against OComment's six workflows and composite action: 28 `run:` blocks, 6
  function definitions, and **zero** calls in a tested context.
  The first attempt at this was reverted. Adding the field makes the compiler
  name 49 sites, which is the point, but half of them are `|` patterns where an
  inserted field lands mid-alternative and the edit has to be made by hand.
- [ ] Do not treat `-e` as the easy one. Measured against bash 3.2.57, `set -e`
  stops on a command that is *not tested*, where tested means the left of
  `&&`/`||`, the condition of `if`/`while`/`until`, the operand of `!`, and every
  element of a pipeline but the last. It is also a property of the call site
  rather than of the code: a function body whose `false` aborts when called
  directly runs to completion when called as `f || true`. Lowering it to `?` per
  statement is observably wrong, and `set +e` … `set -e` pairs are how callers
  express "a non-zero exit is not a failure here".
- [ ] Pin the bash version in `deshell.lock` the way `nu` already is. `doctor`
  now reports the build each interpreter announces on the host, so the difference
  is at least observable:

  ```
  interpreter builds: bash=GNU bash, version 3.2.57(1)-release (arm64-apple-darwin26);
                      nushell=0.115.1; zsh=zsh 5.9 (arm64-apple-darwin26.0)
  ```

  Carrying it in the lock is the remaining half, and the open question is what to
  record: baking the init-time version makes the lock host-specific, while
  recording nothing leaves a delegated node pinned to `bash` with no build. macOS ships
  3.2 and Linux runners ship 5.x; the CI matrix spans both. A tool that claims
  equivalence has to say which interpreter it is equivalent to, and the `set -e`
  rules above were only measured on 3.2.
- [x] Generate a `Test`. Rust emits `i32::from(!(...))` and Go sets the running
  status from the condition; neither needs a helper. Verified end to end: a
  script that is only `[ -n "$VALUE" ]` lowers with zero delegated bytes,
  generates Rust that passes `clippy -D warnings`, and agrees with the shell on
  set, empty and unset.

  Emitting the import unconditionally was a defect this found: a plan made only
  of `Test` nodes starts no process, and `use std::process::Command` is then an
  unused import — a hard error under the very gate the generated Rust is checked
  with. The generator could produce code it could not itself accept.
- [ ] The end-to-end generator tests take 16s each and time out at 30s under
  parallel load, reporting `replacement build failed with exit 124`. A timeout
  that surfaces as a build failure is the same defect as a timeout that surfaces
  as `delegated`: the cause is not recoverable from the message.

- [ ] Give the shell options a region rather than a bool. `set -e` applies from
  where it is set until it is unset, and `Operation::Sequence` carries one
  `on_failure` for the whole list, so a file that turns it on and back off has no
  honest lowering. It currently delegates on that shape, which is correct but
  coarse: the statements above the change were lowerable.

  Splitting the sequence at each change is not enough on its own. Measured:

  ```
  A: set +e / false / set -e / true            shell: exit 0
  B: set +e / false / set -e / false / echo    shell: exit 1, no output
  C: set -e / false / set +e / echo after      shell: exit 1, no output
  ```

  With one outer sequence around the regions, `on_failure: continue` passes A and
  B and fails C — the first region stopped, and the outer one runs the next
  anyway. `on_failure: stop` fails A, since the first region's last statement
  exits 1 and that is indistinguishable from having been cut short. `combine`
  keeps the last statement's status and nothing else, so "this region ended" and
  "this region was stopped" arrive as the same value.

  The same question applies to subshells and `&&` chains, where the option's
  reach differs again. A bool records that an option was set; it cannot record
  where it applied, and anything downstream of a rangeless record has to guess.

  Found by the OComment maintainers, who hit the identical shape from the other
  side: a `valid: bool` on their scan report could say a file failed to parse but
  not how far the parse got, and every consumer of it defaulted to the optimistic
  reading. Theirs deleted source code.

- [ ] Separate "no decision" from `delegated`. A node delegated because `set` is
  unmodelled is a decision: the source was read and isolation was chosen. A node
  delegated because the parser timed out is the absence of one, and it is
  retryable where the first is not. Both currently surface as `delegated` with a
  blocker, so a plan cannot be read to tell them apart.
- [x] Redirection. `Operation::Redirect` and every `Redirection` form were
  already in the IR; the tokenizer refused `<`, `>` and `&` as control syntax
  before a simple command could carry them. `>`, `>>`, `<`, a single-digit
  descriptor and `N>&M` now lower natively; heredocs, `<>`, `>|` and an expanded
  target stay delegated.

  It did not move OComment's composite action, whose five blockers are unchanged
  at three `dynamic expansion or control syntax` and two `shell compound syntax`.
  Redirection was not what those five were waiting on.
- [x] `if COND; then BODY; fi`, with an optional `else`. The statement splitter
  breaks on `;` and newlines, so a branch arrives as several statements and had
  to be rejoined; a nested `if`, an `elif` or a missing arm falls through to
  delegation rather than being lowered from a guess.
- [ ] What OComment's composite action still needs, which is more than one
  feature. Its branches are not the shape that was just implemented:

  ```
  case "$2" in                                     — implemented
  if [ -n "${INPUT_BINARY_PATH}" ]; then           — implemented (`[` operators)
  if ! "${binary}" --version >"${version_file}"    — implemented (`!`)
  while [ "$2" = "${delimiter}" ]; do              — implemented
  if [ ... ] && [[ "${ACTION_REF}" == v* ]]        — `[[` is a bash extension
  expected="$(awk ...)"                            — lowers; the generator cannot
                                                     emit it
  ```

  The substitution case is done. The generated Rust now carries a map of
  shell-local names, an expansion consults it before the environment the way the
  shell resolves a name, and both `SetVariable` and `CaptureStdout` emit into it.
  The map and its lookup helper are emitted together: one without the other is a
  program that does not compile.

  The Go generator does the same: `deshellVars` holds the locals, `deshellLookup`
  consults them before the environment, and `strings` is imported only when a
  capture needs it, since Go rejects an unused import outright. Verified by
  generating, running `go vet`, building, and comparing output with the shell.

  `[` was the one that gated the most and is done: its string operators are
  modelled, measured against the builtin and the external utility, and recorded
  in `contracts/golden/test-builtin-semantics-v1.json`. These blocked three of six
  steps in a corpus with no `set` in it, and unlike the above they are missing
  implementation rather than unsettled semantics.
- [ ] Carry the thirteen `set` semantics cases into the golden corpus. The corpus
  contained no `set` at all, which is why none of this surfaced until the tool
  was pointed at a repository that was not its own.

## `==` against an enum is outside the exhaustiveness check

From the OComment session, on finding two of nine call sites left behind when a
variant was added:

> `match` は守ってくれるが `==` は守ってくれない。同じ型でも書き方で安全性が変わる。

`cargo xtask enum-equality` finds every `==` or `!=` against a variant of an
enum this repository declares, outside tests and comments, and reports the ones
whose type has more than two variants. With two, `!= A` is `== B` and there is
nowhere for a third answer to hide; with three there is, and the compiler stops
helping exactly where the question gets harder.

Twenty-six when the gate was written; zero now, and it fails rather than
reports.

The remedy was not rewriting each comparison but asking the question once, in
a method whose body is a `match`, so a variant added later does not compile
until somebody answers for it — `FindingKind::is_a_shell_file`,
`EvidenceStatus::is_verified`, `ReviewStatus::is_current`,
`OutputFormat::is_structured` and five more. `OutputFormat` is the one that
had already gone wrong: `Agent` was added to it after its comparison sites
were written, and each of them answered "not JSON" for the new form without
anybody deciding that it should.

## What one real `action.yml` still needs

Five `run:` blocks, 22 KiB, measured after each change rather than once.

| blocks the frontend delegates | |
| --- | --- |
| before this round | 5 |
| after | 3 |

The three that remain are not gaps. Each builds a `GITHUB_OUTPUT` delimiter
from `${RANDOM}` and `$$`, which are a new number and a process id — there is
no native expression for "the same random number", and de-shell delegating
them is the tool being right rather than behind. A native replacement would
pick a delimiter a different way, which is a change to the script and belongs
to `harden`.

The two that stopped being frontend blockers moved to the generation side, and
those are open:

- [ ] A composite action's `run:` block. The shell lowers; what has no target
  is the host rewrite. The workflow rewrite writes the generated program into
  the repository the workflow lives in and points at it with a local path,
  which works because it is the same repository. An action is consumed by
  other repositories, so a file beside it does not travel with it — whatever
  GitHub resolves such a path against. The replacement belongs in whatever the
  action already ships its executable through: OComment's downloads a signed
  release archive, so the program belongs in the archive and the call site
  stays a `run:`. That is a generator shape this repository does not have.

  Recorded as a boundary rather than as something unmeasured, because the
  OComment session pointed out that "not established here" invites somebody to
  establish it and turn the rewrite on.
- [ ] `DESHELL_BLOCKER_DUPLICATE_TARGET: multiple sources generate action.yml`.
  Two blocks in one file both want to generate into it. The plan has no way to
  say "two programs, one call site each", and refusing is the safe half of an
  answer.

Both are about where generated code goes, not about what the shell means. That
is a better place for this file to be stuck than where it was.

## A coverage number that improved when the tool got a fact wrong

Measured on OComment's `action.yml`, one file, five `run:` blocks, against
three builds of de-shell:

| de-shell | blocks still delegated |
| --- | --- |
| `b0e74d7` — `case` arms lower | 4 |
| `3108e8d` — an arm is a list of statements | 5 |
| `265fe32` — `case` patterns match | 5 |

The middle row is the fix for an arm whose body was several statements being
joined into a single command. Under `b0e74d7` the block containing

```sh
*)
  echo "::error::OComment failed with exit code ${EXIT_CODE:-2}; ..."
  exit "${EXIT_CODE:-2}"
  ;;
```

lowered, because the two statements became one `Exec` whose argv was the words
of both — and that one command lowered without complaint. Fixing it put the
`exit` back where it belongs, the dynamic status is refused, and the block is
delegated again.

So the number went up when the tool started telling the truth. Anyone reading
"four blocks left" as progress over "five blocks left" would have been reading
a defect. A migration oracle's own coverage figure is not a score: it moves for
two unrelated reasons, and only one of them is work getting done.

Three things follow, and they are open:

- [ ] Report coverage beside the count of guarantees each block rests on, so a
  block that got shorter because a claim got weaker does not read as a block
  that got closer.
- [ ] Give `deshell migrate plan` a way to say *why* a count changed between two
  runs against the same file — which blocks moved, and in which direction.
- [x] A status that is native over a domain. Settled by removing the domain
  rather than describing it: the shells disagree about a non-numeric status
  only because they are different shells, and a plan names the one its source
  runs under. `NonNumericStatus::Ends` carries that interpreter's answer — 255
  for bash and `/bin/sh`, 0 for zsh, measured — so a caller reading the status
  sees what it would have seen. The first version stopped with 70 and said the
  shells disagree, which the OComment session pointed out is an observable
  change of behaviour inside a `migrate`, and claiming less is not the same as
  doing something different. What is left unreproduced is the message bash
  writes, which names the interpreter's own path and a line number.

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
