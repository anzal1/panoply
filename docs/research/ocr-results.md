# OCR results (beta)

Measured 2026-10-14 on a MacBook with an Apple M5 Max (18 cores, 48 GB) running macOS 26.6.2, release build without
LTO. What was built: the OCR engine ladder in `crates/panoply-ocr`, the modules `image.to_text` and `pdf.ocr`
(`modules/builtin/ocr-image-to-text`, `modules/builtin/ocr-pdf`), the CLI commands `panoply image ocr` and
`panoply pdf ocr`. Specs: `docs/specs/pdf.md` section 12, `docs/specs/id-and-image.md` 3.33, ADR 0004.

## The ladder, and what this Mac turned out to cover

OS first, own model second, Tesseract last:

| order | engine | where | reads |
|---|---|---|---|
| 1 | Apple Vision `VNRecognizeTextRequest`, accurate mode | macOS (through `panoply-platform-macos`, `src/ocr.rs`, opened with dlopen at the first read) | the 30 languages Vision lists |
| 1 | `Windows.Media.Ocr` | Windows (`windows` crate) | languages with an OCR pack installed. Compile-checked for `x86_64-pc-windows-msvc`, never run |
| 2 | PP-OCRv5 mobile detection plus a script recognizer on ONNX Runtime 1.28.3 | every platform | Latin (`en`, `latin` models) and Devanagari (Hindi, Marathi, Nepali, with the Latin letters mixed into Hindi text) |
| 3 | Tesseract 5, tessdata_fast, language data pinned by checksum | wherever a `tesseract` program is installed | Bengali (and Hindi, English) |

**Apple Vision on macOS 26.6 reads neither Hindi nor Bengali.** `supportedRecognitionLanguages` for the accurate
level (revision 3), also for the new Swift `RecognizeTextRequest` and `RecognizeDocumentsRequest`, is: en, fr, it, de,
es, pt, zh-Hans, zh-Hant, yue-Hans, yue-Hant, ko, ja, ru, uk, th, vi, ar, tr, id, cs, da, nl, no, nn, nb, ms, pl, ro,
sv. So on a Mac the fallbacks are not a corner case: they are the whole Indic story. When no language is chosen and
Vision finds next to nothing on a picture that looks like lines of text, the ladder tries Devanagari and then Bengali
on its own (`Ocr::second_look`), so a Hindi page still comes out without the user naming the language.

Downloads (all pinned by SHA-256, checked before use, kept in `~/Library/Caches/Panoply/engines/ocr`, size told before
the first download): detection model 4.8 MB, English recognizer 7.8 MB, Latin recognizer 8.0 MB, Devanagari
recognizer 7.9 MB (all Apache-2.0, Hugging Face commits pinned), ONNX Runtime 1.28.3 33 MB on macOS arm64 (MIT, 9 MB
on Linux, 79 MB on Windows because the zip also carries debug symbols), Bengali Tesseract data 0.9 MB. The runtime
is loaded with `load-dynamic` from that pinned download, so nothing is linked and the 39 MB library is not mapped
until the first fallback job. Loaded sessions are dropped after 60 s without a job (tested in
`tests/idle.rs`); the runtime library itself cannot be unloaded and stays mapped until the process ends, the same
known exception as pdfium.

## How it was measured

Character error rate (CER) is the Levenshtein distance in Unicode scalar values between the label and the engine's
text, divided by the label length, after collapsing white space (`panoply_ocr::cer`). Combining marks count as
characters, as in Tesseract's own evaluation. The set (`crates/panoply-ocr/testdata/labelled`, rebuilt by
`testdata/gen/make_labelled.sh`):

- Generated text images, rendered with CoreText at 300 DPI (so Indic conjuncts are shaped correctly): 3 English
  paragraphs (Times New Roman, Helvetica Neue, Courier New, one with numbers and an e-mail address), 3 Hindi
  (Kohinoor Devanagari, Devanagari Sangam MN), 1 Hindi mixed with English words, 3 Bengali (Kohinoor Bangla, Bangla
  Sangam MN). Each in three variants: clean, scan (rotated 1.1 to 1.3 degrees, blurred, grain, lower contrast, JPEG
  quality 55) and low-res (half size, JPEG quality 70). 30 images.
