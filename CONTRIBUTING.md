# Contributing to de-shell

All production changes follow outside-in TDD.

1. Add a failing CLI or adapter contract that describes the user-visible
   behavior.
2. Decompose it into focused unit, golden, property, differential, or security
   tests where useful.
3. Implement only enough behavior to make the test pass.
4. Refactor while the full relevant suite remains green.
5. Add negative, boundary, and failure-path coverage before considering the
   change complete.

Every bug fix starts with a minimized reproduction. Experimental spikes belong
under `experiments/` and must be reimplemented test-first before becoming
production code.

## Local workflow

```console
mise install
mise run setup
mise run lint
mise run test:fast
mise run fmt
mise run fmt:check
mise run test
mise run package
```

Pull requests use the repository template and require the stable `Required gate`
check. Reviews are dismissed after new commits; code-owner review, last-push
approval, resolved threads, signed commits, and linear history are enforced on
the default branch. The owner bypass is limited to pull requests so emergency
solo-maintainer work still remains reviewable in GitHub's audit trail.

GitHub repository settings and Rulesets are versioned under `.github/settings`
and `.github/rulesets`. Maintainers can reconcile them with:

```console
mise run github:apply
mise run github:verify
```

The apply task changes remote repository settings and therefore requires an
authenticated `gh` session with repository administration permission.

GitHub does not offer push-target Rulesets to public repositories owned by a
personal account. The canonical capability record in
`.github/settings/capabilities.json` preserves that residual reason. The required
CI gate enforces the intended 10 MiB file-size and 240-character path limits via
`mise run repository:guardrails`; if the repository is transferred to an
organization, maintainers should reevaluate native push Rulesets.

The repository pins opam, Rust, PowerShell, and actionlint through mise. Do not
replace those tools with unpinned host versions in tests or release scripts.
Build and test commands deliberately use one OCaml/Rust job so the same workflow
remains reliable on constrained Windows, Linux, and macOS runners.

After changing `de-shell.opam`, regenerate the cross-platform direct lock with
`mise exec -- opam lock --direct-only . --switch=.`, review
`de-shell.opam.locked`, and rerun `mise run setup`.

Do not update golden output implicitly. Snapshot changes need an explicit update
mode and reviewable diff. Do not skip unstable tests; make them deterministic
with fixtures, record/replay, or an injected clock.

Rewrites require positive, negative, idempotence, source-map, and behavioral
equivalence tests. New adapters must pass the shared protocol contract,
including version mismatch, malformed response, disconnect, size limit, and
unknown-field behavior.

`mise run package` is also a contract: it must leave the CLI, observer/process
agents, PowerShell and Nushell adapters, and all schemas in Dune's install tree.
Adding a runtime component requires extending that payload check first.
