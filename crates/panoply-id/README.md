# panoply-id

ID, forms and document photos: passport and visa photo, print sheet, and signature cleaner.

Faces come from the OS engine where there is one (Apple Vision on macOS) and from the small YuNet model through
ONNX Runtime elsewhere, downloaded once with a pinned checksum after asking. The pixel work (crop, tilt,
background, thresholds) is Panoply's own, on the `panoply-image` pipeline. Specs and country rules are in
[docs/specs/id-and-image.md](../../docs/specs/id-and-image.md).

```sh
cargo test -p panoply-id
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