- Corpus scans, rendered at 300 DPI: two real Internet Archive book pages (`ia-scan-public-lands.pdf` page 15, a
  typewritten page of about 3,900 characters, and `ia-scan-mining-claims.pdf` page 9, a proportional serif page with
  underlines and a display heading) and `pdfjs/scan-bad.pdf` (one word). The labels of the corpus pages were typed
  by hand from the page images by the agent that built this, checked against the hidden OCR layer those two files
  carry. The label texts of the generated images are the strings they were rendered from.
- One process per engine per input (`testdata/run_eval.sh`), so peak memory is the maximum resident set size of that
  process, models and runtime included. Times: cold is the first read in a fresh process (loading models), warm is
  the second read. The machine is shared with other builds; the numbers below are from a run at a load average of
  about 5. At a load of 10 to 40 the same pages took two to three times as long.

## Character error rate

Mean CER per set, lower is better. "Vision" is Apple Vision with automatic language (it cannot be told Hindi or
Bengali). "Tesseract (defaults)" is Tesseract's own page layout analysis, default segmentation mode, tessdata_fast.
"Tesseract + lines" is what the ladder uses for Bengali: the PP-OCR detector finds the lines and Tesseract reads them
(mode 13), and whichever of the two Tesseract readings found more words wins.

| set | n | Apple Vision | PP-OCRv5 | Tesseract (defaults) | Tesseract + lines |
|---|---|---|---|---|---|
| English, clean | 3 | 0.15% | 0.00% | 0.30% | n/a |
| English, scan | 3 | 0.15% | 0.00% | 0.60% | n/a |
| English, low-res | 3 | 0.00% | 0.00% | 0.15% | n/a |
| Hindi, clean | 3 | 97.56% (reads nothing) | 1.35% | 0.17% | n/a |
| Hindi, scan | 3 | 85.72% | 2.01% | 0.00% | n/a |
| Hindi, low-res | 3 | 91.62% | 2.08% | 0.00% | n/a |
| Hindi + English, clean | 1 | 100.00% | 1.68% | n/a | n/a |
| Hindi + English, scan | 1 | 100.00% | 2.52% | n/a | n/a |
| Hindi + English, low-res | 1 | 77.31% | 1.68% | n/a | n/a |
| Bengali, clean | 3 | 95.68% (reads nothing) | n/a | 19.98% | 8.04% |
| Bengali, scan | 3 | 91.62% | n/a | 18.93% | 2.15% |
| Bengali, low-res | 3 | 94.28% | n/a | 1.60% | 3.06% |
| corpus scans | 3 | 0.73% | 0.15% | 0.23% | n/a |

The two full corpus pages (300 DPI render, 2321 by 3054 and 1117 by 2496 pixels):

| page | engine | CER | WER | words | warm ms | peak MB |
|---|---|---|---|---|---|---|
| ia-scan-public-lands p15 | Apple Vision | 1.88% | 2.09% | 517 | 564 | 347 |
| ia-scan-public-lands p15 | PP-OCRv5 | 0.40% | 2.47% | 526 | 983 | 358 |
| ia-scan-public-lands p15 | Tesseract | 0.18% | 0.95% | 525 | 1323 | 228 |
| ia-scan-mining-claims p9 | Apple Vision | 0.31% | 0.74% | 269 | 322 | 285 |
| ia-scan-mining-claims p9 | PP-OCRv5 | 0.06% | 0.37% | 270 | 450 | 306 |
| ia-scan-mining-claims p9 | Tesseract | 0.50% | 2.22% | 268 | 658 | 203 |

Against the quality bars:

- "Under 2% on clean printed Latin and Devanagari at 300 DPI": met. English 0.00% to 0.15%, Hindi 1.35% (PP-OCR),
  corpus pages 0.06% to 1.88%.
- "At least equal to Tesseract defaults": met for English (0.00% against 0.30%) and for the corpus pages (0.15%
  against 0.23%). **Not met for Hindi**: Tesseract defaults read the generated Hindi at 0.0 to 0.17% against 1.35%
  to 2.08% for PP-OCR. Tesseract is not bundled (it needs a program on the machine), so the ladder keeps PP-OCR for
  Hindi, as the spec orders it. PP-OCR's Hindi errors are mostly a dropped danda ("।") and a lost conjunct mark.
  The set is synthetic print in two font families, so this is a comparison on easy text, not a verdict.
