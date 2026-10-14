# Changes made by Panoply

This is a vendored copy of `gpui-base` from GPUI Kit
(github.com/longbridge/gpui-kit), licensed under Apache-2.0 (see
LICENSE-APACHE). Panoply modified it:

- The text input moves the caret, extends selection and deletes by grapheme
  cluster instead of by code point, so Indic conjuncts such as क्ष and त्र are
  never split.
- Committing composed text through `unmarkText` notifies listeners, so search
  results refresh after an input method commits.

The patch is also kept as a diff in
`prototypes/shell-test/gpui/kit-variant/patches/` and is intended for upstream.
- The unmaintained `instant` crate is gone (RUSTSEC-2024-0384). Panoply builds for desktops only, where
  `instant::Instant` and `instant::Duration` are the `std::time` types, so the seven files that used
  it now import `std::time` directly and the dependency is removed from `Cargo.toml`.
- The `[dev-dependencies]`, `[[test]]` and `[[bench]]` tables are removed from `Cargo.toml`, and the
  `tests/` and `benches/` folders are deleted. Nothing in Panoply builds or runs them, and the
  dev-dependencies pulled a second GPUI, an HTTP client and the unmaintained `rustls-pemfile`
  (RUSTSEC-2025-0134) into the lock file. `Cargo.toml.orig` still shows the upstream manifest.

- `Cargo.toml` sets `test = false` on the library, because its unit tests need dev-dependencies
  that were removed from this vendored copy.
