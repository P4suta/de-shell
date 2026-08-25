# JSON-RPC v1

Adapters and internal agents exchange newline-delimited JSON-RPC 2.0 messages.
Protocol version 1 begins with `deshell.handshake`. Requests and responses are
limited to 4 MiB unless a method declares a smaller limit. Unknown methods use
JSON-RPC error `-32601`; malformed requests use `-32600`; invalid parameters use
`-32602`; incompatible protocol versions use `-32001`; oversized messages use
`-32002`.

Unlike persisted JSON documents, JSON-RPC envelopes and method payloads ignore
unknown fields. Duplicate JSON keys remain invalid. A peer must treat EOF,
timeout, malformed UTF-8, and a response ID mismatch as protocol failures.
