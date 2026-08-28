# JSON-RPC v1

Adapters and internal agents exchange newline-delimited JSON-RPC 2.0 messages.
Protocol version 1 begins with `deshell.handshake`. Requests and responses are
limited to 4 MiB per frame unless a method declares a smaller limit. Binary
stdout and stderr that would exceed one response frame are sent as ordered
`deshell.stream` notifications containing canonical base64 chunks of at most
256 KiB, followed by a final response that binds each stream's byte count and
SHA-256 digest. Receivers reject missing, duplicated, reordered, oversized, or
digest-mismatched chunks; no silent truncation is permitted. Unknown methods use
JSON-RPC error `-32601`; malformed requests use `-32600`; invalid parameters use
`-32602`; incompatible protocol versions use `-32001`; oversized messages use
`-32002`.

Unlike persisted JSON documents, JSON-RPC envelopes and method payloads ignore
unknown fields. Duplicate JSON keys remain invalid. A peer must treat EOF,
timeout, malformed UTF-8, and a response ID mismatch as protocol failures.
