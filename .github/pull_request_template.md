## Summary

Describe the user-visible behavior and why the change is needed.

## Evidence

- [ ] I added a failing user-facing or adapter-contract test first.
- [ ] I ran `mise run fmt:check`.
- [ ] I ran the relevant fast/contract/platform/security gate.
- [ ] I ran `mise run test` and `mise run package`, or explained why not below.
- [ ] I added negative, boundary, and failure-path coverage where applicable.
- [ ] I updated documentation, schemas, snapshots, and source maps where applicable.
- [ ] I reviewed the change for secret exposure and new host/network capabilities.

## Behavioral compatibility

State whether this is an equivalent rewrite, an explicit modernization, or a
breaking change. For compiler changes, identify the affected guarantee level
and any new residual reason.

## Verification notes

List exact commands, platforms, fixtures, and any intentionally deferred release
gate.
