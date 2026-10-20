# de-shell

`de-shell` is a Rust shell retirement migration oracle.
It inventories shell automation, compares supported behavior through Effect IR v1, and verifies project-native replacements before retiring scripts.
The 0.1.0 code is a development version with a protected release qualification gate.

## Build from source

Use the pinned Rust 1.98 toolchain:

```sh
mise trust && mise install
mise run build
```

## Start

```sh
./target/debug/deshell init
./target/debug/deshell check
./target/debug/deshell verify --require shell-free
```

`init` inventories the project and records its target without executing source scripts.
The [usage guide](docs/usage.md) covers approvals, migration, protocols, supported behavior, and verification commands.
See [corpus audits](docs/corpus-audit.md), [contribution rules](CONTRIBUTING.md), and [the roadmap](ROADMAP.md) for further detail.

Licensed under [Apache-2.0](LICENSE).
