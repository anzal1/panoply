# panoply-ffi

A placeholder for the UniFFI bindings that will expose the core, host and convert crates to Swift (macOS, iOS) and Kotlin (Android).

It has no dependencies on purpose, so nothing heavy links into an app before the binding surface is designed.
The desktop app does not use it. It calls the Rust core directly. See [apps/README.md](../../apps/README.md).

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
