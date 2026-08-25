# Project, scenario, and lock contracts v1

`.deshell/project.toml` version 1 declares entrypoints, runtime policy, a
disposable sandbox, and strict export settings. `.deshell/scenarios/*.toml`
declares ordered arguments, name/value environment entries, fixtures, timeout,
and expected results. Duplicate names and unknown fields are invalid.

`deshell.lock` version 1 pins protocol version, Effect IR v1, command-model and
adapter digests, interpreter identities, and a digest-pinned lab image. It is a
fresh 0.1 contract; old lock layouts are not migrated.

All project-relative paths use `/` and reject traversal, absolute paths, drive
prefixes, NUL, empty components, and symlink escape at filesystem boundaries.
