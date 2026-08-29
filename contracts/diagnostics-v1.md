# Machine diagnostics v1

`--diagnostics human` writes human-readable diagnostics to stderr.
`--diagnostics jsonl` writes one compact JSON object conforming to
`schema/diagnostic-v1.schema.json` per stderr line. The option never changes
stdout, generated artifact bytes, or an executed plan's stdout/stderr bytes.
Diagnostics include actionable `help` and typed `next_actions`: command actions
carry exact argv arrays and review actions carry exact project paths.

A completed report that is not ready, blocked by policy, different, or missing
a requested capability is still a report: it is written to stdout, uses the
corresponding exit code, and leaves stderr empty. Diagnostics are reserved for
command-line syntax, I/O/execution failures, invalid contracts, and internal
failures. Scan read failures are the deliberate exception: they are represented
inside the completed scan report and exit 1.

Normal command exit categories are fixed:

| Code | Category |
| ---: | --- |
| 0 | success |
| 1 | execution or I/O failure |
| 2 | command-line usage error |
| 3 | invalid configuration or Effect IR |
| 4 | policy refusal |
| 5 | observed difference |
| 6 | requested provider unavailable |
| 70 | internal invariant violation |

After `run` starts a plan, the plan's exit code is returned unchanged.
