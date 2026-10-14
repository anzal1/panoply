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
