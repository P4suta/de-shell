# Canonical and persisted JSON v1

Digest input follows RFC 8785 (JSON Canonicalization Scheme): no insignificant
whitespace, recursively sorted object member names using UTF-16 code units, and
the JSON string escaping rules used by ECMAScript. de-shell contract documents
further restrict numbers to signed 64-bit integers, so floating-point and
non-finite number serialization is outside the contract.

Persisted JSON uses the same member ordering and primitive spelling, two ASCII
spaces per indentation level, UTF-8 without a BOM, and exactly one final LF.
Arrays retain semantic order. Duplicate object names are rejected before typed
decoding. Unknown fields are rejected except in JSON-RPC envelopes and payloads.

SHA-256 digests are lowercase, 64-character hexadecimal strings over canonical
bytes, never over pretty persisted bytes.
