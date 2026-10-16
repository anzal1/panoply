# panoply-platform-windows

File change events on Windows for Panoply's instant search: the NTFS USN change journal, with
`ReadDirectoryChangesW` as the fallback, behind a safe API.

This is the only crate in the workspace allowed to use `unsafe` on Windows. Its `Cargo.toml` sets
`unsafe_code = "allow"` instead of inheriting the workspace `forbid`, and every unsafe block carries a
`// SAFETY:` comment. Callers never see a handle or a raw pointer.

The system calls compile on Windows only. The byte decoders (USN records, directory change records, path
helpers) have no dependencies and build and test everywhere, so the workspace builds on every platform.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