- "At least equal to Apple Live Text or Windows OCR": Apple Vision is the engine behind Live Text and is the first
  rung on a Mac, so on Latin the ladder is equal by construction. For Hindi and Bengali Vision reads nothing, so
  there is nothing to compare with. Windows OCR was not measured.
- Bengali: 2.2% to 8.0% with the ladder's reading against 1.6% to 20.0% for plain Tesseract. Tesseract's page layout
  analysis drops whole lines of small pictures (the first line of `bn-3` at most of the sizes tried, and of `bn-2` at some), and
  line mode 7 returned nothing at all for lines it reads inside a page; mode 13 on detector lines recovers most of
  that. `bn-3` clean is still 14.8% because one line is read wrongly by the model itself.

## Speed and memory

Per page, same run (load average about 5):

| engine | small pictures (1700 px wide), warm | full 300 DPI page, warm | peak, small picture | peak, full page |
|---|---|---|---|---|
| Apple Vision | 80 ms median | 322 to 564 ms | 88 MB median | 285 to 347 MB |
| PP-OCRv5 | 81 ms median | 450 to 983 ms | 221 MB median | 306 to 358 MB |
| Tesseract | 116 ms median | 658 to 1323 ms | 32 MB | 203 to 228 MB |

First read in a fresh process: Vision 350 ms median, PP-OCR 134 ms median once the files are cached (a first ever
run adds the downloads: 8 s on a good connection for detection, English recognizer and the runtime).

End to end, `panoply pdf ocr` on a 5 page scanned PDF (A4 pages holding 2321 by 3054 pixel PNGs, 2,392 words), wall time
including opening the PDF, drawing pages at 300 DPI, reading, and writing the new file (load average about 6):

| engine | total | per page | peak memory |
|---|---|---|---|
| Apple Vision | 3.2 s | 0.65 s | 289 MB |
| PP-OCRv5 (`PANOPLY_OCR_ENGINE=ppocr`) | 4.6 s | 0.93 s | 422 MB |

Bars: "under 2 s per page for printed Latin on a laptop" is met by both at a load of about 5 (a dense typewritten page
is the worst case here, 0.98 s with PP-OCR); at a load of 40 to 60 on this shared machine the same pages took 1.5 to
2.7 s. "Under 300 MB per job": **not met on the model path**. Vision is
near it (285 to 347 MB for the whole process) and PP-OCR a little over it (306 to 358 MB). About 180 MB of either is
drawing the page with pdfium (measured at the start of a PP-OCR run, before detection). Two measures already bought most of that:
detection runs in 384 pixel high strips with overlapping margins instead of one pass over the page (a full A4 page
needed 787 MB before, 358 MB after, same CER), and the detector's long side is capped at 1600 pixels. Batching the
recognizer made it slower (batches of 3 and 8 against 1, the padding costs more than the larger runs save), so it is
off. Switching ONNX Runtime's arena allocator off made peak memory worse (919 MB at the time), so it is on.

## The text layer

Written by `crates/panoply-ocr/src/textlayer.rs` and `searchable.rs`:

- Every word is drawn in render mode 3 at its box: font size is the box height, horizontal scaling stretches the
  characters to the box width. The font is a generated Type 0 composite font (Identity-H, one code per distinct
  character in the document) with a `/ToUnicode` map, a generated TrueType program with one glyph, and a
  `/CIDToGIDMap` stream. Any script works because viewers ask the map, not glyph shapes. pdfium (in
  `tests/searchable.rs`) and `pdftotext` extract the Hindi text from the output.
- Alignment: word boxes from Vision and from PP-OCR on the same page agree to a median of 0.04 to 0.09 line heights
  (largest 0.27) between box centres, and the overlay of the text layer on the printed page sits on the words. The
  bar is "within one line height". Pages with `/Rotate` 90, 180 and 270 (made from rotated copies of one scan) were
  checked against `pdftotext -bbox` overlays; they line up.
- Pictures are copied byte for byte: the test compares the decoded image bytes of the input and the output. The page's
  own content streams are copied unchanged and wrapped as `q`, original streams, `Q`, text layer.
- Pages that already have text (24 bytes or more of text strings, counting invisible text and text in form
  XObjects) are copied and not read. `ia-scan-public-lands.pdf` has 72 such pages out of 76. A file where no page
  needs reading, or where nothing was found, writes no new file.

