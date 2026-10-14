# panoply-core

The small, dependency-light pieces every platform shares: file sniffing, never-overwrite output naming, the cancel flag, job and event types, the download consent gate, and the performance budgets as constants.

Every other crate builds on this one, and so will the native mobile apps, so it stays small on purpose. A new
dependency here needs a reason.

```sh
cargo test -p panoply-core
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
