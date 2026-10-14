# Third-party notices

Panoply is licensed under MIT OR Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). It is built on the
open-source software and data listed here. This file ships inside every Panoply download. It is generated
by `tools/notices/generate.py` from Cargo.lock, so the list of Rust crates always matches the build.

What is in this file:

1. Credits that the licenses ask us to show
2. Code that is copied into this repository
3. Rust crates (generated)
4. Software and models downloaded at run time
5. Software you install yourself

## 1. Credits

- This software is based in part on the work of the Independent JPEG Group. This applies to the JPEG
  encoder (`jpeg-encoder`) and to MozJPEG and libjpeg-turbo (`mozjpeg-sys`), which are IJG licensed.
- libwebp, Copyright (c) Google Inc., is BSD-3-Clause and is built into the WebP encoder (`libwebp-sys`).
  Its license and patent grant are in section 3.3.
- rav1e (AVIF encoding) is BSD-2-Clause with the Alliance for Open Media patent license. Both texts are in
  section 3.3.
- Icons in the app are from Lucide (ISC License, Copyright (c) 2026 Lucide Icons and Contributors). Some
  Lucide icons derive from Feather (MIT License, Copyright (c) 2013-present Cole Bemis). The standard
  ISC and MIT texts are in section 3.3 under the crates that use them. The Lucide license is at
  https://github.com/lucide-icons/lucide/blob/main/LICENSE.
- The character lists in `crates/panoply-ocr/data/*.dict` are the lists the PP-OCRv5 recognizers decode with.
  PP-OCRv5 is by PaddlePaddle and is Apache-2.0 (https://github.com/PaddlePaddle/PaddleOCR).
- Two crates are MPL-2.0 (file-level copyleft): `option-ext` and `dwrote`. Panoply does not change them. Their
  source is available unchanged at https://crates.io/crates/option-ext and https://crates.io/crates/dwrote,
  and the MPL 2.0 text is printed in section 3.3. The Mozilla Public License covers only their own files.
- The Apache-2.0 license asks that NOTICE files travel with the software. The generator prints every NOTICE
  file a dependency ships in section 3.3 under "Notice text", before the license texts. None of the
  dependencies in this build ships one, so there are none to print.

## 2. Code copied into this repository

**gpui-base 0.7.3**, from GPUI Kit (https://github.com/longbridge/gpui-kit), Copyright 2024 - 2026
Longbridge, licensed under the Apache License 2.0. It lives in `apps/desktop/patches/gpui-base/` with its
original LICENSE-APACHE. Panoply changed it, as Apache-2.0 section 4(b) requires us to say:

- The text input moves the caret, extends the selection and deletes by grapheme cluster instead of by code
  point, so Indic conjuncts are never split.
- Committing composed text through `unmarkText` notifies listeners.
- The unmaintained `instant` crate is replaced by `std::time`, and the unused dev-dependencies, tests and
  benchmarks are removed.

The full list is in `apps/desktop/patches/gpui-base/PANOPLY-CHANGES.md`. The Apache License 2.0 text is
printed in section 3.3. GPUI itself (the `gpui-pre` crates) is Apache-2.0 and is listed in section 3.

