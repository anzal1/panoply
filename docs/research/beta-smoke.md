# Beta smoke matrix

Every registered module (built-in and the example WebAssembly modules) over real corpus files, through the path the desktop app takes: the catalog, the options form built from the manifest with its default values, the file sniffed from its bytes, `jobs::run_blocking`, and the result card text from `Done::headline`. No window is opened.

- Commit `e37f112`, 2026-10-14, macos aarch64 with 18 CPUs (Apple M5 Max), debug build.
- 33 modules, 498 jobs: **385 pass, 113 expected refusals, 0 fail**, 0 budget violations that are not documented exceptions.
- Pass: the job wrote what the manifest promises, or did nothing and the result card says why (already the target format, a report, the answer on the card). Expected refusal: the module declined with a plain sentence and a next step, and a rule in `crates/panoply-smoke/src/expect.rs` says that was right for this input (see Expected refusals below). Fail: a crash, a hang, a wrong or empty output, an error with no next step, or a refusal no rule explains.
- Up to 2 real files per accepted file type, plus one batch job per module. Each job runs in its own process under `/usr/bin/time`, so peak memory is that job's own high-water mark (peak footprint on macOS, resident set on Linux), and it includes the registry the process builds.
- Refused on purpose and not in this matrix: encrypted, truncated and broken files. The host's corpus tests check that those fail in plain words.

## What the first run found, and what was fixed

The first run, before any triage, had 468 jobs and called 244 of them not clean. Every one was classified by hand:

| Class | Jobs | What it was | What was done |
|---|---:|---|---|
| (a) Expected refusal, plain and right | 151 | 84 modules that need an answer only a person has (which country, how big, what to erase or keep, which pages), 24 animated pictures that ask before dropping frames, 21 arithmetic-coded JPEGs and subsampled YCbCr TIFFs (documented), 12 files with no sound, 4 formats with no metadata to strip, 4 pictures already large or sharp, 2 pictures with no ink | The matrix types the smallest valid answer, runs the animated files again with `animation=first`, and counts a refusal as clean only when a rule in `expect.rs` explains it for that input |
| (a) Pass, nothing to change | 52 | Reports, answers on the result card, files already in the target form (Streams, HDR to SDR, Image to text, Repair, already MP4, already mono, already searchable) | Counted as a pass |
| (b) Wrong refusal | 22 | Convert refused every audio file because its default was MP4 (8), OCR refused every picture that is not a JPEG or PNG (12), Normalize and Stereo to mono refused an Ogg video (2) | Fixed, below |
| (c) Wrong output | 6 | HEIC and Canon RAW results that sniff as `image/heif` and `image/x-canon-cr2`, which the manifests did not list | Fixed in the manifests |
| (d) Budget | 13 | Peak memory over 300 MB | Triaged, below |

Beyond the 244, the stricter matrix and a read of the results that passed found more (c) problems: a "compressed" 7 kb/s Opus recording that came out 7 times bigger, a stereo Vorbis file that became a bigger mono one, Normalize raising the bit rate of a low rate recording, Resize writing a JPEG 2000 photo as a PNG seven times its size, Streams saying "1 streams", and the plan never putting a face in front of the passport photo module or an HDR file in front of HDR to SDR.

### Fixed in the modules

- `media.convert`: with no target chosen the file decides (video becomes MP4, sound becomes M4A), instead of refusing every audio file.
- `pdf.ocr`: a picture a PDF page cannot hold (HEIC, AVIF, WebP, GIF, TIFF, RAW, ICO, JPEG 2000) is kept as a PNG for the page. Test: `a_picture_a_pdf_cannot_hold_is_kept_as_a_png_for_reading`.
- `media.normalize` and `media.to_mono`: an Ogg video keeps its container (`.ogv`), and any other video container with no writer falls back to Matroska instead of M4A. Test: `ogg_video_keeps_its_container_and_sound_only_ogg_stays_sound_only`.
- `media.normalize` and `media.to_mono`: a sound written again in its own codec keeps its real bit rate (a smaller channel count takes its share), and Opus standing in for Vorbis does too. Before, a 7 kb/s recording was raised to 64 kb/s and a stereo to mono file came out bigger. Tests: `writing_a_sound_again_in_its_codec_never_raises_its_rate`, `a_sound_with_no_stated_rate_keeps_the_rate_of_its_file`.
- `media.compress` (sound): when the default quality cannot make a file smaller, the original is left alone and the card says so.
- `image.resize`: a JPEG 2000, JPEG XL or RAW photo is written as a JPEG, not a PNG several times its size. Test: `resizing_a_jpeg_2000_photo_writes_a_jpeg_not_a_larger_png`.
- `media.streams`: "1 stream", not "1 streams".
- `image.to_text` and `pdf.ocr`: pictures and pages are read at no more than 6 million pixels (A4 at about 250 dpi). A 31 MP photograph took 660 MB, now 288 MB. Test: `a_page_sized_picture_is_read_whole_and_a_huge_one_is_brought_down`.
- Manifests: `image/heif` is listed as an output of Compress, Convert, Resize and Strip metadata, and `image/x-canon-cr2` for Strip metadata.

### Budgets

Jobs that stay over 300 MB do so for a reason that is written down in DECISIONS.md, "Exceptions found by the beta smoke matrix", with the measured numbers: ImageIO on a 33 MP 10-bit HEIF, camera RAW and an HDR AVIF (each named by file, so the same module on an ordinary picture is held to 300 MB), the fill model behind Erase and Remove watermark, Apple Vision's cutout for the passport photo, the OCR fallback engines, and a batch of Upscale. These are accepted costs, not fixes, and the model-backed rows are the first thing to revisit.

### Still open

- Arithmetic-coded JPEG and subsampled YCbCr TIFF are refused in plain words. The matrix counts that as right because the decoder cannot read them yet (panoply-image README). A person with such a file is not served.
- No picture in the corpus has a watermark or a signature, so Remove watermark and Signature cleaner are only shown refusing or acting on an ordinary photo. Remove watermark took text that belongs to the scene (the label of a bottle in the Canon RAW photo, a caption area on two other photos) for a watermark and filled it in. The detector may be too eager on photos with text in them, which is worth a look before launch.
- HDR to SDR ran on two HDR files and passed. Windows and Linux are not covered: the matrix test runs only when the whole corpus is on disk, which CI does not fetch (pictures and PDFs only), and the rules and exceptions were measured on macOS.

## Summary

| Module | Pattern | Source | Jobs | Pass | Expected refusal | Fail | Budget flags | Median job ms | Worst job ms | Worst peak MB |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `files.hash` | I | built in | 49 | 49 | 0 | 0 | 0 | 0 | 6 | 6 |
| `id.passport_photo` | D | built in | 20 | 2 | 18 | 0 | 0 | 468 | 875 | 786 |
| `id.print_sheet` | B | built in | 20 | 16 | 4 | 0 | 0 | 106 | 2183 | 859 |
| `id.signature_cleaner` | D | built in | 17 | 13 | 4 | 0 | 0 | 127 | 976 | 369 |
| `image.compress` | B | built in | 19 | 13 | 6 | 0 | 0 | 772 | 10157 | 821 |
| `image.convert` | A | built in | 19 | 13 | 6 | 0 | 0 | 66 | 1412 | 829 |
| `image.erase` | C | built in | 20 | 13 | 7 | 0 | 0 | 326 | 6209 | 1540 |
| `image.remove_background` | B | built in | 19 | 13 | 6 | 0 | 0 | 618 | 3549 | 1325 |
| `image.remove_watermark` | C | built in | 20 | 3 | 17 | 0 | 0 | 1380 | 7098 | 1612 |
| `image.resize` | B | built in | 19 | 13 | 6 | 0 | 0 | 88 | 441 | 340 |
| `image.resize_to_kb` | B | built in | 19 | 13 | 6 | 0 | 0 | 86 | 2025 | 783 |
| `image.strip_metadata` | I | built in | 17 | 13 | 4 | 0 | 0 | 1 | 8 | 29 |
| `image.to_text` | A | built in | 17 | 14 | 3 | 0 | 0 | 180 | 769 | 419 |
| `image.upscale` | B | built in | 19 | 9 | 10 | 0 | 0 | 465 | 1901 | 754 |
| `media.compress` | B | built in | 16 | 16 | 0 | 0 | 0 | 605 | 4427 | 5 |
| `media.convert` | A | built in | 16 | 16 | 0 | 0 | 0 | 114 | 896 | 5 |
| `media.extract_audio` | A | built in | 16 | 12 | 4 | 0 | 0 | 74 | 110 | 5 |
| `media.hdr_to_sdr` | A | built in | 9 | 9 | 0 | 0 | 0 | 38 | 1461 | 16 |
| `media.info` | I | built in | 16 | 16 | 0 | 0 | 0 | 35 | 265 | 5 |
| `media.normalize` | A | built in | 16 | 12 | 4 | 0 | 0 | 356 | 3448 | 7 |
| `media.repair` | A | built in | 16 | 16 | 0 | 0 | 0 | 113 | 791 | 9 |
| `media.streams` | C | built in | 16 | 16 | 0 | 0 | 0 | 19 | 112 | 5 |
| `media.to_mono` | A | built in | 16 | 12 | 4 | 0 | 0 | 180 | 988 | 7 |
| `media.trim` | C | built in | 17 | 16 | 1 | 0 | 0 | 73 | 929 | 11 |
| `pdf.compress` | B | built in | 3 | 3 | 0 | 0 | 0 | 3632 | 3644 | 109 |
| `pdf.images_to_pdf` | A | built in | 5 | 5 | 0 | 0 | 0 | 5 | 7 | 10 |
| `pdf.merge` | A | built in | 3 | 3 | 0 | 0 | 0 | 25 | 27 | 5 |
| `pdf.ocr` | B | built in | 21 | 19 | 2 | 0 | 0 | 300 | 30048 | 961 |
| `pdf.organize` | C | built in | 4 | 3 | 1 | 0 | 0 | 18 | 28 | 5 |
| `pdf.split` | A | built in | 3 | 3 | 0 | 0 | 0 | 142 | 151 | 5 |
| `pdf.to_images` | A | built in | 3 | 3 | 0 | 0 | 0 | 2465 | 2545 | 148 |
| `text.slugify_lines` | A | WASM example | 4 | 4 | 0 | 0 | 0 | 16 | 20 | 11 |
| `text.word_count` | I | WASM example | 4 | 4 | 0 | 0 | 0 | 13 | 15 | 10 |

