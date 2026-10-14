# panoply-image

The image engine behind the Panoply modules `image.convert`, `image.compress`, `image.resize`,
`image.resize_to_kb` and `image.strip_metadata`. Specs: `docs/specs/id-and-image.md` (the pipeline
contract IPC-1 to IPC-9, 2.3 and 2.4 for Resize to exact KB, 3.1 Convert, 3.2 Compress, 3.5 Resize,
3.17 Metadata), budgets and the engine ladder in `docs/FLOWS.md`, licensing in `docs/adr/0002`, OS
engines first in `docs/adr/0004`. The host wraps the engine in `crates/panoply-host/src/builtin/image.rs`,
which is what the command line, the MCP server and `panoply-bench` run:

```
panoply run image.compress photo.jpg
panoply run image.convert   photo.heic --opt to=jpg
panoply run image.resize    photo.jpg  --opt longest=1080
panoply run image.resize_to_kb photo.jpg --opt preset=ssc-signature
panoply run image.resize_to_kb photo.jpg --opt width=200 --opt height=230 --opt min_kb=20 --opt max_kb=50
panoply run image.strip_metadata photo.jpg --opt remove=gps
```

Every result is a new file next to the original (`name (compressed).jpg`, `name (1080x810).jpg`,
`name (clean).jpg`, `name.png`, `name (200x230, 48KB).jpg`), numbered on a clash, never over an
existing file. A job that fails or is cancelled removes what it had written. `--out DIR` puts results
in a folder. The result card lines (sizes, what was kept and dropped, GPS whenever it is kept) come
back as report lines.

## The modules

| id | pattern | what it does |
|---|---|---|
| `image.convert` | A | Any readable format to JPG, PNG, WebP, AVIF, HEIC, TIFF, BMP or GIF. Orientation applied, color managed, 16 bit kept for PNG and TIFF, transparency flattened onto a color for formats without it, metadata kept by default and the card says what was kept and dropped. JPG and PNG targets stream, so a 400 megapixel picture converts in about 20 MB. JPEG is written baseline with optimized tables and PNG with plain deflate, so it runs at the speed of the native tools; the full MozJPEG and oxipng effort belongs to Compress. |
| `image.compress` | B | Smallest file that scores at least SSIM 0.97 (luma and color, the worse of the two), a size to stay under (quarter steps of quality, so it lands within 2% below), or a fixed quality. Format kept by default, or a format shootout. Reduced colors for flat art PNG, never for photographs. Never inflates: if nothing is smaller the original is left alone and the card says so. |
| `image.resize` | B | Longest side (default 1080), exact pixels, or percent. Contain, cover, pad or stretch. Lanczos3 down and Mitchell up, in linear light, premultiplied alpha, limited to the range under the kernel's main lobe so there is no ringing. A big JPEG is decoded at a reduced size by the decoder first. Does not enlarge unless asked. Same fast JPEG and PNG profile as Convert. |
| `image.resize_to_kb` | B | An exact pixel size and a file size inside a window, for forms and exam portals. 15 shipped presets (`modules/builtin/image-resize-to-kb/presets/`, each with its source link, quote, date and status), 1000 or 1024 bytes per KB stated or read as "inside both", real encodes searched until the window is met, padding with a JPEG comment only as the last step and said on the card. |
| `image.strip_metadata` | I | Removes location, or private fields, or everything, by rewriting container segments. Pixel data is copied byte for byte. Orientation and the color profile survive. |

## What is in the crate

