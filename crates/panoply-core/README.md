# panoply-core

The small, dependency-light pieces every platform shares: file sniffing, never-overwrite output naming, the cancel flag, job and event types, the download consent gate, `move_to_trash` (the Trash on macOS and Linux, the Recycle Bin on Windows, never a permanent delete), and the performance budgets as constants.

Every other crate builds on this one, and so will the native mobile apps, so it stays small on purpose. A new
dependency here needs a reason.

```sh
cargo test -p panoply-core
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
