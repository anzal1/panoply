# panoply-ocr

OCR with an engine ladder, word boxes with confidence, and the searchable-PDF text layer writer.

The ladder is OS first, own model second ([ADR 0004](../../docs/adr/0004-os-engines-first.md)): Apple Vision on
macOS, `Windows.Media.Ocr` on Windows, then PP-OCRv5 on ONNX Runtime for Latin and Devanagari script on every
platform, and Tesseract when it is installed, for the rest (Bengali). Models are downloaded once, after asking.

Labelled test pages in `testdata/labelled/` come with their expected text, so accuracy is measured, not guessed.

```sh
cargo test -p panoply-ocr
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