## Failures and budget violations

None. Every job passed and every budget held.

## Expected refusals

Each is a refusal the app shows on its result card in plain words with a next step, for an input the module is right to refuse. A refusal no rule explains is a failure.

| Rule | Why it is right | Jobs | Modules |
|---|---|---:|---|
| needs a choice | The case runs the module with its default options on purpose. These modules have no default for the answer that is the person's (which country, how big, what to erase or keep, which pages), so they refuse in plain words and say what to type. | 6 | `id.passport_photo`, `id.print_sheet`, `image.erase`, `image.remove_watermark`, `media.trim`, `pdf.organize` |
| animated, asks first | Panoply does not write animated files yet, so by default it asks before dropping frames. The same file runs with animation=first in its own case. | 32 | `image.compress`, `image.convert`, `image.erase`, `image.remove_background`, `image.remove_watermark`, `image.resize`, `image.resize_to_kb`, `image.upscale` |
| arithmetic-coded JPEG | A rare JPEG variant the decoder cannot read yet. Panoply says so and asks for a standard JPEG or PNG (panoply-image README). | 16 | `id.passport_photo`, `id.print_sheet`, `id.signature_cleaner`, `image.compress`, `image.convert`, `image.erase`, `image.remove_background`, `image.remove_watermark`, `image.resize`, `image.resize_to_kb`, `image.to_text`, `image.upscale`, `pdf.ocr` |
| subsampled YCbCr TIFF | A TIFF flavor the decoder cannot read yet. Panoply says so and asks for a plain TIFF or PNG (panoply-image README). | 13 | `id.passport_photo`, `id.print_sheet`, `id.signature_cleaner`, `image.compress`, `image.convert`, `image.erase`, `image.remove_background`, `image.remove_watermark`, `image.resize`, `image.resize_to_kb`, `image.to_text`, `image.upscale`, `pdf.ocr` |
| no face in the picture | The photo has no face to build a passport photo around. A face photo in the same run must not be refused. | 14 | `id.passport_photo` |
| no ink in the picture | The picture holds no signature, so there is nothing to clean. | 2 | `id.signature_cleaner` |
| no watermark found | The corpus has no watermarked picture. Panoply says nothing was changed and how to mark the area by hand. | 10 | `image.remove_watermark` |
| no sound in the file | The file has no audio stream (checked with ffprobe). | 12 | `media.extract_audio`, `media.normalize`, `media.to_mono` |
| no metadata container | GIF, ICO, JPEG 2000 and EXR have no EXIF or XMP container to strip. | 4 | `image.strip_metadata` |
| too large or too small to upscale | Upscaling a picture that is already large would not help and would not fit the memory limit, and upscaling one that is already sharp gains little. | 4 | `image.upscale` |

## Budget exceptions in use

These are written down in DECISIONS.md ("Exceptions to the budgets", and "Exceptions found by the beta smoke matrix") and, for the first four, on the Budgets line of the module's spec. The matrix honours only these. A row that names files covers those files only:

| Module | Cap | Source |
|---|---|---|
| `image.remove_background` | 2000 MB peak, no 100 ms first preview | id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP |
| `image.erase` | 600 MB peak, no 100 ms first preview | id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s |
| `image.upscale` | no 100 ms first preview | id-and-image.md 3.11 Budgets: time and model size, preview tile in seconds |
| `image.convert` | 400 MB peak | id-and-image.md 3.1 Budgets: RAW development is about 8 bytes per pixel, 45 MP is about 360 MB |
| `image.erase` | 1250 MB peak, no 100 ms first preview | DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch |
| `image.remove_watermark` | 950 MB peak, no 100 ms first preview | DECISIONS.md, smoke matrix exceptions: the same fill model as Erase, 786 MB on a camera RAW and 892 MB with a 31 MP picture in the batch |
| `id.passport_photo` | 450 MB peak | id-and-image.md 2.1 Budgets (the cutout carries the 3.8 exception) and DECISIONS.md, smoke matrix exceptions: Apple Vision's foreground mask, 348 MB measured |
| `pdf.ocr` | 650 MB peak | DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured |
| `image.upscale` | 400 MB peak, no 100 ms first preview | DECISIONS.md, smoke matrix exceptions: the upscale model plus the allocator keeping its high-water mark between the pictures of a batch, 313 MB measured |
| `*` on `img-heic-rotated-cw90` | 1800 MB peak | DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3) |
| `*` on `img-raw-canon-7d-sraw`, `img-raw-pixel-7-pro-dng` | 1000 MB peak | DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception) |
| `*` on `img-avif-gainmap-avif` | 450 MB peak | DECISIONS.md, smoke matrix exceptions: an HDR AVIF costs ImageIO about 170 MB before any module touches it, even at 400 x 300 |

Jobs in this run that needed an exception:

- `id.passport_photo` on img-face-borman: peak memory 318 MB, above 300 MB: id-and-image.md 2.1 Budgets (the cutout carries the 3.8 exception) and DECISIONS.md, smoke matrix exceptions: Apple Vision's foreground mask, 348 MB measured
- `id.passport_photo` on img-face-jemison: peak memory 391 MB, above 300 MB: id-and-image.md 2.1 Budgets (the cutout carries the 3.8 exception) and DECISIONS.md, smoke matrix exceptions: Apple Vision's foreground mask, 348 MB measured
- `id.print_sheet` on img-avif-gainmap-avif: peak memory 335 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: an HDR AVIF costs ImageIO about 170 MB before any module touches it, even at 400 x 300
- `id.print_sheet` on img-heic-rotated-cw90: peak memory 859 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `id.print_sheet` on img-raw-pixel-7-pro-dng: peak memory 362 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `id.signature_cleaner` on img-heic-rotated-cw90: peak memory 369 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `id.signature_cleaner` on img-raw-pixel-7-pro-dng: peak memory 356 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.compress` on img-raw-canon-7d-sraw: peak memory 389 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.compress` on img-heic-rotated-cw90: peak memory 821 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.compress` on img-raw-pixel-7-pro-dng: peak memory 647 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.convert` on img-heic-rotated-cw90: peak memory 829 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.convert` on img-raw-pixel-7-pro-dng: peak memory 399 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.erase` on img-avif-gainmap-avif: peak memory 771 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-raw-canon-7d-sraw: peak memory 661 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-heic-unsupported-depth: peak memory 577 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-heic-rotated-cw90: peak memory 1540 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.erase` on img-vips-ico: peak memory 572 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-vips-jp2: peak memory 588 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-commons-wide-nasa: peak memory 1024 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-pngsuite-odd-size-palette: peak memory 572 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-avif-png-icc-exif-xmp: peak memory 580 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-raw-pixel-7-pro-dng: peak memory 936 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-vips-apng-16bit: peak memory 572 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-commons-animated-webp: peak memory 630 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.erase` on img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa: peak memory 1156 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch
- `image.remove_background` on img-avif-gainmap-avif: peak memory 847 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-raw-canon-7d-sraw: peak memory 334 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-heic-unsupported-depth: peak memory 789 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-heic-rotated-cw90: peak memory 1325 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-commons-wide-nasa: peak memory 579 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-pngsuite-odd-size-palette: peak memory 788 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-avif-png-icc-exif-xmp: peak memory 796 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-raw-pixel-7-pro-dng: peak memory 445 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-vips-apng-16bit: peak memory 790 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-commons-animated-webp: peak memory 322 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa: peak memory 1009 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_watermark` on img-raw-canon-7d-sraw: peak memory 786 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.remove_watermark` on img-heic-rotated-cw90: peak memory 1612 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.remove_watermark` on img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa: peak memory 892 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the same fill model as Erase, 786 MB on a camera RAW and 892 MB with a 31 MP picture in the batch
- `image.resize` on img-heic-rotated-cw90: peak memory 306 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.resize` on img-raw-pixel-7-pro-dng: peak memory 340 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.resize_to_kb` on img-heic-rotated-cw90: peak memory 783 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.resize_to_kb` on img-raw-pixel-7-pro-dng: peak memory 361 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `image.to_text` on img-avif-gainmap-avif: peak memory 377 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: an HDR AVIF costs ImageIO about 170 MB before any module touches it, even at 400 x 300
- `image.to_text` on img-heic-rotated-cw90: peak memory 419 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `image.upscale` on img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa: peak memory 310 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: the upscale model plus the allocator keeping its high-water mark between the pictures of a batch, 313 MB measured
- `pdf.ocr` on img-avif-gainmap-avif: peak memory 378 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured
- `pdf.ocr` on img-raw-canon-7d-sraw: peak memory 304 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)
- `pdf.ocr` on img-heic-rotated-cw90: peak memory 961 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)
- `pdf.ocr` on pdf-qpdf-inherited-rotate: peak memory 355 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured
- `pdf.ocr` on pdf-commons-ia-scan-mining-claims: peak memory 346 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured
- `pdf.ocr` on pdf-commons-arabic-book: peak memory 466 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured
- `pdf.ocr` on pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims: peak memory 544 MB, above 300 MB: DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured

## Full table

Job ms is the time inside the job (the worker thread, as the result card measures it). Process ms is the whole child, with process start and the registry. Peak MB is the child's high-water mark.

