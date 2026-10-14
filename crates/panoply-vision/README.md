# panoply-vision

The image AI modules: remove background, erase, remove watermark and upscale. Small ONNX models, OS engines first.

A small model manager downloads each pinned model on first use, after saying how big it is, checks its SHA-256
and caches it per user. Nothing is downloaded at build time and nothing ships in the binary. Inference runs
through ONNX Runtime, loaded at run time from a pinned release, with CoreML on macOS, DirectML on Windows and the
CPU everywhere else. A model is unloaded after 60 seconds without use.

```sh
cargo test -p panoply-vision
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
