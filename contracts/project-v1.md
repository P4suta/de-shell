# Project, scenario, and lock contracts v1

`.deshell/project.toml` version 1 declares all entrypoints, a capability policy,
disposable-by-default execution, fixed resource ceilings, and strict/bundle
export policy. CLI options may narrow these settings but may not broaden them.
Local execution requires both `sandbox.allow_local = true` and
`run --backend local`.

`.deshell/scenarios/*.toml` declares ordered argv, named arguments and
environment, binary-safe stdin and fixtures, an optional project-relative cwd,
resource-limit narrowing, and fully optional expected exit/stream/file results.
Duplicate names and unknown fields are invalid.

`deshell.lock` version 1 pins protocol version, Effect IR v1, command-model and
adapter digests, interpreter identities, target-specific runtime images, a
digest-pinned lab image, and optional OS/architecture-specific lab assets. It
is a fresh 0.1 contract; old lock layouts are not migrated.

`.deshell/manifest.json` maps each configured entrypoint to immutable
`.deshell/artifacts/<source-sha256>/<plan-sha256>/{plan,evidence}.json` files.
Artifact reads revalidate the plan digest, exact current source digest, Evidence
node inventory, and every non-symlink path component. Reanalysis atomically
switches the manifest and does not overwrite prior Evidence.

`deshell check` additionally rebinds every delegated node against the current
interpreter lock and verifies the bytes of every declared lab asset. A valid
digest string without the corresponding exact regular, non-symlink file is not
a valid project contract.

All project-relative paths use `/` and reject traversal, absolute paths, drive
prefixes, NUL, empty components, and every symlink component at filesystem
boundaries, including a symlink that points back inside the project.