Two things found on the way, both worth knowing:

1. **pdfium drops the characters of a font whose glyphs have empty bounds when the text runs along a page turned by
   `/Rotate` 90 or 270.** A text layer with an empty-glyph font (the usual "glyphless" trick) was extractable on
   upright pages and returned zero characters on rotated ones, which is what Chrome searches with. The generated glyph
   is therefore a plain box covering the character cell. It is never drawn because the text is invisible. A separate
   pdfium-render quirk: `PdfPageText::all()` clips to the page rectangle compared in the wrong space on rotated pages,
   so the tests read characters instead.
2. **`panoply-pdf`'s page renderer (`RenderDoc`, `set_fixed_size` plus strip origin) draws layered scans at the wrong
   scale.** On `ia-scan-public-lands.pdf` page 15 (JPEG 2000 background with a mask layer from LuraDocument), the page
   came out at one pixel per point in the top left corner of the bitmap at every DPI, which is also what
   `panoply pdf images` writes for that file. pdfium's own `scale_page_by_factor` draws the same page correctly. This
   crate draws its pages with its own `input::PageSource` for that reason; the renderer in `panoply-pdf` was not
   touched and still has the bug.

## Gaps

- **Page orientation is not detected.** A page that is sideways or upside down in the file is read as it is drawn;
  only `/Rotate` is honoured. Vision does not turn pages over either. The spec asks for detection before recognition.
- **PP-OCR is behind Tesseract on the generated Hindi** (1.35% to 2.08% against 0.0 to 0.17%) and over the 300 MB
  budget on a full page (306 to 358 MB measured as a whole process).
- **Tesseract is not bundled.** Bengali works only where a `tesseract` program is on the `PATH` (or
  `PANOPLY_TESSERACT`); without it Bengali is refused with a plain sentence. Rewriting this as a linked library or a
  downloaded build is a decision for the owner.
- **Windows OCR was never run**, only compile-checked. Its language coverage for Hindi and Bengali is unknown, and it
  reports no confidence (words carry 1.0). The whole crate was not cross-compiled for Linux or Windows because
  `panoply-image`'s C dependencies need a target toolchain; the Windows module was checked on its own.
- **No ONNX Runtime pin for macOS on Intel** (release 1.28.3 has no such archive): an Intel Mac reads Latin through
  Vision and needs `PANOPLY_ORT_LIB` for anything else.
- **Languages with a PP-OCR model not wired:** Tamil, Telugu, Arabic, Thai, Greek, Cyrillic recognizers exist
  upstream; here Arabic, Cyrillic, Thai and CJK are Vision only. Marathi and Nepali use the Devanagari recognizer and
  were not measured. The Latin recognizer (French and the other Latin languages without Vision) was only smoke-tested
  on English text. Right-to-left pages (Arabic) were not run end to end: the text layer is written in the engine's
  logical order and nothing checks selection direction.
- **No redo of pages that already have text** and no judgment of how good an existing layer is; a page with 24 or
  more bytes of text strings counts as done. No "replace the page image with a cleaned one" mode.
- **Layout:** reading order is an XY cut (columns before rows), paragraphs come from line spacing, Markdown from line
  heights and list markers. There is no table detection, no `RecognizeDocumentsRequest`, no vertical text on the PP-OCR
  path, no handwriting flag.
- **Vision's confidence is per line** and is almost always 1.0 in accurate mode, so "flagged for low confidence" is
  rare on a Mac. Pages where nothing was found are flagged instead.
- **Detection of skew:** crooked lines are straightened per line (rotated crops), but a skewed page is not
  straightened as a whole, and no "run Clean scans first" hint is shown.
- Auto language detection is a second look, not a classifier: Vision finds nothing or little, the picture looks like
  lines of text, so Devanagari and then Bengali are tried. A page of Tamil still reads as nothing.

## Reproduce

```sh
cargo build --release -p panoply-ocr --example ocr_eval
sh crates/panoply-ocr/testdata/run_eval.sh results.jsonl     # needs tests/corpus/cache for the corpus pages
python3 crates/panoply-ocr/testdata/summarize_eval.py results.jsonl
cargo test -p panoply-ocr                                    # downloads the pinned files on first run, skips offline
```