### `files.hash` (Hash files, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-vips-exr |  | 378.6 KB | PASS | 1 | 1443 | 5 | report | dd7355a2c2a694bae041500af4f61ae96bdc02a6e9474fa53d4e4ccef83c8774  $TMPpanoply-smoke-62782-0/sample.exr |
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 0 | 19 | 4 | report | 2f8683d21725261f37f86e115f0c212cc52d0fefd3a2ddfcc4fa648c1859906d  $TMPpanoply-smoke-62782-1/colors-animated-8bpc.avif |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 0 | 13 | 5 | report | 9bf9c6a7606951de07e4079cd63c2cfe379d95139cd99ab9142d8a6ee22d28c7  $TMPpanoply-smoke-62782-2/seine-hdr-gainmap.avif |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 6 | 19 | 6 | report | 3789b1ba5d880613104637c40a06619cd9f3b54b0cf62d2d95b9f9e885edcf6e  $TMPpanoply-smoke-62782-3/Canon---EOS-7D---sRAW2-sRAW-3x2.CR2 |
| off-poi-docx-rtl | docx | 6.8 KB | PASS | 0 | 13 | 4 | report | d159b6ac6734f14a9ecf8e97ca5029bdcddbfae2d33ba0128aa774924e0b190d  $TMPpanoply-smoke-62782-4/rtl.docx |
| off-poi-docx-comments | docx | 65.3 KB | PASS | 0 | 13 | 5 | report | 06b63b13f0b78a7179151af52c2282557c1ed204899feee9fcef4ca575344f97  $TMPpanoply-smoke-62782-5/comments.docx |
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 1 | 13 | 6 | report | cd53a05924b02513cee5ccef00f8b4bc9cc4d90fdc87446202a695823c2af21b  $TMPpanoply-smoke-62782-6/lachmoeweisma.flac |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 1 | 13 | 6 | report | 0c563965a86b422c695892d0dae07b458d200d8b08f741d1a9d01ead37588337  $TMPpanoply-smoke-62782-7/glockenweihe.flac |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 0 | 13 | 4 | report | bd0beae04d8688100541bec4cd87c87b51bb8ab2c9b7f257bc20a0e0e2011291  $TMPpanoply-smoke-62782-8/dispose-previous.gif |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 0 | 13 | 5 | report | b6b2d01837fe70230094010a2bd106d1f6b8e23f1370575e470402987d5a0244  $TMPpanoply-smoke-62782-9/muybridge-race-horse.gif |
| img-heic-unsupported-depth | heif | 698 B | PASS | 0 | 13 | 4 | report | 0b421065e0ec70dde8dace2d300c4c27d2a49e3d19babd15673d626f3c9d3740  $TMPpanoply-smoke-62782-10/unsupported-depth.heic |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 3 | 13 | 6 | report | f0d5e88be07b9d70a3715218ca612ccf45b126e5b4b42172dce14a62d2099932  $TMPpanoply-smoke-62782-11/guitar-rotated-cw90.hif |
| img-vips-ico | ico | 7.4 KB | PASS | 0 | 13 | 5 | report | 461350ef07af2b4376c277ba9cb62a98e535ef6a13760ebfacc82cf95d347acb  $TMPpanoply-smoke-62782-12/favicon.ico |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 0 | 13 | 5 | report | ac78346a830a6bca8eeb99c56d45571dbbbde2e3994907102ff9a099d0bbad34  $TMPpanoply-smoke-62782-13/world.jp2 |
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 0 | 13 | 5 | report | 4672c7f08864cd0a8c73a4fa4b66ca32b635d38464551c1ecf06564ae8c89b38  $TMPpanoply-smoke-62782-14/testimgari.jpg |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 2 | 13 | 6 | report | de3151e7c5c786215428e799da596356120cc8d09adb6c98d5af6a7bee034a08  $TMPpanoply-smoke-62782-15/day-earth-smiled.jpg |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 0 | 13 | 5 | report | caba82e1205938dd89548539805baf487c7121692922ae528e1e0832f41431cf  $TMPpanoply-smoke-62782-16/aac-lc-stereo.m4a |
| CONTRIBUTING.md | md | 3.4 KB | PASS | 0 | 12 | 4 | report | 1c24b8e902dbfe895b6fd488d45101020a43e61cddd91548cbaa87e5394c08cd  $TMPpanoply-smoke-62782-17/CONTRIBUTING.md |
| docs/FLOWS.md | md | 16.7 KB | PASS | 0 | 13 | 4 | report | 7ff371f35c33f0336cb0de79c5f3486ea506e26076248c59c6f008c40f69000d  $TMPpanoply-smoke-62782-18/FLOWS.md |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 1 | 13 | 6 | report | f789ec720d4901f80417aecc1ec615eb94f53118c4ec0ba0b065b419e2866933  $TMPpanoply-smoke-62782-19/multi-audio-3-tracks-subs.mkv |
| vid-gen-prores | mov | 2.9 MB | PASS | 2 | 13 | 6 | report | ff5abdcf98f938be804a2d593948bf0f63dbf14d4e2ac130b20947867f221aae  $TMPpanoply-smoke-62782-20/prores-proxy-720p.mov |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 0 | 13 | 5 | report | 3f4cd70ef98bbff1e592b4b96f0fa556f94d49da4ec4c3bbc55155684053d43e  $TMPpanoply-smoke-62782-21/recorder-improvisation.mp3 |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 2 | 13 | 6 | report | 7727db92e45ab2fba3997ecd396d19230a8b6d98208f097ce91729be04cb2910  $TMPpanoply-smoke-62782-22/stay-down-here-1915.mp3 |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 1 | 12 | 5 | report | 77145c94c11f3754207499158df22406e1fe7635553c1c86dc5e881dfeb32016  $TMPpanoply-smoke-62782-23/bbb-360p-h264-10s.mp4 |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 4 | 13 | 5 | report | 1bb2a62abfcb5c4bf2e9216b63ba715b358f22c9fa8e68ca7149c4788b906826  $TMPpanoply-smoke-62782-24/bbb-1080p-h264-10s.mp4 |
| off-poi-doc-legacy | msi | 27.1 KB | PASS | 0 | 13 | 5 | report | 717585c0a88f20862c86220163dd03da87cd743970354c6b29add32725f2e833  $TMPpanoply-smoke-62782-25/sample.doc |
| off-poi-ppt-legacy | msi | 125.4 KB | PASS | 0 | 13 | 5 | report | 64275a6685b8f267827178037dcc59de927475e56385725916f4c6039c7ce7f6  $TMPpanoply-smoke-62782-26/sample.ppt |
| off-gen-odp-sample | odp | 1.0 KB | PASS | 0 | 13 | 4 | report | b8012719061d326ba5b6e7547609da994d15397851f745f252902099b595761a  $TMPpanoply-smoke-62782-27/sample.odp |
| off-poi-ods-sample | ods | 20.4 KB | PASS | 0 | 13 | 4 | report | a551ef35d3faddae4bf135960bde419f4a0140f9398c2218614b089ae8a4b46f  $TMPpanoply-smoke-62782-28/sample.ods |
| off-gen-odt-sample | odt | 1.0 KB | PASS | 0 | 13 | 5 | report | 26a6325f96f6e3e3c80aedfec03a7777d5996b24213db81be7039101657d9127  $TMPpanoply-smoke-62782-29/sample.odt |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 0 | 13 | 5 | report | 6ed2068e155e362ac5eaf208e2c3eaf3247af3b63697a514aea8fe64a45f5532  $TMPpanoply-smoke-62782-30/netaji-1943.ogg |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 1 | 11 | 6 | report | 3eef53a936f0315eec41cb1d20641d2b2289702ad9186ea894505d67d1b12e66  $TMPpanoply-smoke-62782-31/camera-clip-mvi0131.ogv |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 0 | 13 | 5 | report | a8ac5f3019f070de83323daf7fd2a010efa8d29e588eb08ae817a8f7f1aac129  $TMPpanoply-smoke-62782-32/speech-opus-7kbps.opus |
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 0 | 13 | 5 | report | 69cc42c34812790a88d8275d45ac9b0a72b9e5ad62aa70427d25caeee4a9f46a  $TMPpanoply-smoke-62782-33/inherited-rotate.pdf |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 3 | 13 | 6 | report | c0562b7c09948044dab28a65de4b16f59079a715624b2e3a04b6b71aaebc7eef  $TMPpanoply-smoke-62782-34/ia-scan-mining-claims.pdf |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 0 | 13 | 5 | report | 1b9e244a5c1d6cc9830391fff4eac565ada026d000e60faf7525c8add6bdc726  $TMPpanoply-smoke-62782-35/s05n3p02.png |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 0 | 13 | 5 | report | 308bb51c3338ed9519cdaaa79186b934a21d1748d94120ed0348eb4f193d0988  $TMPpanoply-smoke-62782-36/paris-icc-exif-xmp.png |
| off-poi-pptx-sample | pptx | 39.1 KB | PASS | 0 | 13 | 5 | report | bfb4b2f07c9233afd2f32aa5781d849d0c7d225bf03721a10becf487b481d828  $TMPpanoply-smoke-62782-37/sample.pptx |
| off-poi-pptx-smartart | pptx | 41.4 KB | PASS | 0 | 17 | 5 | report | b97e4c6d2ee1dd4094f50f9043820610268dd452d7717c537f5456636adcc353  $TMPpanoply-smoke-62782-38/smartart.pptx |
| img-vips-tiff-subsampled | tif | 72.8 KB | PASS | 0 | 13 | 5 | report | 264d4955898a35ae70af0ec6eed0dbc49d7929f6c18c92a4ed6198f6db396f4b  $TMPpanoply-smoke-62782-39/subsampled.tif |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 5 | 19 | 5 | report | 6b921558944db5fdb4f1c63dc64919b6666ef700857a187e18500099c6e7c6c6  $TMPpanoply-smoke-62782-40/Google---Pixel-7-Pro---16bit-4x3.dng |
| tests/corpus/pdf.toml | toml | 28.2 KB | PASS | 0 | 13 | 5 | report | e3dddca42bb73a30e492da7e6b973012cccac77ac7af856c9ecdc0ba9fa56c27  $TMPpanoply-smoke-62782-41/pdf.toml |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 0 | 13 | 5 | report | 322728e6d71288c219919579376112d80cf0d8f5df82212af8c6bf285a4c3988  $TMPpanoply-smoke-62782-42/alaa1.wav |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 3 | 13 | 6 | report | 5d83087e7fe53aa61db02e159adb10b46411ab75263a95d5814a09e24214a3d4  $TMPpanoply-smoke-62782-43/phone-clip-vid20240601.webm |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 2 | 13 | 6 | report | f4457bf8cee98b882c659f13883e58ad058d72b80516ecdf3f21b45ef70dd188  $TMPpanoply-smoke-62782-44/bbb-720p-vp9-10s.webm |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 2 | 13 | 5 | report | 3f24267941a9253f49a7d8ab8401132fe868bcb229618a9970db4d3049303d29  $TMPpanoply-smoke-62782-45/animated-webp-sample.webp |
| off-poi-xlsx-charts | xlsx | 13.0 KB | PASS | 0 | 13 | 5 | report | 47e9e8dea87494d17dbacce9aa3a9ec31cb2b26b288de05ca95c0a4622ca7f80  $TMPpanoply-smoke-62782-46/charts.xlsx |
| img-vips-svg | xml | 4.7 KB | PASS | 0 | 13 | 5 | report | 0e2e46c26b8335bdca45a930af59366329e6fa66bf186ffeb4a90f2129c8f664  $TMPpanoply-smoke-62782-47/logo.svg |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 3 | 13 | 5 | report | 69cc42c34812790a88d8275d45ac9b0a72b9e5ad62aa70427d25caeee4a9f46a  $TMPpanoply-smoke-62782-48/inherited-rotate.pdf |