| path | job |
|---|---|
| `src/decode/` | One `RowReader` per format: JPEG (MozJPEG decoder, DCT-domain shrink-on-load, CMYK and YCCK, ICC, EXIF orientation), PNG and APNG, TIFF (strips and tiles, 1 to 16 bit, CMYK), WebP, GIF, BMP and ICO. `os.rs` is the OS decoder: HEIC, HEIF, AVIF, JPEG XL, JPEG 2000, OpenEXR and camera RAW through ImageIO on macOS, in bands. |
| `src/pipeline.rs` | The row pipeline: decode, color convert, resample, flatten alpha, change depth (blue noise dither, never truncation), into a `RowSink`. Nothing holds more than a batch of rows and the resampler window, so memory does not grow with the picture. |
| `src/color.rs`, `src/orient.rs` | ICC handling with `moxcms` (sRGB, Display P3, Adobe RGB, ProPhoto, untagged is sRGB), CMYK through the embedded profile, else a built-in conversion that handles Adobe's inverted CMYK and YCCK, the eight EXIF orientations (strip-wise for the two that can stream, whole image otherwise). |
| `src/resample.rs` | Streaming linear-light resampler on top of `fast_image_resize` (width) and a sliding window of rows (height). |
| `src/encode/` | MozJPEG (trellis, optimized tables, progressive, fractional quality, streaming baseline), `png` plus `oxipng`, libwebp, `ravif` (AVIF) with the OS encoder first, TIFF, BMP, GIF. HEIC only through the OS encoder (x265 is GPL). |
| `src/quant.rs` | Our own OKLab median cut and k-means palette quantizer with Floyd-Steinberg dithering (imagequant is GPL, gifski is AGPL). |
| `src/ssim.rs` | Our own SSIM (Gaussian 11x11, sigma 1.5, luma and chroma) in bands of rows. |
| `src/meta/` | Lossless metadata inspection and stripping for JPEG, PNG, WebP, TIFF and HEIC, HEIF and AVIF (ISO BMFF): EXIF, GPS, XMP, IPTC, ICC, thumbnails, maker notes, auxiliary images, Live Photo ids. Entries removed are zeroed, so a raw byte scan finds nothing. |
| `src/ops/` | The five operations, option handling (`options.rs`), fitting (`fit.rs`), the quality searches (`search.rs`) and the Resize to exact KB solver (`kb.rs`). |
| `src/presets.rs`, `build.rs` | The exam presets, embedded at build time from the presets folder. `PANOPLY_PRESETS=/folder` adds or replaces presets with your own `.toml` files. |
| `src/modules.rs` | Pairs each manifest (`modules/builtin/image-*/module.toml`) with its operation. |
| `examples/` | `smoke` (decode what you give it), `kb_proof` (every preset, 20 photos, both KB readings), `compress_report` (against other encoders at equal quality). |
| `tests/` | `corpus.rs`, `modules.rs`, `kb.rs`, `memory.rs`; see Testing. |

## Platforms

The engine builds and runs on macOS, Linux and Windows. The only platform specific code is the
operating system codec path, and it sits behind `cfg(target_os = "macos")`
(`panoply-platform-macos`, ImageIO). Everything else is Rust or C that builds with a plain `cc`.

| | macOS | Linux and Windows today |
|---|---|---|
| JPEG, PNG, APNG, WebP, GIF, TIFF, BMP, ICO decode | own readers | the same |
| JPEG, PNG, WebP, GIF, TIFF, BMP encode, AVIF encode | own encoders, ImageIO for AVIF | own encoders, `ravif` for AVIF |
| HEIC, HEIF decode | ImageIO | not available yet: plain error (below) |
| HEIC encode | ImageIO | not available: plain error, choose JPG, WebP or AVIF |
| AVIF, JPEG XL, JPEG 2000, OpenEXR, camera RAW decode | ImageIO | not available yet: plain error |

HEIC off macOS. Today every module answers a HEIC or HEIF file with "HEIC needs the HEIC helper on
this system" and what to do meanwhile, instead of failing obscurely; nothing is left behind. The
planned route, to be built behind the same `decode::os::open` seam:

1. Windows: the HEIF Image Extensions through WIC (`IWICBitmapDecoder`) where the user has them
   installed, the same way macOS uses ImageIO, so no codec is shipped.
2. Everywhere the OS has no decoder (Linux, Windows without the extension): libheif with libde265
   (both LGPL) as a separate helper executable that Panoply starts and talks to over a pipe, never
   linked into any Panoply binary. The helper is a download the user is asked about first, with its
   license shown.

Windows and Linux builds do not need extra system packages beyond a C compiler and, on x86_64,
`nasm` for MozJPEG's SIMD code (on aarch64 the NEON path needs no assembler).

## Memory

