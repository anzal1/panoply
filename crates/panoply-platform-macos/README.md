# panoply-platform-macos

HEIC, HEIF, AVIF and every other format ImageIO reads, with HEIC and AVIF encode, camera RAW through CoreImage, and the Vision engines, behind a safe API.

This is the only crate in the workspace allowed to use `unsafe`. Its `Cargo.toml` sets `unsafe_code = "allow"`
instead of inheriting the workspace `forbid`, and every unsafe block carries a `// SAFETY:` comment. Callers
never see a raw pointer, a Core Foundation object or an Objective-C type.

The crate is empty on every platform except macOS, so the workspace builds everywhere.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