### `id.passport_photo` (Passport photo, pattern D)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed country=IN) | 1.2 KB | EXPECTED REFUSAL |  | 157 | 159 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-gainmap-avif | avif (typed country=IN) | 123.0 KB | EXPECTED REFUSAL |  | 168 | 214 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-animated-8bpc | defaults only | 1.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | Say which country's rule to use, for example country=IN or country=US. |
| img-raw-canon-7d-sraw | cr2 (typed country=IN) | 11.1 MB | EXPECTED REFUSAL |  | 507 | 259 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-gif-dispose-previous | gif (typed country=IN) | 1.1 KB | EXPECTED REFUSAL |  | 88 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-commons-gif-horse | gif (typed country=IN) | 297.2 KB | EXPECTED REFUSAL |  | 93 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-heic-unsupported-depth | heif (typed country=IN) | 698 B | EXPECTED REFUSAL |  | 131 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-heic-rotated-cw90 | heif (typed country=IN) | 5.2 MB | EXPECTED REFUSAL |  | 1983 | 786 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-ico | ico (typed country=IN) | 7.4 KB | EXPECTED REFUSAL |  | 101 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-jp2 | jp2 (typed country=IN) | 64.0 KB | EXPECTED REFUSAL |  | 119 | 166 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-turbo-arithmetic | jpg (typed country=IN) | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed country=IN) | 5.0 MB | EXPECTED REFUSAL |  | 537 | 246 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-pngsuite-odd-size-palette | png (typed country=IN) | 129 B | EXPECTED REFUSAL |  | 100 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-png-icc-exif-xmp | png (typed country=IN) | 153.2 KB | EXPECTED REFUSAL |  | 99 | 160 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-tiff-subsampled | tif (typed country=IN) | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed country=IN) | 12.2 MB | EXPECTED REFUSAL |  | 747 | 363 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-commons-animated-webp | webp (typed country=IN) | 2.9 MB | EXPECTED REFUSAL |  | 185 | 201 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-face-borman | jpg (face) (typed country=IN) | 863.0 KB | PASS | 468 | 486 | 318 | 228.2 KB | 863 KB to 228 KB, 74% smaller |
| img-face-jemison | jpg (face) (typed country=IN) | 2.0 MB | PASS | 875 | 893 | 391 | 226.2 KB | 2.0 MB to 226 KB, 89% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed country=IN) | 5.4 MB | EXPECTED REFUSAL |  | 630 | 267 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `id.print_sheet` (Print sheet, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed size=35x45) | 1.2 KB | PASS | 132 | 151 | 142 | 194.7 KB | 1 KB to 195 KB, 15666% bigger |
| img-avif-gainmap-avif | avif (typed size=35x45) | 123.0 KB | PASS | 177 | 191 | 335 | 2.5 MB | 123 KB to 2.5 MB, 1947% bigger |
| img-avif-animated-8bpc | defaults only | 1.2 KB | EXPECTED REFUSAL |  | 80 | 8 |  | Panoply does not know how big to print this photo, and it will not guess. Say the size with size=35x45 (millimetres), or take it from a country rule with rule=in-passport. |
| img-raw-canon-7d-sraw | cr2 (typed size=35x45) | 11.1 MB | PASS | 473 | 488 | 259 | 3.9 MB | 11 MB to 3.9 MB, 65% smaller |
| img-vips-gif-dispose-previous | gif (typed size=35x45) | 1.1 KB | PASS | 45 | 60 | 139 | 779.0 KB | 1 KB to 779 KB, 71238% bigger |
| img-commons-gif-horse | gif (typed size=35x45) | 297.2 KB | PASS | 54 | 67 | 139 | 1.4 MB | 297 KB to 1.4 MB, 360% bigger |
| img-heic-unsupported-depth | heif (typed size=35x45) | 698 B | PASS | 106 | 118 | 141 | 303.7 KB | 698 bytes to 304 KB, 43408% bigger |
| img-heic-rotated-cw90 | heif (typed size=35x45) | 5.2 MB | PASS | 2183 | 2202 | 859 | 1.8 MB | 5.2 MB to 1.8 MB, 66% smaller |
| img-vips-ico | ico (typed size=35x45) | 7.4 KB | PASS | 45 | 62 | 139 | 881.0 KB | 7 KB to 881 KB, 11796% bigger |
| img-vips-jp2 | jp2 (typed size=35x45) | 64.0 KB | PASS | 100 | 112 | 146 | 3.0 MB | 64 KB to 3.0 MB, 4558% bigger |
| img-turbo-arithmetic | jpg (typed size=35x45) | 5.1 KB | EXPECTED REFUSAL |  | 12 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed size=35x45) | 5.0 MB | PASS | 545 | 560 | 263 | 1.9 MB | 5.0 MB to 1.9 MB, 62% smaller |
| img-pngsuite-odd-size-palette | png (typed size=35x45) | 129 B | PASS | 44 | 55 | 138 | 1.1 MB | 129 bytes to 1.1 MB, 823481% bigger |
| img-avif-png-icc-exif-xmp | png (typed size=35x45) | 153.2 KB | PASS | 69 | 85 | 142 | 2.5 MB | 153 KB to 2.5 MB, 1526% bigger |
| img-vips-tiff-subsampled | tif (typed size=35x45) | 72.8 KB | EXPECTED REFUSAL |  | 13 | 4 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed size=35x45) | 12.2 MB | PASS | 671 | 686 | 362 | 2.8 MB | 12 MB to 2.8 MB, 77% smaller |
| img-commons-animated-webp | webp (typed size=35x45) | 2.9 MB | PASS | 92 | 105 | 168 | 1.7 MB | 2.9 MB to 1.7 MB, 43% smaller |
| img-face-borman | jpg (face) (typed size=35x45) | 863.0 KB | PASS | 146 | 161 | 173 | 3.0 MB | 863 KB to 3.0 MB, 245% bigger |
| img-face-jemison | jpg (face) (typed size=35x45) | 2.0 MB | PASS | 225 | 241 | 299 | 2.9 MB | 2.0 MB to 2.9 MB, 45% bigger |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed size=35x45) | 5.4 MB | EXPECTED REFUSAL |  | 12 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `id.signature_cleaner` (Signature cleaner, pattern D)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 73 | 88 | 10 | 150 B | 1 KB to 150 bytes, 88% smaller |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 127 | 143 | 203 | 26.8 KB | 123 KB to 27 KB, 78% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 831 | 846 | 272 | 156.7 KB | 11 MB to 157 KB, 99% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 4 | 19 | 6 | 562 B | 1 KB to 562 bytes, 49% smaller |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 14 | 25 | 7 | 5.0 KB | 297 KB to 5 KB, 98% smaller |
| img-heic-unsupported-depth | heif | 698 B | EXPECTED REFUSAL |  | 79 | 11 |  | No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame. |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 658 | 675 | 369 | 27.3 KB | 5.2 MB to 27 KB, 99% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 2 | 12 | 6 | 554 B | 7 KB to 554 bytes, 93% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 99 | 112 | 20 | 25.3 KB | 64 KB to 25 KB, 60% smaller |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 320 | 334 | 68 | 61.2 KB | 5.0 MB to 61 KB, 99% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | EXPECTED REFUSAL |  | 13 | 5 |  | No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame. |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 26 | 38 | 10 | 18.1 KB | 153 KB to 18 KB, 88% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 12 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 976 | 992 | 356 | 114.6 KB | 12 MB to 115 KB, 99% smaller |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 380 | 394 | 81 | 56.1 KB | 2.9 MB to 56 KB, 98% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 750 | 762 | 102 | 312.0 KB | 2 files, 312 KB |

