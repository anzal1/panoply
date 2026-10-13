# 0002. Permissive licenses only, for code and models

- Status: accepted
- Date: 2026-10-13

## Context

Panoply is open source and meant to be embedded, forked and shipped anywhere, including app stores. GPL and AGPL engines (x264, x265, gifski, MuPDF, Ghostscript, Rubber Band) and models with non-commercial or custom terms would block that.

## Decision

Panoply is released under MIT OR Apache-2.0. No GPL, LGPL-linked or AGPL code in the core; LGPL ffmpeg only as a dynamically loaded build for demuxing. Models must carry a clean license (MIT, Apache-2.0, BSD and similar) on the weights. cargo-deny and a model gate enforce this in CI.

## Consequences

We write some pieces ourselves (GIF quantizer, SSIM) and use OS encoders for H.264, HEVC and HEIC. A few popular models are off limits (RMBG, MobileCLIP, Parakeet, Jina).
