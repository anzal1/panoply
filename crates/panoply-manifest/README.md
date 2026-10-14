# panoply-manifest

The `module.toml` version 0 schema and its validation.

One `module.toml` per module declares inputs, options, outputs, capabilities and the surfaces it appears on.
Every platform renders the options natively, and the CLI and MCP tools are generated from the same file, so a
module never ships UI code. Examples are in [modules/examples/](../../modules/examples/).

```sh
cargo test -p panoply-manifest
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
