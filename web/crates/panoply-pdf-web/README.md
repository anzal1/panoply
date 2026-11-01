# panoply-pdf

The PDF engine behind the Panoply modules `pdf.merge`, `pdf.split`, `pdf.organize`,
`pdf.images_to_pdf`, `pdf.to_images` and `pdf.compress` (specs: `docs/specs/pdf.md`, the compress worked
example in `docs/FLOWS.md`, licensing in `docs/adr/0002`). The host wraps it in
`crates/panoply-host/src/builtin/pdf/`; the command line is `panoply pdf <merge|split|pages|compress|from|images>`.

## What is in it

| module | job |
|---|---|
| `doc`, `source`, `obj`, `filters` | Lazy reader. Opening reads the end of the file, the cross-reference data and the trailer. Objects are parsed on demand with positioned reads, so a whole file is never loaded. Encrypted files are reported, never decrypted. Damaged files are repaired in memory (own scan, then lopdf, then pdfium) and the file on disk is untouched. |
| `writer`, `copy` | Streaming writer (objects go to the output as they are produced, small ones are packed into object streams behind a cross-reference stream) and a copier that walks references from the pages you ask for, so nothing unreachable is written. |
| `pageops`, `ranges`, `dests` | Merge, split, extract, reorder, delete and rotate are one operation with a list of pages. Bookmarks, named destinations, form fields and links are carried or rebuilt. |
| `images_in` | Images to PDF. A JPEG is stored byte for byte. A PNG that PDF can carry is stored with its own compressed data. |
| `render`, `pdfium` | Pages drawn by pdfium in strips (never a bitmap over 64 MB), PNG written strip by strip. |
| `compress` | See below. |
| `jpeg`, `resample`, `ssim` | Own SSIM, area-averaging and bilinear resamplers, JPEG header parser, zune-jpeg to decode and jpeg-encoder to encode (both pure Rust). |

## Compress

One pass copies the document through the copier with a policy that recodes streams on the way.

- `compress::analyze` looks at the document first: every image and its stored size, the highest resolution
  each image is shown at (it follows page content streams and the forms they draw, tracking the matrix),
  which images are masks, bytes of embedded fonts (full fonts versus subsets), and exact duplicates of
  streams and of shared font, descriptor, graphics state and encoding dictionaries. Duplicates are written once.
- Images are shrunk to the resolution the page shows (at most 150 dpi by default) and stored as JPEG at the
  lowest quality whose SSIM against the original stays at or above 0.97, or stored again losslessly. An image
  that does not get at least 5% smaller is copied untouched. Masks are only ever recoded without loss.
- Other streams are deflated better, objects are packed into object streams, metadata, attachments and form
  data can be stripped.
- Targets: smallest that still looks good, a fixed quality, or under a size. The size mode estimates from a
  sample of the images which setting fits, confirms with whole-document passes and reports when the target
  cannot be reached.
- Never inflate: the engine reports the saving, and the module keeps the original (no file written) when it is
  under 2%.

What it does not do: recode CCITT pictures (already Group 4), recode a JBIG2 picture, or beat JPEG 2000 with
JPEG on a picture that is not oversampled and not a mask layer (JPEG needs more bytes at the same SSIM, so it
is left alone). Without pdfium (no library, no network for the one-time download) JPEG 2000 and JBIG2
pictures are left alone and counted as `NoEngine` in the report. Encrypted files are refused, and a signed
file is compressed with a note that the copy is not signed. Measured results, losses included:
`docs/research/pdf-compress-results.md`.

## pdfium

pdfium is loaded at run time through pdfium-render, never linked and never committed. Drawing needs it, and
compress needs it for JPEG 2000 and JBIG2 pictures only; merge, split and organize never load it, and compress
does not load it for a file with neither codec. The first job that needs it downloads the prebuilt
library from `github.com/bblanchon/pdfium-binaries` at release `chromium/7881`, checks its SHA-256 against a
pin in `src/pdfium.rs`, and caches it outside the repository:

| OS | architectures pinned |
|---|---|
| macOS | arm64, x64 |
| Windows | x64, arm64 |
| Linux | x64, arm64 |

Cache folder: `PANOPLY_ENGINES_DIR`, else `~/Library/Caches/Panoply/engines` (macOS), `%LOCALAPPDATA%\Panoply\cache\engines`
(Windows), `$XDG_CACHE_HOME/panoply/engines` or `~/.cache/panoply/engines` (Linux). `PANOPLY_PDFIUM_LIB` points
at a pdfium library and skips the download. The download uses the system `curl` and `tar`, which ship with
Windows 10 and later, macOS and mainstream Linux. The pins come from the release's published digests; the
macOS arm64 archive was also checked against a download.

pdfium is not safe to call from two threads at once, so every call into it holds one process-wide lock
(`pdfium::lock`). A test binary that drew on two threads crashed within a few runs before the lock.

## Platforms

The crate has no OS-specific code besides positioned file reads (`pread` on Unix, `seek_read` on Windows, in
`source.rs`) and the engine cache path. `cargo check -p panoply-pdf` passes for `x86_64-unknown-linux-gnu` and
`x86_64-pc-windows-msvc` from macOS (checked, not run). The host crate was not cross-checked for Linux and
Windows because it also pulls in `panoply-image`, whose C dependencies (libdeflate, MozJPEG) need a target C
toolchain. The PDF modules in the host use `std` only.

## Budgets (FLOWS.md), measured with `panoply-bench`

Apple M5 Max, release build without LTO, 44 default-tier corpus PDFs (35 open, 8 are encrypted and 1 is not a
PDF, so those 9 return plain errors), median of runs. Wall time includes starting the process and the registry.

| module | wall p50 / p95 / max (ms) | peak memory max | first preview, 100 ms (files up to 10 MB) | 300 MB file |
|---|---|---|---|---|
| pdf.merge | 10 / 33 / 92 | 15.8 MB | pass, slowest 92 ms | 211 ms, 3.6 MB |
| pdf.split (every page) | 10 / 790 / 12,205 (the 3,000-page file makes 3,000 parts) | 14.8 MB | not an instant module | 1.2 s (300 parts), 3.6 MB |
| pdf.organize (reverse, via the CLI) | 10 / 31 / 161 | 25.1 MB | fail on one file: 161 ms on the 6 MB tagged NIST document, the other 43 files pass | 96 ms, 4.0 MB |
| pdf.compress | 10 / 81 / 677 | 26.1 MB | tuned transform, not timed as instant | 5.0 s, 11.0 MB, 56.5% smaller |
| pdf.to_images (every page, 150 dpi PNG) | 30 / 4,235 / 20,714 | 120.4 MB | not an instant module | first 10 pages: 152 ms, 35.8 MB |
| pdf.images_to_pdf (67 image corpus files) | 6 / 12 / 23 | 27.9 MB | pass, slowest 23 ms | not applicable |

Peak memory stays under the 300 MB cap everywhere and is flat across a 315 MB input. Speed against qpdf was not
measured because qpdf is not installed here. The organize miss is Full catalog mode (reverse and rotate keep
the whole catalog, including the structure tree) on a 492-page tagged file.

## Tests

`cargo test -p panoply-pdf` runs unit tests, and integration tests over `tests/corpus` PDFs
(`python3 -I tests/corpus/fetch.py --kind pdf` fills the cache; the 300 MB file is only made by
`generate.py --tier large`). Every output is opened by pdfium as an independent reader, and compress outputs are
drawn and compared against the originals. Examples: `compress_report` (the Markdown tables in the results
file), `compress_cli`, `inspect`, `pageops_time`, `jpegprobe`.

## License notes

MIT OR Apache-2.0. pdfium is BSD-3-Clause and Apache-2.0 and is downloaded, not distributed. The CCITT
Group 4 coder (`fax`) is MIT and was already in the tree through `tiff`. The JPEG
encoder (`jpeg-encoder`) is `(MIT OR Apache-2.0) AND IJG`, so shipping it carries this credit line: "This
software is based in part on the work of the Independent JPEG Group."
