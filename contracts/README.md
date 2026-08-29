# de-shell public contracts

This directory is the language-neutral source of truth for de-shell 0.1.x.
The supported public surface is the `deshell` command line, bytes written by
that command, the JSON Schemas in `schema/`, and JSON-RPC v1. Rust modules and
OCaml modules are implementation details and are not stable APIs.

All non-RPC JSON documents reject duplicate keys and fields not declared by the
relevant schema. Digests use the canonical representation specified in
`canonical-json-v1.md`; persisted documents use sorted keys, two-space
indentation, and one trailing LF. JSON-RPC objects ignore unknown fields for
forward compatibility. Every embedded Schema is self-contained so a caller can
validate artifacts offline using only the bytes returned by `deshell schema`.

`scan --format json` emits the versioned Inventory v1 document described by
`schema/inventory-v1.schema.json`; scan omissions and failures are explicit.
Bundle exports contain a `bundle-manifest.json` conforming to
`schema/bundle-v1.schema.json` and bind every embedded file by size and SHA-256.
They retain every active manifest entry, identify the selected entrypoint, and
archive runtime assets at the exact project-relative paths named by the lock.

The migration oracle is a development and CI tool, not a production runtime or
a replacement DSL. It lowers shell locations into Effect IR v1 and asks a
digest-pinned generator for ordinary project-native code. The generator sees a
minimal Migration Request v1 and returns a Proposal v1; it cannot delete the
source, write the archive, or mutate the live workspace. `migrate plan` binds
all requests and proposals into a content-addressed Migration Plan v1.

Migration Evidence v1 independently compares the original source, native IR,
and replacement for every approved scenario and platform cell. Only the core
can atomically apply a complete plan, archive retired bytes under an
Archive Manifest v1, and remove the live shell. Audit Finding v1 is the shared
finding shape for human, JSONL, SARIF, and GitHub annotation output. Intentional
semantic changes use the separate Harden Plan, Harden Approval, and Harden
Evidence v1 series and are never accepted as migration equivalence.

The contract starts at v1. There is no published pre-v1 Effect IR or migration
format, and pre-release local project formats are not migration inputs.

`golden/frontend-v1.json` is shared by the Rust implementation and the
unpublished OCaml reference for lowering, guarantees, node IDs, and canonical
plan digests. `golden/transform-export-v1.json` likewise fixes equivalent
rewrites, modernization output, and byte-level exporter digests.