The budget is 300 MB per job. Measured with `panoply-bench` (peak physical footprint of the process,
full table in `docs/research/image-compress-results.md`):

- Convert, Resize, Resize to exact KB and Strip stream rows, so a 20000 by 20000 PNG (400 MP) converts in about 20 MB and a
  24000 by 5000 JPEG (120 MP) in under 70 MB. The tests in `tests/memory.rs` enforce 300 MB.
- Rotating by an orientation tag needs the whole picture (a 90 degree turn cannot stream), so
  those pictures above 24 MP are refused with a plain message instead of going over the cap
  (Convert; Resize decodes them reduced).
- Compress searches in memory: the picture (3 bytes a pixel) plus MozJPEG's coefficient buffers
  (about 6 bytes a pixel). A 23.5 MP baseline JPEG peaks at 257 MB, a 14 MP progressive one at 259 MB. Larger pictures are refused with
  "Resize it first", because the baseline-only encoding that would fit gives files up to three
  times bigger, which is not "smallest that looks the same".
- HEIC and other OS-decoded files cost what ImageIO costs. A hardware HEVC decode keeps some memory
  per tile, and a grid with very large tiles (the 33 MP Canon HIF in the corpus, 3520 x 1600 tiles)
  needs about 700 MB inside ImageIO whatever we do. See `crates/panoply-platform-macos/src/lib.rs`.
  Those rows fail the budget in the bench report and are listed there with the cause.

## Not done yet

- Animation output. Animated GIF, WebP, APNG and AVIF inputs stop with a message that offers
  `animation=first`, which uses the first frame (and the card says so). Writing all frames
  (IPC-6's default) needs frame compositing and an animated encoder; neither exists here yet.
- RAW is decoded through ImageIO on macOS only. SVG, JPEG XL decode (the `jxl-oxide` dependency is
  declared, not wired) and multi-page TIFF export are not wired.
- Face aware crop. Resize to exact KB crops around the center, or the top for portraits
  (`anchor=top`); the face detector arrives with the face tools.
- Ultra HDR gain maps are dropped and the card says so (IPC-3); applying them is the HDR module.
- Subsampled YCbCr TIFF, arithmetic-coded JPEG and planar TIFF are refused with a plain message.

## Testing

```
python3 -I tests/corpus/fetch.py --kind image
python3 tests/corpus/generate.py --kind image
PANOPLY_REQUIRE_CORPUS=1 cargo test -p panoply-image -p panoply-platform-macos
```

Without `PANOPLY_REQUIRE_CORPUS` a corpus test whose file is missing skips with a message; CI sets
it so a missing file is a failure. `tests/corpus.rs` covers EXIF orientation (all eight), CMYK, Display
P3, 16 bit, alpha to JPG, animated inputs, HEIC with depth, metadata, never overwriting and damaged
files. `tests/modules.rs` covers manifests against the specs, resize math (linear light, no ringing,
every fit) and the Compress targets. `tests/kb.rs` covers Resize to exact KB. `tests/memory.rs` runs the
400 MP PNG and a 120 MP JPEG through every module under the 300 MB cap. Budgets over the corpus:

```
cargo build --release -p panoply-bench -p panoply-cli
target/release/panoply-bench run --module image.compress --runner cli \
  --cli-args "run image.compress {input} --opt animation=first --out {out_dir}" --kind image
```

The measured proofs are examples so they can be rerun and printed as tables:

```
cargo run --release -p panoply-image --example kb_proof        # every preset, 20 photos, 1000 and 1024
cargo run --release -p panoply-image --example compress_report # Compress against other encoders
```

## Licenses

Every dependency is permissive (`deny.toml`, checked by `cargo deny`). No libheif, imagequant,
gifski or dssim. MozJPEG, libwebp and libjpeg-turbo are BSD or IJG style, `ravif` and `rav1e` are
BSD-2, `moxcms` is BSD-3 or Apache-2.0, `oxipng` is MIT, `fast_image_resize` is MIT or Apache-2.0.
`panoply-platform-macos` is the one crate that uses `unsafe`, for ImageIO and CoreGraphics; this crate
forbids it.