### `image.compress` (Compress image, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 351 | 363 | 244 | 20.2 KB | 123 KB to 20 KB, 84% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 6740 | 6760 | 389 | 528.8 KB | 11 MB to 529 KB, 95% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 248 | 264 | 15 | 487 B | 698 bytes to 487 bytes, 30% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 4070 | 4091 | 821 | 260.7 KB | 5.2 MB to 261 KB, 95% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 157 | 172 | 34 | 315 B | 7 KB to 315 bytes, 96% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 1267 | 1282 | 61 | 56.3 KB | 64 KB to 56 KB, 12% smaller |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 772 | 785 | 156 | 268.5 KB | 5.0 MB to 268 KB, 95% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 1 | 13 | 6 | 86 B | 129 bytes to 86 bytes, 33% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 66 | 81 | 10 | 130.4 KB | 153 KB to 130 KB, 15% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 10157 | 10174 | 647 | 754.7 KB | 12 MB to 755 KB, 94% smaller |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 1 | 13 | 6 | 78 B | 212 bytes to 78 bytes, 63% smaller |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 5775 | 5790 | 141 | 22.9 KB | 2.9 MB to 23 KB, 99% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 1488 | 1503 | 154 | 567.1 KB | 2 files, 567 KB |

### `image.convert` (Convert image, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 99 | 112 | 200 | 31.7 KB | 123 KB to 32 KB, 74% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 379 | 392 | 269 | 578.5 KB | 11 MB to 578 KB, 95% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 66 | 75 | 11 | 382 B | 698 bytes to 382 bytes, 45% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 1412 | 1435 | 829 | 1.4 MB | 5.2 MB to 1.4 MB, 73% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 0 | 13 | 5 | 907 B | 7 KB to 907 bytes, 88% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 23 | 31 | 12 | 73.1 KB | 64 KB to 73 KB, 14% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 66 | 74 | 31 | 972.1 KB | 5.0 MB to 972 KB, 81% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 0 | 12 | 5 | 352 B | 129 bytes to 352 bytes, 173% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 2 | 13 | 6 | 18.3 KB | 153 KB to 18 KB, 88% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 554 | 566 | 399 | 1.1 MB | 12 MB to 1.1 MB, 91% smaller |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 26 | 38 | 5 | 289 B | 212 bytes to 289 bytes, 36% bigger |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 29 | 38 | 42 | 140.6 KB | 2.9 MB to 141 KB, 95% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 85 | 99 | 37 | 1.4 MB | 2 files, 1.4 MB |

### `image.erase` (Erase object, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed rect=10%,10%,20%,20%) | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif (typed rect=10%,10%,20%,20%) | 123.0 KB | PASS | 326 | 349 | 771 | 178.7 KB | 123 KB to 179 KB, 45% bigger |
| img-avif-animated-8bpc | defaults only | 1.2 KB | EXPECTED REFUSAL |  | 13 | 4 |  | Say what to erase: give a mask picture as the second file (white is erased), or set rect to a rectangle such as 100,200,300,80. |
| img-raw-canon-7d-sraw | cr2 (typed rect=10%,10%,20%,20%) | 11.1 MB | PASS | 1016 | 1037 | 661 | 6.9 MB | 11 MB to 6.9 MB, 38% smaller |
| img-vips-gif-dispose-previous | gif (typed rect=10%,10%,20%,20%) | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif (typed rect=10%,10%,20%,20%) | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif (typed rect=10%,10%,20%,20%) | 698 B | PASS | 249 | 270 | 577 | 2.8 KB | 698 bytes to 3 KB, 297% bigger |
| img-heic-rotated-cw90 | heif (typed rect=10%,10%,20%,20%) | 5.2 MB | PASS | 6209 | 6240 | 1540 | 23.1 MB | 5.2 MB to 23 MB, 347% bigger |
| img-vips-ico | ico (typed rect=10%,10%,20%,20%) | 7.4 KB | PASS | 191 | 212 | 572 | 1.9 KB | 7 KB to 2 KB, 74% smaller |
| img-vips-jp2 | jp2 (typed rect=10%,10%,20%,20%) | 64.0 KB | PASS | 241 | 261 | 588 | 496.9 KB | 64 KB to 497 KB, 677% bigger |
| img-turbo-arithmetic | jpg (typed rect=10%,10%,20%,20%) | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed rect=10%,10%,20%,20%) | 5.0 MB | PASS | 993 | 1020 | 1024 | 1.5 MB | 5.0 MB to 1.5 MB, 69% smaller |
| img-pngsuite-odd-size-palette | png (typed rect=10%,10%,20%,20%) | 129 B | PASS | 183 | 201 | 572 | 148 B | 129 bytes to 148 bytes, 15% bigger |
| img-avif-png-icc-exif-xmp | png (typed rect=10%,10%,20%,20%) | 153.2 KB | PASS | 207 | 229 | 580 | 154.3 KB | 153 KB to 154 KB, 1% bigger |
| img-vips-tiff-subsampled | tif (typed rect=10%,10%,20%,20%) | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed rect=10%,10%,20%,20%) | 12.2 MB | PASS | 2598 | 2616 | 936 | 12.8 MB | 12 MB to 13 MB, 5% bigger |
| img-commons-animated-webp | webp (typed rect=10%,10%,20%,20%) | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed rect=10%,10%,20%,20% animation=first) | 212 B | PASS | 210 | 232 | 572 | 136 B | 212 bytes to 136 bytes, 36% smaller |
| img-commons-animated-webp | webp (first frame) (typed rect=10%,10%,20%,20% animation=first) | 2.9 MB | PASS | 371 | 391 | 630 | 67.6 KB | 2.9 MB to 68 KB, 98% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed rect=10%,10%,20%,20%) | 5.4 MB | PASS | 1144 | 1169 | 1156 | 1.8 MB | 2 files, 1.8 MB |

### `image.remove_background` (Remove background, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 619 | 649 | 847 | 2.7 KB | 123 KB to 3 KB, 98% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 853 | 870 | 334 | 1.4 MB | 11 MB to 1.4 MB, 88% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 12 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 571 | 597 | 789 | 577 B | 698 bytes to 577 bytes, 17% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 3549 | 3594 | 1325 | 3.7 MB | 5.2 MB to 3.7 MB, 28% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 152 | 166 | 202 | 1.8 KB | 7 KB to 2 KB, 75% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 241 | 256 | 222 | 562.4 KB | 64 KB to 562 KB, 779% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 1823 | 1842 | 579 | 3.5 MB | 5.0 MB to 3.5 MB, 30% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 535 | 564 | 788 | 94 B | 129 bytes to 94 bytes, 27% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 558 | 583 | 796 | 36.0 KB | 153 KB to 36 KB, 76% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 1567 | 1593 | 445 | 2.8 MB | 12 MB to 2.8 MB, 77% smaller |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 618 | 647 | 790 | 82 B | 212 bytes to 82 bytes, 61% smaller |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 453 | 476 | 322 | 12.8 KB | 2.9 MB to 13 KB, 100% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 2660 | 2695 | 1009 | 3.5 MB | 2 files, 3.5 MB |

### `image.remove_watermark` (Remove watermark, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed own_content=true) | 1.2 KB | EXPECTED REFUSAL |  | 25 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif (typed own_content=true) | 123.0 KB | EXPECTED REFUSAL |  | 168 | 202 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-avif-animated-8bpc | defaults only | 1.2 KB | EXPECTED REFUSAL |  | 12 | 5 |  | This tool is for pictures you own or have the right to edit: your own photos with a date stamp, your own exports with a logo you added, your own AI images with a visible badge. Set own_content to true to confirm that, then run it again. |
| img-raw-canon-7d-sraw | cr2 (typed own_content=true) | 11.1 MB | PASS | 1358 | 1385 | 786 | 6.9 MB | 11 MB to 6.9 MB, 38% smaller |
| img-vips-gif-dispose-previous | gif (typed own_content=true) | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif (typed own_content=true) | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif (typed own_content=true) | 698 B | EXPECTED REFUSAL |  | 81 | 11 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-heic-rotated-cw90 | heif (typed own_content=true) | 5.2 MB | PASS | 7098 | 7126 | 1612 | 23.2 MB | 5.2 MB to 23 MB, 350% bigger |
| img-vips-ico | ico (typed own_content=true) | 7.4 KB | EXPECTED REFUSAL |  | 13 | 5 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-vips-jp2 | jp2 (typed own_content=true) | 64.0 KB | EXPECTED REFUSAL |  | 162 | 20 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-turbo-arithmetic | jpg (typed own_content=true) | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed own_content=true) | 5.0 MB | EXPECTED REFUSAL |  | 803 | 329 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-pngsuite-odd-size-palette | png (typed own_content=true) | 129 B | EXPECTED REFUSAL |  | 13 | 5 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-avif-png-icc-exif-xmp | png (typed own_content=true) | 153.2 KB | EXPECTED REFUSAL |  | 56 | 10 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-vips-tiff-subsampled | tif (typed own_content=true) | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed own_content=true) | 12.2 MB | EXPECTED REFUSAL |  | 952 | 395 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-commons-animated-webp | webp (typed own_content=true) | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed own_content=true animation=first) | 212 B | EXPECTED REFUSAL |  | 38 | 5 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-commons-animated-webp | webp (first frame) (typed own_content=true animation=first) | 2.9 MB | EXPECTED REFUSAL |  | 219 | 47 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed own_content=true) | 5.4 MB | PASS | 1380 | 1408 | 892 | 318.4 KB | 1 file, 318 KB |

### `image.resize` (Resize image, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 123 | 136 | 226 | 23.7 KB | 123 KB to 24 KB, 81% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 441 | 456 | 272 | 170.1 KB | 11 MB to 170 KB, 98% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 88 | 100 | 14 | 623 B | 698 bytes to 623 bytes, 11% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 376 | 388 | 306 | 32.1 KB | 5.2 MB to 32 KB, 99% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 0 | 13 | 5 | 1.4 KB | 7 KB to 1 KB, 81% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 23 | 32 | 12 | 90.1 KB | 64 KB to 90 KB, 41% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 64 | 75 | 21 | 40.5 KB | 5.0 MB to 40 KB, 99% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 0 | 13 | 5 | 102 B | 129 bytes to 102 bytes, 21% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 11 | 19 | 6 | 151.1 KB | 153 KB to 151 KB, 1% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 424 | 440 | 340 | 154.8 KB | 12 MB to 155 KB, 99% smaller |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 0 | 13 | 5 | 90 B | 212 bytes to 90 bytes, 58% smaller |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 104 | 112 | 46 | 30.9 KB | 2.9 MB to 31 KB, 99% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 101 | 114 | 30 | 148.1 KB | 2 files, 148 KB |

