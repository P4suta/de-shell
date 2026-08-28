# Effect IR v1

Effect IR v1 is defined by `schema/effect-ir-v1.schema.json`. Every dynamic
text position is a `textExpression` containing explicit `literal`, `variable`,
or `argument` parts. Implementations evaluate these parts exactly once and must
never reinterpret the resulting text as shell expansion syntax.

`variable` refers to declared environment or task-local runtime state.
`argument` refers to a named task input. Named expression arrays preserve
declaration order and duplicate names are invalid.

## Values and paths

Public structured types are UTF-8 `text`, `bool`, signed 64-bit `int`, a
normalized project-relative `path`, and recursively wrapped `secret<T>`.
Booleans use `true` or `false`; integers use canonical base-10 spelling. Path
separators are `/`, and empty components, `.`, `..`, NUL, drive prefixes, and
absolute paths are invalid.

## Source coordinates and node IDs

Source byte spans are zero-based and half-open. Lines are one-based. Columns
are zero-based counts of Unicode scalar values. The end line and column identify
the position immediately after the span.

Node IDs are the lowercase hexadecimal encoding of the first 128 bits of
SHA-256 over this byte sequence:

1. ASCII `deshell.node-id.v1`, followed by NUL;
2. each of normalized source path and operation name, encoded as an unsigned
   64-bit big-endian byte length followed by its UTF-8 bytes;
3. start byte, end byte, and preorder number, each unsigned 64-bit big-endian.

Preorder starts at zero for a frontend result and increments before visiting
children from left to right. Generated nodes use an empty source path and a
zero-width byte span. Operation names are the schema `type` strings.

## Guarantees, delegation, and residual source

Nodes carry only `native`, `delegated`, or `residual` guarantees. `native`
identifies the pinned semantic model that fully lowered the node. `delegated`
is represented only by an `interpreter_call` containing the interpreter digest,
exact source bytes and span, minimum capabilities, and delegation reason.
Differential results belong to Evidence v1 and never mutate a plan. An
`opaque_capsule` is residual-only and is never executable; its original bytes
are retained as either UTF-8 text or canonical base64.
