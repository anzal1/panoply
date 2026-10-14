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
- `image.to_text` and `pdf.ocr`: on macOS, pictures and pages are read at no more than 6 million pixels (A4 at about 250 dpi), because Apple Vision's memory grows with the picture. Other systems keep 36 million. A 31 MP photograph took 660 MB, now 288 MB. Test: `a_page_sized_picture_is_read_whole_and_a_huge_one_is_brought_down`.
- Manifests: `image/heif` is listed as an output of Compress, Convert, Resize and Strip metadata, and `image/x-canon-cr2` for Strip metadata.

### Budgets

Jobs that stay over 300 MB do so for a reason that is written down in DECISIONS.md, "Exceptions found by the beta smoke matrix", with the measured numbers: ImageIO on a 33 MP 10-bit HEIF, camera RAW and an HDR AVIF (each named by file, so the same module on an ordinary picture is held to 300 MB), the fill model behind Erase and Remove watermark, Apple Vision's cutout for the passport photo, the OCR fallback engines, and a batch of Upscale. These are accepted costs, not fixes, and the model-backed rows are the first thing to revisit.

### Still open

- Arithmetic-coded JPEG and subsampled YCbCr TIFF are refused in plain words. The matrix counts that as right because the decoder cannot read them yet (panoply-image README). A person with such a file is not served.
- No picture in the corpus has a watermark or a signature, so Remove watermark and Signature cleaner are only shown refusing or acting on an ordinary photo. Remove watermark took text that belongs to the scene (the label of a bottle in the Canon RAW photo, a caption area on two other photos) for a watermark and filled it in. The detector may be too eager on photos with text in them, which is worth a look before launch.
- HDR to SDR ran on two HDR files and passed. Windows and Linux are not covered: the matrix test runs only when the whole corpus is on disk, which CI does not fetch (pictures and PDFs only), and the rules and exceptions were measured on macOS.