### `image.resize_to_kb` (Resize to exact KB, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 106 | 118 | 200 | 31.2 KB | 123 KB to 31 KB, 75% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 581 | 593 | 259 | 636.2 KB | 11 MB to 636 KB, 94% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 67 | 81 | 11 | 382 B | 698 bytes to 382 bytes, 45% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 2025 | 2045 | 783 | 453.7 KB | 5.2 MB to 454 KB, 91% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 1 | 13 | 5 | 1.0 KB | 7 KB to 1 KB, 86% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 44 | 56 | 11 | 78.2 KB | 64 KB to 78 KB, 22% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 4 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 507 | 522 | 83 | 243.8 KB | 5.0 MB to 244 KB, 95% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 1 | 13 | 5 | 416 B | 129 bytes to 416 bytes, 222% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 8 | 19 | 6 | 19.8 KB | 153 KB to 20 KB, 87% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 12 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 909 | 921 | 361 | 1.1 MB | 12 MB to 1.1 MB, 91% smaller |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 37 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 26 | 38 | 5 | 334 B | 212 bytes to 334 bytes, 58% bigger |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 86 | 100 | 34 | 144.0 KB | 2.9 MB to 144 KB, 95% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 641 | 654 | 101 | 656.9 KB | 2 files, 657 KB |

### `image.strip_metadata` (Strip metadata, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 0 | 13 | 5 | 1.2 KB | 1 KB to 1 KB, about the same size |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 1 | 13 | 5 | 123.0 KB | 123 KB to 123 KB, about the same size |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 3 | 13 | 27 | 11.1 MB | 11 MB to 11 MB, about the same size |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 13 | 4 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 0 | 13 | 4 | 698 B | 698 bytes to 698 bytes, about the same size |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 2 | 13 | 15 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| img-vips-ico | ico | 7.4 KB | EXPECTED REFUSAL |  | 13 | 4 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-vips-jp2 | jp2 | 64.0 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 0 | 13 | 4 | 5.1 KB | 5 KB to 5 KB, about the same size |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 6 | 19 | 15 | 5.0 MB | 5.0 MB to 5.0 MB, about the same size |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 0 | 13 | 4 | 129 B | 129 bytes to 129 bytes, about the same size |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 0 | 13 | 5 | 151.5 KB | 153 KB to 151 KB, 1% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | PASS | 0 | 13 | 5 | 72.8 KB | 73 KB to 73 KB, about the same size |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 3 | 13 | 29 | 12.2 MB | 12 MB to 12 MB, about the same size |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 1 | 13 | 10 | 2.9 MB | 2.9 MB to 2.9 MB, about the same size |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 8 | 19 | 15 | 5.4 MB | 5.4 MB to 5.4 MB, about the same size |

### `image.to_text` (Image to text, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 180 | 191 | 156 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 212 | 231 | 377 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 769 | 792 | 274 | report | Z4  MINERAL WATER |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 149 | 168 | 156 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 129 | 151 | 156 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 166 | 187 | 156 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 532 | 555 | 419 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-vips-ico | ico | 7.4 KB | PASS | 290 | 308 | 159 | report | W |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 180 | 199 | 166 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 668 | 684 | 288 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 103 | 117 | 149 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 113 | 131 | 158 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 18 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 354 | 377 | 219 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 227 | 247 | 212 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | EXPECTED REFUSAL |  | 13 | 5 |  | testimgari.jpg: This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `image.upscale` (Upscale, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | EXPECTED REFUSAL |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 477 | 494 | 241 | 2.1 MB | 123 KB to 2.1 MB, 1635% bigger |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | EXPECTED REFUSAL |  | 403 | 259 |  | This picture is already 2,592 x 1,728. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. |
| img-vips-gif-dispose-previous | gif | 1.1 KB | EXPECTED REFUSAL |  | 12 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | EXPECTED REFUSAL |  | 12 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 142 | 162 | 177 | 14.1 KB | 698 bytes to 14 KB, 1916% bigger |
| img-heic-rotated-cw90 | heif | 5.2 MB | EXPECTED REFUSAL |  | 2143 | 754 |  | guitar-rotated-cw90.hif is 4,672 x 7,008, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. |
| img-vips-ico | ico | 7.4 KB | PASS | 85 | 100 | 176 | 26.5 KB | 7 KB to 26 KB, 257% bigger |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 1029 | 1045 | 186 | 4.7 MB | 64 KB to 4.7 MB, 7247% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | EXPECTED REFUSAL |  | 517 | 131 |  | day-earth-smiled.jpg is 9,000 x 3,500, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 78 | 94 | 176 | 878 B | 129 bytes to 878 bytes, 581% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 465 | 480 | 180 | 2.1 MB | 153 KB to 2.1 MB, 1263% bigger |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | EXPECTED REFUSAL |  | 580 | 393 |  | This picture is already 3,866 x 2,908. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. |
| img-commons-animated-webp | webp | 2.9 MB | EXPECTED REFUSAL |  | 38 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-vips-apng-16bit | png (first frame) (typed animation=first) | 212 B | PASS | 108 | 125 | 176 | 111 B | 212 bytes to 111 bytes, 48% smaller |
| img-commons-animated-webp | webp (first frame) (typed animation=first) | 2.9 MB | PASS | 1901 | 1917 | 208 | 3.5 MB | 2.9 MB to 3.5 MB, 19% bigger |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 1777 | 1799 | 310 | 901.7 KB | 1 file, 902 KB |

### `media.compress` (Compress video, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 263 | 278 | 5 | 279.6 KB | 1.6 MB to 280 KB, 82% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 178 | 191 | 5 | 533.1 KB | 1.9 MB to 533 KB, 72% smaller |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 148 | 161 | 5 | 148.3 KB | 200 KB to 148 KB, 26% smaller |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 814 | 829 | 5 | 1.0 MB | 2.1 MB to 1.0 MB, 50% smaller |
| vid-gen-prores | mov | 2.9 MB | PASS | 605 | 623 | 5 | 277.1 KB | 2.9 MB to 277 KB, 91% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 246 | 262 | 5 | 293.8 KB | 555 KB to 294 KB, 47% smaller |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 791 | 804 | 5 | 2.1 MB | 2.8 MB to 2.1 MB, 24% smaller |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 869 | 888 | 5 | 400.4 KB | 991 KB to 400 KB, 60% smaller |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 3656 | 3674 | 5 | 7.1 MB | 10 MB to 7.1 MB, 33% smaller |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 506 | 527 | 5 | report | netaji-1943.ogg is already small for its length (395 KB), so Compress could not make it smaller and left it as it is. |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 839 | 852 | 5 | 860.9 KB | 2.5 MB to 861 KB, 65% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 110 | 125 | 5 | report | speech-opus-7kbps.opus is already small for its length (10 KB), so Compress could not make it smaller and left it as it is. |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 133 | 144 | 5 | 17.4 KB | 230 KB to 17 KB, 92% smaller |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 1746 | 1756 | 5 | 3.5 MB | 5.1 MB to 3.5 MB, 32% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 1208 | 1224 | 5 | 1.6 MB | 5.2 MB to 1.6 MB, 70% smaller |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 4427 | 4443 | 5 | 8.7 MB | 17 MB to 8.7 MB, 48% smaller |

### `media.convert` (Convert video or audio, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 243 | 256 | 5 | 462.3 KB | 1.6 MB to 462 KB, 71% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 151 | 168 | 5 | 533.1 KB | 1.9 MB to 533 KB, 72% smaller |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 19 | 30 | 5 | report | aac-lc-stereo.m4a is already M4A, so there was nothing to convert. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 94 | 103 | 5 | 2.1 MB | 2.1 MB to 2.1 MB, about the same size |
| vid-gen-prores | mov | 2.9 MB | PASS | 400 | 412 | 5 | 1.2 MB | 2.9 MB to 1.2 MB, 58% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 283 | 300 | 5 | 582.8 KB | 555 KB to 583 KB, 5% bigger |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 896 | 911 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, 1% bigger |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 38 | 50 | 4 | report | bbb-360p-h264-10s.mp4 is already MP4, so there was nothing to convert. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 57 | 69 | 4 | report | bbb-1080p-h264-10s.mp4 is already MP4, so there was nothing to convert. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 363 | 376 | 5 | 862.5 KB | 395 KB to 863 KB, 119% bigger |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 519 | 529 | 5 | 1.8 MB | 2.5 MB to 1.8 MB, 28% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 114 | 125 | 5 | 155.1 KB | 10 KB to 155 KB, 1506% bigger |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 114 | 123 | 5 | 38.9 KB | 230 KB to 39 KB, 83% smaller |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 114 | 129 | 5 | 5.1 MB | 5.1 MB to 5.1 MB, about the same size |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 114 | 125 | 5 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 133 | 142 | 5 | report | bbb-360p-h264-10s.mp4 is already MP4, so there was nothing to convert. |

