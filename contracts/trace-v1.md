# Execution trace v1

`--trace off` records nothing and is the default: a trace is a thing somebody
asked for. `--trace jsonl` writes one compact JSON object conforming to
`schema/trace-v1.schema.json` per line, in the order the events happened.
`--trace-output PATH` names the destination; without it the trace goes to
stderr. The option never changes stdout, generated artifact bytes, or an
executed plan's stdout/stderr bytes, and `--trace-output` without `--trace` is
a usage error rather than a request that quietly does not happen.

Trace v1 answers a different question from `diagnostics-v1.md`. Diagnostics
explain a failure to somebody who hit one, and say nothing about a run that
succeeded. A trace says what the run *did*: which files it staged and
committed, with which digests, in which order; which environment variables it
read and whether they were set; which processes it started, with the exact
argv, and how each ended.

## The vocabulary is closed

| Event | What it records |
| --- | --- |
| `directory_create` | A directory was established, and whether this call created it |
| `file_stage` | Bytes were staged beside their destination, before any commit |
| `file_commit` | A staged file replaced its destination — the rename itself |
| `file_remove` | A file was removed |
| `file_rollback` | A commit was undone and the destination restored |
| `environment_read` | A variable was read: its name, and whether it was set |
| `clock_read` | The wall clock was read |
| `digest` | A digest was computed: how many bytes, and the answer |
| `process_start` | A process was started, with the exact argv |
| `process_exit` | A process ended; `code` is absent when a signal ended it |
| `approval_decision` | A review was decided: its subject, the digest reviewed, and whether it stands |
| `provider_select` | A disposable provider was chosen, or refused for want of one |

`cargo xtask trace-events` holds this table, `trace::Event` and
`schema/trace-v1.schema.json` equal, in that order, so a reader can decode a
trace against the schema alone.

## No event carries a value read from outside

An environment variable contributes its name and whether it was set, never its
value. A file contributes its path, its length and its digest, never its
contents. A process contributes its argv and its exit code, never its streams.
The clock contributes that it was read, never what it said.

This is a property of the vocabulary rather than of each call site, so it holds
for a caller who has not thought about it. A trace that could carry a secret
would have to be handled like one, and then nobody would turn it on.

## Two runs of the same work record the same trace

Every field except `elapsed_nanos` is deterministic, which is why the elapsed
time is a field of its own rather than folded into the event. de-shell's
deterministic-output contract is a claim about the bytes a command writes;
comparing two traces checks the same property one layer down — the same files
staged in the same order with the same digests, after the same readings of the
environment.

Paths are recorded canonically where the path resolves, so one tree is one name
within a trace.

## Where the events come from

The filesystem events come from the transactional layer, the environment and
clock events from the ambient-input layer, the process events from the launch
layer, the approval decisions from the one function that produces a review
status, and the provider selections from the one function that chooses one. `clippy.toml` makes each of those the only route to what it
describes, so a new call site is traced because it compiles rather than because
somebody remembered to add a line.
