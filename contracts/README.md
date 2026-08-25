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

The contract starts at v1. There is no published pre-v1 Effect IR and no
migration contract.

`golden/frontend-v1.json` is shared by the Rust implementation and the
unpublished OCaml reference for lowering, guarantees, node IDs, and canonical
plan digests. `golden/transform-export-v1.json` likewise fixes equivalent
rewrites, modernization output, and byte-level exporter digests.