### `media.extract_audio` (Extract audio, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 58 | 68 | 5 | 1.6 MB | 1.6 MB to 1.6 MB, about the same size |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 73 | 82 | 5 | 1.9 MB | 1.9 MB to 1.9 MB, about the same size |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 57 | 69 | 5 | 200.4 KB | 200 KB to 200 KB, about the same size |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 73 | 87 | 5 | 74.3 KB | 2.1 MB to 74 KB, 96% smaller |
| vid-gen-prores | mov | 2.9 MB | PASS | 75 | 91 | 5 | 192.1 KB | 2.9 MB to 192 KB, 93% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 74 | 87 | 5 | 555.5 KB | 555 KB to 556 KB, about the same size |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 109 | 123 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | EXPECTED REFUSAL |  | 48 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | EXPECTED REFUSAL |  | 68 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 75 | 91 | 5 | 394.7 KB | 395 KB to 395 KB, about the same size |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 77 | 88 | 5 | 401.5 KB | 2.5 MB to 401 KB, 84% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 58 | 68 | 5 | 9.7 KB | 10 KB to 10 KB, about the same size |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 110 | 124 | 5 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 94 | 105 | 5 | 163.9 KB | 5.1 MB to 164 KB, 97% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | EXPECTED REFUSAL |  | 48 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | EXPECTED REFUSAL |  | 49 | 5 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.hdr_to_sdr` (HDR to SDR, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 38 | 49 | 5 | report | multi-audio-3-tracks-subs.mkv is already standard range, so nothing was changed. |
| vid-gen-prores | mov | 2.9 MB | PASS | 35 | 50 | 5 | report | prores-proxy-720p.mov is already standard range, so nothing was changed. |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 38 | 50 | 5 | report | bbb-360p-h264-10s.mp4 is already standard range, so nothing was changed. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 57 | 68 | 5 | report | bbb-1080p-h264-10s.mp4 is already standard range, so nothing was changed. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 36 | 47 | 5 | report | phone-clip-vid20240601.webm is already standard range, so nothing was changed. |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 38 | 50 | 5 | report | bbb-720p-vp9-10s.webm is already standard range, so nothing was changed. |
| vid-gen-hlg | mp4 (hdr) | 4.6 MB | PASS | 1461 | 1476 | 16 | 3.0 MB | 4.6 MB to 3.0 MB, 35% smaller |
| vid-gen-hdr10 | mp4 (hdr) | 5.8 MB | PASS | 1450 | 1467 | 16 | 2.9 MB | 5.8 MB to 2.9 MB, 50% smaller |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 128 | 140 | 5 | report | bbb-360p-h264-10s.mp4 is already standard range, so nothing was changed. |

### `media.info` (Video and audio info, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 35 | 50 | 5 | report | lachmoeweisma.flac: FLAC, 22.8 s, 1.6 MB, 552 kb/s |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 19 | 30 | 5 | report | glockenweihe.flac: FLAC, 2:55, 1.9 MB, 87 kb/s |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 19 | 30 | 5 | report | aac-lc-stereo.m4a: MP4, 12.0 s, 200 KB, 134 kb/s |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 57 | 68 | 5 | report | multi-audio-3-tracks-subs.mkv: Matroska (MKV), 6.0 s, 2.1 MB, 2.8 Mb/s |
| vid-gen-prores | mov | 2.9 MB | PASS | 38 | 49 | 5 | report | prores-proxy-720p.mov: QuickTime MOV, 2.0 s, 2.9 MB, 11.7 Mb/s |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 19 | 31 | 5 | report | recorder-improvisation.mp3: MP3, 24.0 s, 555 KB, 185 kb/s |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 19 | 30 | 5 | report | stay-down-here-1915.mp3: MP3, 2:55, 2.8 MB, 128 kb/s |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 73 | 85 | 5 | report | bbb-360p-h264-10s.mp4: MP4, 10.0 s, 991 KB, 793 kb/s |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 112 | 128 | 5 | report | bbb-1080p-h264-10s.mp4: MP4, 10.0 s, 10.5 MB, 8.4 Mb/s |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 19 | 32 | 5 | report | netaji-1943.ogg: Ogg, 53.2 s, 395 KB, 59 kb/s |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 70 | 81 | 5 | report | camera-clip-mvi0131.ogv: Ogg, 32.3 s, 2.5 MB, 611 kb/s |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 19 | 32 | 5 | report | speech-opus-7kbps.opus: Ogg, 10.8 s, 10 KB, 7 kb/s |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 35 | 50 | 5 | report | alaa1.wav: WAV, 2.6 s, 230 KB, 706 kb/s |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 76 | 87 | 5 | report | phone-clip-vid20240601.webm: WebM, 9.5 s, 5.1 MB, 4.3 Mb/s |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 76 | 91 | 5 | report | bbb-720p-vp9-10s.webm: WebM, 10.0 s, 5.2 MB, 4.2 Mb/s |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 265 | 280 | 5 | report | == bbb-360p-h264-10s.mp4 |

### `media.normalize` (Normalize loudness, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 200 | 212 | 6 | 1.7 MB | 1.6 MB to 1.7 MB, 8% bigger |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 332 | 346 | 5 | 2.1 MB | 1.9 MB to 2.1 MB, 11% bigger |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 246 | 256 | 7 | 220.8 KB | 200 KB to 221 KB, 10% bigger |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 250 | 262 | 6 | 2.1 MB | 2.1 MB to 2.1 MB, 2% bigger |
| vid-gen-prores | mov | 2.9 MB | PASS | 130 | 143 | 6 | 2.9 MB | 2.9 MB to 2.9 MB, about the same size |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 364 | 378 | 7 | 577.5 KB | 555 KB to 577 KB, 4% bigger |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 3448 | 3462 | 6 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | EXPECTED REFUSAL |  | 50 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | EXPECTED REFUSAL |  | 69 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 453 | 466 | 6 | 360.9 KB | 395 KB to 361 KB, 9% smaller |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 356 | 370 | 6 | 2.5 MB | 2.5 MB to 2.5 MB, about the same size |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 443 | 459 | 6 | 11.2 KB | 10 KB to 11 KB, 16% bigger |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 408 | 423 | 6 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 441 | 456 | 6 | 5.2 MB | 5.1 MB to 5.2 MB, 1% bigger |
| vid-bbb-720-vp9 | webm | 5.2 MB | EXPECTED REFUSAL |  | 49 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | EXPECTED REFUSAL |  | 50 | 4 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.repair` (Repair video, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 77 | 87 | 5 | 1.6 MB | 1.6 MB to 1.6 MB, about the same size |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 93 | 107 | 5 | 1.9 MB | 1.9 MB to 1.9 MB, about the same size |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 35 | 50 | 5 | report | aac-lc-stereo.m4a is not damaged: its index is complete, all 521 samples are in the file and it plays start to end. Nothing was changed. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 122 | 130 | 5 | 2.1 MB | 2.1 MB to 2.1 MB, about the same size |
| vid-gen-prores | mov | 2.9 MB | PASS | 39 | 49 | 9 | report | prores-proxy-720p.mov is not damaged: its index is complete, all 96048 samples are in the file and it plays start to end. Nothing was change... |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 76 | 88 | 5 | 555.5 KB | 555 KB to 556 KB, about the same size |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 185 | 194 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 70 | 81 | 5 | report | bbb-360p-h264-10s.mp4 is not damaged: its index is complete, all 300 samples are in the file and it plays start to end. Nothing was changed. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 406 | 420 | 5 | report | bbb-1080p-h264-10s.mp4 is not damaged: its index is complete, all 600 samples are in the file and it plays start to end. Nothing was changed... |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 114 | 129 | 5 | 394.7 KB | 395 KB to 395 KB, about the same size |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 114 | 124 | 5 | 2.5 MB | 2.5 MB to 2.5 MB, about the same size |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 74 | 88 | 5 | 9.7 KB | 10 KB to 10 KB, about the same size |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 113 | 125 | 5 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 387 | 399 | 5 | 5.1 MB | 5.1 MB to 5.1 MB, about the same size |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 388 | 402 | 5 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 791 | 806 | 5 | 5.2 MB | 1 file, 5.2 MB |

### `media.streams` (Streams, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 19 | 30 | 5 | report | lachmoeweisma.flac: 1 stream. Say what to do with drop, extract, default or language. |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 19 | 32 | 4 | report | glockenweihe.flac: 1 stream. Say what to do with drop, extract, default or language. |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 19 | 31 | 5 | report | aac-lc-stereo.m4a: 2 streams. Say what to do with drop, extract, default or language. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 19 | 30 | 5 | report | multi-audio-3-tracks-subs.mkv: 5 streams. Say what to do with drop, extract, default or language. |
| vid-gen-prores | mov | 2.9 MB | PASS | 19 | 32 | 5 | report | prores-proxy-720p.mov: 2 streams. Say what to do with drop, extract, default or language. |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 19 | 31 | 5 | report | recorder-improvisation.mp3: 1 stream. Say what to do with drop, extract, default or language. |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 19 | 31 | 5 | report | stay-down-here-1915.mp3: 1 stream. Say what to do with drop, extract, default or language. |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 34 | 44 | 5 | report | bbb-360p-h264-10s.mp4: 1 stream. Say what to do with drop, extract, default or language. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 54 | 62 | 5 | report | bbb-1080p-h264-10s.mp4: 1 stream. Say what to do with drop, extract, default or language. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 19 | 31 | 5 | report | netaji-1943.ogg: 1 stream. Say what to do with drop, extract, default or language. |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 19 | 31 | 4 | report | camera-clip-mvi0131.ogv: 2 streams. Say what to do with drop, extract, default or language. |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 19 | 31 | 5 | report | speech-opus-7kbps.opus: 1 stream. Say what to do with drop, extract, default or language. |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 38 | 50 | 5 | report | alaa1.wav: 1 stream. Say what to do with drop, extract, default or language. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 37 | 50 | 5 | report | phone-clip-vid20240601.webm: 2 streams. Say what to do with drop, extract, default or language. |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 38 | 50 | 5 | report | bbb-720p-vp9-10s.webm: 1 stream. Say what to do with drop, extract, default or language. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 112 | 124 | 5 | report | bbb-360p-h264-10s.mp4: 1 stream. Say what to do with drop, extract, default or language. |

### `media.to_mono` (Stereo to mono, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 188 | 200 | 6 | 1.3 MB | 1.6 MB to 1.3 MB, 20% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 19 | 32 | 5 | report | glockenweihe.flac is already mono, so there was nothing to change. |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 180 | 194 | 6 | 112.2 KB | 200 KB to 112 KB, 44% smaller |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 178 | 194 | 7 | 2.1 MB | 2.1 MB to 2.1 MB, 1% bigger |
| vid-gen-prores | mov | 2.9 MB | PASS | 19 | 31 | 5 | report | prores-proxy-720p.mov is already mono, so there was nothing to change. |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 260 | 269 | 6 | 288.7 KB | 555 KB to 289 KB, 48% smaller |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 988 | 1001 | 7 | 1.4 MB | 2.8 MB to 1.4 MB, 50% smaller |
| vid-bbb-360-h264 | mp4 | 991.0 KB | EXPECTED REFUSAL |  | 50 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | EXPECTED REFUSAL |  | 68 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 635 | 648 | 7 | 194.5 KB | 395 KB to 195 KB, 51% smaller |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 320 | 335 | 7 | 2.3 MB | 2.5 MB to 2.3 MB, 6% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 20 | 30 | 5 | report | speech-opus-7kbps.opus is already mono, so there was nothing to change. |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 36 | 50 | 5 | report | alaa1.wav is already mono, so there was nothing to change. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 303 | 317 | 6 | 5.1 MB | 5.1 MB to 5.1 MB, 1% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | EXPECTED REFUSAL |  | 69 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | EXPECTED REFUSAL |  | 50 | 5 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.trim` (Trim video or audio, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac (typed keep=0:00-0:01) | 1.6 MB | PASS | 87 | 100 | 5 | 78.8 KB | 1.6 MB to 79 KB, 95% smaller |
| aud-commons-glocken-flac | flac (typed keep=0:00-0:01) | 1.9 MB | PASS | 73 | 88 | 5 | 10.8 KB | 1.9 MB to 11 KB, 99% smaller |
| aud-commons-lachmoeweisma-flac | defaults only | 1.6 MB | EXPECTED REFUSAL |  | 13 | 4 |  | Say what to keep, for example 0:12-1:40, or what to remove with the remove option |
| aud-gen-m4a-aac | m4a (typed keep=0:00-0:01) | 200.4 KB | PASS | 20 | 31 | 5 | 17.6 KB | 200 KB to 18 KB, 91% smaller |
| vid-gen-multi-audio | mkv (typed keep=0:00-0:01) | 2.1 MB | PASS | 95 | 106 | 5 | 361.7 KB | 2.1 MB to 362 KB, 83% smaller |
| vid-gen-prores | mov (typed keep=0:00-0:01) | 2.9 MB | PASS | 19 | 31 | 11 | 1.5 MB | 2.9 MB to 1.5 MB, 49% smaller |
| aud-commons-recorder-mp3 | mp3 (typed keep=0:00-0:01) | 555.3 KB | PASS | 58 | 69 | 5 | 28.5 KB | 555 KB to 29 KB, 95% smaller |
| aud-commons-1915-mp3 | mp3 (typed keep=0:00-0:01) | 2.8 MB | PASS | 58 | 69 | 5 | 17.4 KB | 2.8 MB to 17 KB, 99% smaller |
| vid-bbb-360-h264 | mp4 (typed keep=0:00-0:01) | 991.0 KB | PASS | 327 | 339 | 5 | 116.5 KB | 991 KB to 117 KB, 88% smaller |
| vid-bbb-1080-h264 | mp4 (typed keep=0:00-0:01) | 10.5 MB | PASS | 514 | 529 | 6 | 1.2 MB | 10 MB to 1.2 MB, 88% smaller |
| aud-commons-netaji-ogg | ogg (typed keep=0:00-0:01) | 394.7 KB | PASS | 58 | 69 | 5 | 10.8 KB | 395 KB to 11 KB, 97% smaller |
| vid-commons-camera-ogv | ogg (typed keep=0:00-0:01) | 2.5 MB | PASS | 56 | 68 | 5 | 96.8 KB | 2.5 MB to 97 KB, 96% smaller |
| aud-commons-opus-7kbps | opus (typed keep=0:00-0:01) | 9.7 KB | PASS | 58 | 67 | 5 | 1.1 KB | 10 KB to 1 KB, 89% smaller |
| aud-commons-alaa-wav | wav (typed keep=0:00-0:01) | 229.7 KB | PASS | 114 | 123 | 5 | 90.2 KB | 230 KB to 90 KB, 61% smaller |
| vid-commons-phone-webm | webm (typed keep=0:00-0:01) | 5.1 MB | PASS | 110 | 123 | 5 | 778.3 KB | 5.1 MB to 778 KB, 85% smaller |
| vid-bbb-720-vp9 | webm (typed keep=0:00-0:01) | 5.2 MB | PASS | 110 | 123 | 5 | 558.2 KB | 5.2 MB to 558 KB, 89% smaller |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 (typed keep=0:00-0:01) | 16.7 MB | PASS | 929 | 942 | 7 | 1.8 MB | 17 MB to 1.8 MB, 89% smaller |

### `pdf.compress` (Compress PDF, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 7 | 19 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 3644 | 3655 | 106 | 2.8 MB | 6.5 MB to 2.8 MB, 57% smaller |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 3632 | 3643 | 109 | 2.8 MB | 2 files, 2.8 MB |

### `pdf.images_to_pdf` (Images to PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 4 | 19 | 5 | 5.9 KB | 5 KB to 6 KB, 16% bigger |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 7 | 19 | 10 | 5.0 MB | 5.0 MB to 5.0 MB, about the same size |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 5 | 19 | 5 | 917 B | 129 bytes to 917 bytes, 611% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 4 | 13 | 5 | 152.2 KB | 153 KB to 152 KB, 1% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 6 | 19 | 10 | 5.4 MB | 1 file, 5.4 MB |

### `pdf.merge` (Merge PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 5 | 19 | 5 | 789 B | 978 bytes to 789 bytes, 19% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 25 | 38 | 5 | 6.5 MB | 6.5 MB to 6.5 MB, about the same size |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 27 | 38 | 5 | 6.6 MB | 1 file, 6.6 MB |

### `pdf.ocr` (OCR, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 176 | 193 | 156 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 235 | 250 | 378 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 1209 | 1229 | 304 | 6.4 MB | 11 MB to 6.4 MB, 42% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 126 | 138 | 156 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 134 | 150 | 156 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-heic-unsupported-depth | heif | 698 B | PASS | 173 | 187 | 156 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 3129 | 3155 | 961 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-vips-ico | ico | 7.4 KB | PASS | 300 | 318 | 159 | 3.6 KB | 7 KB to 4 KB, 52% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 219 | 232 | 167 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-turbo-arithmetic | jpg | 5.1 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 683 | 699 | 289 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 777 | 798 | 355 | 1.9 KB | 978 bytes to 2 KB, 95% bigger |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 293 | 307 | 346 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 121 | 136 | 149 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 147 | 162 | 158 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-vips-tiff-subsampled | tif | 72.8 KB | EXPECTED REFUSAL |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 495 | 513 | 232 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 349 | 367 | 213 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example ... |
| pdf-pdfjs-scan-bad | pdf (no-text-layer) | 29.8 KB | PASS | 386 | 407 | 269 | 19.5 KB | 30 KB to 20 KB, 34% smaller |
| pdf-commons-arabic-book | pdf (no-text-layer) | 1.2 MB | PASS | 30048 | 30073 | 466 | 1.4 MB | 1.2 MB to 1.4 MB, 17% bigger |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 859 | 885 | 544 | 1.9 KB | 1 file, 2 KB |

### `pdf.organize` (Organize PDF pages, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf (typed rotate=90) | 978 B | PASS | 6 | 19 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf (typed rotate=90) | 6.5 MB | PASS | 28 | 38 | 5 | 6.5 MB | 6.5 MB to 6.5 MB, about the same size |
| pdf-qpdf-inherited-rotate | defaults only | 978 B | EXPECTED REFUSAL |  | 13 | 5 |  | Nothing to do. Say which pages to keep or delete, a rotation, or turn on reverse. |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf (typed rotate=90) | 6.6 MB | PASS | 18 | 30 | 5 | 6.6 MB | 6.6 MB to 6.6 MB, about the same size |

### `pdf.split` (Split PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 5 | 19 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 142 | 153 | 5 | 6.5 MB | 32 files, 6.5 MB |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 151 | 163 | 5 | 6.6 MB | 34 files, 6.6 MB |

### `pdf.to_images` (PDF to images, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 27 | 37 | 26 | 38.2 KB | 978 bytes to 38 KB, 3801% bigger |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 2465 | 2480 | 129 | 8.5 MB | 32 files, 8.5 MB |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 2545 | 2562 | 148 | 8.6 MB | 34 files, 8.6 MB |

### `text.slugify_lines` (Slugify lines, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| CONTRIBUTING.md | md | 3.4 KB | PASS | 20 | 37 | 11 | 3.2 KB | 3 KB to 3 KB, 7% smaller |
| docs/FLOWS.md | md | 16.7 KB | PASS | 16 | 31 | 11 | 15.0 KB | 17 KB to 15 KB, 10% smaller |
| img-vips-svg | xml | 4.7 KB | PASS | 15 | 32 | 11 | 4.3 KB | 5 KB to 4 KB, 9% smaller |
| CONTRIBUTING.md, README.md, docs/FLOWS.md | batch of 3 md | 28.8 KB | PASS | 17 | 31 | 11 | 26.0 KB | 29 KB to 26 KB, 10% smaller |

### `text.word_count` (Word count, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| CONTRIBUTING.md | md | 3.4 KB | PASS | 13 | 25 | 10 | report | CONTRIBUTING.md: 77 lines, 509 words, 3415 characters, 3415 bytes |
| docs/FLOWS.md | md | 16.7 KB | PASS | 14 | 26 | 10 | report | FLOWS.md: 231 lines, 2725 words, 16742 characters, 16746 bytes |
| img-vips-svg | xml | 4.7 KB | PASS | 13 | 32 | 10 | report | logo.svg: 31 lines, 327 words, 4700 characters, 4700 bytes |
| CONTRIBUTING.md, README.md, docs/FLOWS.md | batch of 3 md | 28.8 KB | PASS | 15 | 32 | 10 | report | CONTRIBUTING.md: 77 lines, 509 words, 3415 characters, 3415 bytes |

