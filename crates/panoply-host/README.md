# panoply-host

The module host. A module declares a manifest and does one job; the host gives it a cancel token, a progress sink and a capability guard. Built-in modules live here too.

The guard is the sandbox contract: a module reads only the files the user chose and writes only inside the
chosen output folder. `panoply-wasm` enforces the same contract for community modules, so built-in and
community modules behave the same.

- `Module` and `Registry` are the public surface.
- `src/builtin/` has the built-in modules, each a thin wrapper over an engine crate.

```sh
cargo test -p panoply-host
```

To add a module, see [CONTRIBUTING.md](../../CONTRIBUTING.md#adding-a-module).

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
