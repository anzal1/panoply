# panoply-wasm

Runs community modules, compiled to WebAssembly components, inside a sandbox on wasmtime.

A community module is a `module.toml` plus a `.wasm` component built from `wit/module.wit`. It registers, lists
and runs exactly like a built-in module. The sandbox has no ambient authority: no paths, no folders, no network
(a component that imports `wasi:sockets` or `wasi:http` is refused before it runs), and writes go through the host,
which picks a name that never overwrites.

Module projects are standalone and build for `wasm32-wasip2`. Start from [modules/](../../modules/) and the
decision in [ADR 0007](../../docs/adr/0007-declared-modules-wasm.md).

```sh
rustup target add wasm32-wasip2
cargo test -p panoply-wasm
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
