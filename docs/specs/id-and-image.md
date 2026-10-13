# Module specs: ID, forms and document photos (section 2) and Images (section 3)

Template and shared capabilities: see FLOWS.md. Each spec lists only what is
special to the module. Passport and visa photo is specified in FLOWS.md
(worked example 2) and is not repeated here; it shares the models and the
preset schema below.

Metric names used below. **SSIM** is the structural similarity index.
**SSIMULACRA2** (Cloudinary, BSD-2) is a perceptual score used to define "equal
quality" between codecs. A "corpus" is a test set of real files we assemble and
publish in the repo, so every bar is reproducible. Competitor comparisons are
run by us on that corpus; no result is claimed until it is run.

Budgets. Every spec has a `Budgets:` line after its Pattern line. It says
"standard" when the module fits the table in FLOWS.md, otherwise the exception
and the reason. Models follow the Models budget: load on first use in a job,
unload after 60 s idle, size shown before the first download. Model sizes are
approximate and are re-read from each model's `MODEL.json`. Model-backed
modules that cannot show a first preview in 100 ms show the original at once
with a progress line and state the time to result.

License policy for models. Every model file ships with a `MODEL.json` (name,
version, license, source URL, weights hash, training data note). A model whose
license or weights terms are non-commercial or unclear is not shipped. Licenses
below were checked against model cards or repos in October 2026; items marked
"verify" must be re-read at the source before the first release. The project
license is not decided yet (VISION.md), so engines under GPL or AGPL are
flagged and avoided in the core.

## Image pipeline contract (IPC)

Every image module decodes through one pipeline so these cases behave the same
everywhere. Specs refer to them as IPC-n.

- **IPC-1 Orientation.** Apply EXIF orientation (all eight values, including
  the mirrored 2, 4, 5, 7) at decode and write orientation 1. Exception: lossless
  JPEG ops (see Rotate and flip) and HEIC `irot`/`imir`, which rotate in the
  container.
- **IPC-2 Color.** Decode with the embedded ICC profile. Display P3, Adobe RGB,
  ProPhoto and untagged images are handled explicitly. Resampling happens in
  linear light. Output is converted to sRGB for web and portal targets and keeps
  the profile for archival or print targets. Untagged means sRGB. CMYK JPEGs
  (including Adobe inverted-CMYK and YCCK) convert through lcms2 using the
  embedded profile, else a bundled open CMYK profile.
- **IPC-3 Auxiliary data in HEIC and JPEG.** Depth maps, portrait mattes, gain
  maps and thumbnails are separate images inside the file. We process the primary
  image only. Gain maps are kept when the output format can carry them (see HDR
  images), otherwise the SDR rendition is used and the result card says the depth
  or gain map was dropped. The Apple portrait matte, if present, is passed to
  background modules as a free hint.
- **IPC-4 Live Photos and motion photos.** The still is processed. The paired
  MOV (Apple content identifier) or the Google motion-photo trailer is not
  touched and not silently dropped: the card offers "Keep the video" (writes it
  next to the output) via the Video module.
- **IPC-5 Bit depth.** 16-bit PNG and TIFF and 10-bit AVIF/HEIC stay 16-bit
  internally. Down-conversion to 8-bit uses blue-noise dither, never truncation.
- **IPC-6 Animated.** Animated WebP, GIF, APNG and AVIF sequences: modules that
  make sense per frame run on all frames and keep timing and loop count; others
  ask once ("Use first frame or all frames") and default to all.
- **IPC-7 Huge images.** Strip or tile based streaming decode with a memory cap.
  Panoramas beyond format limits (JPEG 65535 px per side, WebP 16383 px) are
  detected before encoding, and we offer a format that fits or a downscale.
- **IPC-8 Alpha.** Premultiplied-correct resampling. Flattening to a format
  without alpha asks for a color, default white, with a live preview.
- **IPC-9 Metadata.** Kept by default for conversions, stripped by default for
  web and portal outputs. GPS is always named on the result card when kept.

Codec choices that apply to every module: JPEG (zune-jpeg decode, MozJPEG
encode), PNG (`png` crate, oxipng), WebP (libwebp, BSD-3), AVIF (libavif with
rav1e or SVT-AV1 encoders, BSD-2), JPEG XL (jxl-oxide decode, libjxl encode,
BSD-3), TIFF (`tiff` crate), resize (`fast_image_resize`), color (lcms2, MIT),
vector (resvg, MPL-2.0). HEIC: decode through OS codecs where present
(ImageIO, Windows HEVC extension) and libheif with libde265 (LGPL) elsewhere.
HEIC encode only through OS encoders, because x265 is GPL and HEVC is patent
encumbered; this needs a legal review before release.
PNG and GIF palette quantization: `imagequant` is GPL-3.0 or commercial and
`gifski` is AGPL-3.0, so neither goes in the core. We write our own
OKLab-space quantizer (Wu or k-means with blue-noise dithering). This is one of
the hardest pieces and is shared by Compress, Convert and GIF maker.

## Exam preset data format (used by sections 2.3, 2.4 and 2.5)

A preset is a JSON file, one per portal and field, in `presets/exams/`,
user-editable, with a "reset to shipped" button:

```
id, authority, form, field (photo|signature|thumb|declaration|document)
width_px, height_px, tolerance_px, physical_mm and dpi (when the rule is in cm)
min_bytes_claimed, max_bytes_claimed, unit_claimed ("KB")
kb_basis: "1000" | "1024" | "unknown"
format ("jpg"|"png"|"pdf"), background, color_mode, extra rules (text, strip)
source_url, source_quote, source_date, last_verified, status (verified|conflict|stale)
```

**KB handling.** If `kb_basis` is known, use it. If `unknown`, we pick a target
inside the intersection of both readings: at least `min * 1024` and at most
`max * 1000` bytes. For "20 to 50 KB" that is 20,480 to 50,000 bytes, and we aim
near the upper third of the window for quality. If the intersection is empty,
the preset is marked `conflict` and we show both readings and ask. The result
card always prints exact bytes and both KB readings ("48,213 bytes = 48.2 KB
(1000) = 47.1 KB (1024)").

**Staleness.** Specs vary across sources and change (tool-harvest section C
records conflicts for UPSC, Passport Seva and PAN). Seeded presets carry
`status: conflict` where the harvest found disagreement, and the UI shows the
source link beside every preset. Presets update through the same model and data
mirror, never silently.

---

# Section 2. ID, forms and document photos

## 2.1 Passport and visa photo

- **Budgets:** model-backed, OS engines first. Faces and the background cutout use the per-platform ladders of 3.16 and 3.8. OS engines: no download, no resident memory for us. The fallback face model (YuNet, under 1 MB) and the fallback cutout model (the 3.8 model, which carries its exception, see 3.8) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. Camera preview follows the camera frame rate (30 to 60 fps), live checks run on every third frame off the UI thread.

See FLOWS.md, worked example 2. Additions from this file: face detection and
landmarks use the models named in 3.16 and 3.6; background replacement uses
the cutout engine of 3.8; the rule files use the preset format above.

## 2.2 Print sheet

- **Module:** Print sheet. **Pattern:** B.
- **Budgets:** standard.
- **Job:** "Put several copies of this photo on one sheet for the print shop."
- **Inputs:** one or many images (also takes the output of 2.1 directly).
- **On drop:** reads the pixel size and, if the photo came from a preset or 2.1,
  the physical size in mm; otherwise reads DPI metadata and defaults to 35 x 45
  mm if a face fills the frame like a passport photo, else asks for the size.
  Locale picks the paper (US 4 x 6 in, India 4 x 6 in or A4, rest A4).
- **Options:** Paper (4x6 in, 5x7 in, A4, Letter; default by region). Copies
  (count; default is the maximum that fits with 2 mm gutters, or 6 for 4x6).
  Cut marks (on, thin corner ticks). Mix photos (a list of photos with a count
  each, default one). Margin and gutter (mm, defaults 3 and 2).
- **Preview:** the sheet with real-size rulers, a "prints at 100%" notice.
- **Output:** JPG at 300 DPI (600 optional) or a single-page PDF with exact
  physical size; "name (sheet).jpg".
- **Result card:** "8 photos, 35 x 45 mm each, A4, 300 DPI, 0.0 mm size error."
- **Edge cases:** photo with no DPI tag and no preset (ask size, never guess a
  print size); printers that add margins (PDF output with a "do not scale"
  note, plus a 1 px test row on request); mixed orientations (rotate to pack,
  not distort); Display P3 source (convert to sRGB for print shops, IPC-2);
  transparent PNG (flatten white, IPC-8).
- **Quality bar:** printed size within 0.2 mm of the stated size when measured on
  the PDF; packing count equals the best of a brute-force search on the corpus.
- **Engine:** own layout code, `fast_image_resize`, `printpdf`-class PDF writer.
- **Surfaces:** panel, right-click, CLI `panoply id sheet photo.jpg --paper a4`,
  MCP `id_print_sheet`.

## 2.3 Resize to exact KB

- **Module:** Resize to exact KB. **Pattern:** B.
- **Budgets:** standard. The exact-bytes preview encodes in the background; the crop box shows at once.
- **Job:** "Make this photo 20 to 50 KB and 200 x 230 px for the form."
- **Inputs:** one or many images (JPG, PNG, HEIC, WebP, TIFF).
- **On drop:** reads size, pixels and aspect ratio, finds a face (detector of 3.16)
  and whether the file already meets a preset; if a preset was chosen from the
  bar ("ssc photo") it loads it and everything below is prefilled.
- **Options:** Preset (exam preset list, default none, or the last used).
  Pixels (w x h, default from preset else current size). Size window (min and
  max bytes with unit and `kb_basis`; default from preset). Fit (crop to
  fill around the face, pad, or stretch; default crop with the face kept, pad
  white if the aspect gap exceeds 15%). Format (JPG default).
- **Preview:** exact predicted bytes (we encode in the background, not guess),
  the crop box over the photo, and both KB readings.
- **Output:** "name (200x230, 48KB).jpg". The encoder searches quality with
  MozJPEG until the result lands inside the window. If the window's minimum is
  above what the pixels reach at quality 95, we raise quality and then
  chroma resolution (4:4:4); only as a last step, when a portal enforces a
  minimum size, do we pad with a JPEG comment segment, and the card says so.
- **Result card:** "200 x 230 px, 47,980 bytes (47.9 KB or 46.9 KB), inside
  20 to 50 KB under both readings."
- **Edge cases:** a min size above what a tiny image can reach; a 12 MP photo
  needing a 20 KB result (downscale first, then chroma subsample 4:2:0, then
  quality); PNG with transparency (IPC-8); CMYK or P3 (convert to sRGB, IPC-2);
  EXIF orientation (IPC-1, the classic "my photo is sideways on the portal" bug);
  portals that reject progressive JPEG (preset flag `baseline_only`, default
  baseline for presets, progressive otherwise); strip all EXIF and the ICC
  profile for presets that cap size at 20 KB.
- **Quality bar:** hits the window in one pass on 100% of the corpus (500 real
  phone photos across the preset list) and, at equal bytes, SSIM at least equal
  to PhotoKB-class sites and iLoveIMG on the same pixels; runs with no upload.
- **Engine:** MozJPEG, `fast_image_resize`, binary search on quality, optional
  trellis on, own face-aware crop.
- **Surfaces:** bar ("photo 20 to 50 kb"), panel, right-click, CLI
  `panoply image kb photo.jpg --min 20kb --max 50kb --size 200x230`, MCP
  `image_resize_to_kb`.

## 2.4 Exam and portal presets

- **Module:** Exam and portal presets. **Pattern:** B (picker plus 2.3 engine).
- **Budgets:** standard. No model of its own; classification uses the tiny face detector of 3.16.
- **Job:** "Give me the right size for my exam form, photo and signature."
- **Inputs:** one or many images, or a folder; takes photo, signature, thumb and
  declaration in one drop and assigns each.
- **On drop:** classifies each file by content (face present means photo;
  mostly white with dark ink strokes means signature; fingerprint ridge texture
  means thumb; text lines means declaration) and shows them against the chosen
  exam's fields; wrong-way uploads are flagged ("this looks like a signature,
  the slot wants a photo").
- **Options:** Exam and form (searchable list, default from region and recent;
  seeded from tool-harvest section C: SSC, UPSC, RRB, Passport Seva, PAN
  NSDL and UTIITSL, Bangladesh Teletalk photo and signature, NID, Bangladesh
  passport; Aadhaar, DigiLocker and Pakistan CNIC are not seeded because the
  harvest did not verify them). Field assignments (auto). Editing a number
  saves a user copy and keeps the shipped one.
- **Preview:** one row per field with the rule, source link, status badge
  (verified, conflict, stale) and the result.
- **Output:** one file per field, "name (ssc-photo).jpg"; optional single zip.
- **Result card:** per field pass or fail with exact bytes under both KB
  readings, and a "last verified" date.
- **Edge cases:** conflicting sources (UPSC and Passport Seva are `conflict` in
  the seed data: we show both readings and default to the stricter); a portal
  that changed after our date (the status shows "stale after 180 days" and the
  link to re-check); mixed 1000 and 1024 portals (see Exam preset format);
  signature that needs a given physical size at DPI (we convert mm to pixels).
- **Quality bar:** every seeded preset has a source link, quote, date and a
  test image in CI that passes the arithmetic; against exam photo sites
  (PhotoKB, SignResizer, EasyResizeImage), we win on being offline, on showing
  the source, and on explicit KB handling. We do not claim "accepted by the
  portal" without a portal check; the card says "meets the published rule".
- **Engine:** the 2.3 engine plus the data files; no model of its own.
- **Surfaces:** bar ("ssc photo and signature"), panel, CLI
  `panoply id exam --form ssc --photo p.jpg --signature s.jpg`, MCP
  `id_exam_preset`.

## 2.5 Name and date strip

- **Module:** Name and date strip. **Pattern:** B.
- **Budgets:** standard. The final re-encode of a small photo finishes well under 100 ms.
- **Job:** "Add my name and the photo date under the exam photo."
- **Inputs:** one photo (typically already sized by 2.3).
- **On drop:** reads the photo date from EXIF; if none, uses file date and says
  so, because forms usually want the date it was taken; reads the name from
  saved details (Panoply's "my details", local only) or leaves it blank.
- **Options:** Name (text, default saved name, uppercase on). Date (date,
  default EXIF date, format default from the preset such as DD/MM/YYYY).
  Strip (below the photo adding height, or over the bottom edge; default
  below, because exam rules usually want an unobstructed face). Font size
  (auto-fit so the text fits one line).
- **Preview:** photo with the strip at real pixel size, and the final byte
  count against the window (the strip must not push the file over the cap,
  so this module re-runs the 2.3 encoder at the end).
- **Output:** "name (strip).jpg".
- **Result card:** "200 x 260 px, 31,420 bytes, strip: RAHUL SHARMA 12/03/2026."
- **Edge cases:** very long names (shrink down to a floor of legible size, then
  warn); non-Latin names (Hindi, Bengali, Urdu): shaped with a real text shaper
  and a bundled Noto font, not per-glyph drawing; strip makes the image fail the
  exact pixel rule (offer "make the photo area smaller so the total stays
  200 x 230").
- **Quality bar:** text pixel-sharp at 100% and legible at 20 KB; shaping is
  correct on a test set of Devanagari and Bengali conjuncts, which most
  online strip tools mangle.
- **Engine:** `cosmic-text` or `swash` with HarfBuzz-class shaping, Noto fonts
  (OFL).
- **Surfaces:** panel, right-click, CLI `panoply id strip p.jpg --name "A B"`,
  MCP `id_name_date_strip`.

## 2.6 Thumb impression and declaration

- **Module:** Thumb impression and declaration. **Pattern:** D.
- **Budgets:** standard. OS engines: no download, no resident memory for us. The optional ink-mask model is not shipped in v1; if it ships it downloads on first use and unloads after 60 s idle. Camera preview follows the camera frame rate (30 to 60 fps), live checks run on every third frame off the UI thread.
- **Job:** "Turn my photo of an inked thumb print or handwritten declaration
  into what the form wants."
- **Inputs:** one photo or scan, or camera.
- **On drop:** detects paper boundaries (page quad), ink mask, and whether it
  is a thumb print (single dark blob with ridge texture, roughly square crop)
  or a declaration (several text lines); sets mode and the exam preset
  fields (thumb often 240 x 240 or similar; declaration often a full page under
  a size; the actual numbers come from presets, not from here).
- **Options:** Mode (thumb or declaration, auto). Preset (from 2.4). Ink
  (keep paper color, whiten paper, black and white; default whiten paper).
  Thumb crop (square around the print, auto).
- **Preview:** live checks: print fully inside the frame, not smudged (ridge
  contrast), no paper edge or shadow, ink dark enough; fix buttons for
  shadow removal and contrast.
- **Output:** JPG or PNG to the preset, under its size window.
- **Result card:** "Thumb 240 x 240 px, 12.4 KB, ridge contrast good."
- **Edge cases:** phone shadow across the paper (background-estimate division,
  not a blunt threshold); a blurred print (we say so and ask for a retake,
  never "enhance" ridges because that invents detail on a biometric); tilted
  paper; red or blue inks; HEIC with Live Photo (IPC-3, IPC-4).
- **Quality bar:** on 100 real phone photos of paper, background whitened with
  no residual shadow in more than 95% of them (our target) and ink strokes
  preserved (SSIM on the ink mask against a flatbed scan of the same page).
- **Engine:** classical page-quad detection and illumination flattening in Rust
  (OpenCV-class, Apache-2.0 or our own). Ink mask model: OS engine: none.
  Own model on every platform; it is optional, a small segmentation model
  trained by us on permissive data (not shipped in v1).
- **Surfaces:** panel, phone camera, CLI `panoply id thumb scan.jpg --form ssc`,
  MCP `id_thumb_declaration`.

## 2.7 Documents under a size

- **Module:** Documents under a size. **Pattern:** B.
- **Budgets:** standard. OCR follows the 2.11 ladder: OS engines first (no download, no resident memory for us), the fallback OCR models (about 15 MB per language) only where the quality bar test picks them, loaded per job and unloaded after 60 s idle. Pages are recompressed one at a time.
- **Job:** "Make a PDF or JPG of this document under 100 KB for an upload."
- **Inputs:** images, scans or PDFs, single or many (many become pages).
- **On drop:** page count, whether pages are text or photographic, whether
  the PDF has real text (keep it), and the smallest size that stays readable
  (OCR confidence on a downscaled copy, which sets the lowest allowed DPI).
- **Options:** Target (under 50, 100, 200, 500 KB, custom; default from the
  last preset or 100 KB). Output (PDF default, JPG for a single page).
  Grayscale (on for text pages). Min readability (auto; a floor of about 150
  DPI equivalent on text, the user can lower it).
- **Preview:** the first page at 200% with the predicted size, and a
  readable/not-readable badge from OCR confidence.
- **Output:** "name (under 100KB).pdf". Pages are recompressed with 1-bit
  JBIG2-free techniques (adaptive threshold plus CCITT G4 or optimized
  PNG for pure text) and JPEG for photographic pages.
- **Result card:** "3 pages, 96,412 bytes (96.4 KB or 94.2 KB), readable."
- **Edge cases:** cannot reach target without becoming unreadable (stop at the
  floor and say what the best reachable size is, never output an unreadable
  file); phone photos with perspective (offer the scanner, 2.9); multi-page
  from mixed HEIC and JPG (IPC-1 on each).
- **Quality bar:** on the corpus of 100 scanned forms, text OCR character error
  rate within 1 point of the original at the target, and smaller than iLoveIMG
  and the same-size path of Compress PDF at equal OCR accuracy.
- **Engine:** shares the compression stack of the PDF module (pdfium, MozJPEG,
  CCITT writer); JBIG2 is avoided because of past character-substitution
  errors in lossy JBIG2.
- **Surfaces:** bar ("jpg to pdf under 100 kb"), panel, CLI
  `panoply image doc-under scan.jpg --size 100kb --out pdf`, MCP
  `image_document_under_size`.

## 2.8 Signature cleaner

- **Module:** Signature cleaner. **Pattern:** D.
- **Budgets:** standard. Camera preview follows the camera frame rate (30 to 60 fps), live checks run on every third frame off the UI thread.
- **Job:** "Turn the photo of my signature into a clean transparent PNG at the
  size the form wants."
- **Inputs:** one photo or the camera; also takes a drawn signature.
- **On drop:** finds the ink strokes versus paper and shadow, ink color (blue,
  black), stroke thickness and bounding box; sets crop with 4% padding.
- **Options:** Ink color (keep, black, blue; default keep). Background
  (transparent PNG default; white JPG when the preset says so). Size (from a
  preset such as SSC 140 x 60 px or 6 x 2 cm, or custom). Thickness
  (auto-compensates thin strokes lost when downscaling).
- **Preview:** the signature over checker and over white, at the final size,
  with the byte count against the preset window.
- **Output:** "name (signature).png" or JPG.
- **Result card:** "140 x 60 px, transparent PNG, 6.1 KB."
- **Edge cases:** ruled or lined paper (remove lines that are thinner and
  straighter than the strokes, ask before removing); ballpoint with gaps (keep
  gaps, do not close them); small image cropped too tight; heavy shadow or a
  gradient from the phone (illumination flattening, as in 2.6); downscaling
  thin strokes to 140 px (premultiplied alpha resampling, IPC-8, so strokes do
  not turn gray and fringe); PNG transparency that some portals reject (the
  preset can force white JPG).
- **Quality bar:** stroke recall and precision against hand-labelled masks
  on 100 real signatures; no halo on a dark background; and smaller PNG than
  the same-size output of a generic remove-background site at equal edge
  accuracy. Must beat: online signature-resize sites on shadow handling.
- **Engine:** classical illumination flattening and adaptive thresholding, then
  alpha from ink darkness; optional small model later (see 3.8 fallback).
- **Surfaces:** panel, phone camera, bar ("signature"), CLI
  `panoply id signature sig.jpg --size 140x60`, MCP `id_signature_clean`.

## 2.9 Document scanner

- **Module:** Document scanner. **Pattern:** D.
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback models, a small corner-regression model (target under 20 MB) and OCR models (about 15 MB per language, size shown before the first download), download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. Camera preview follows the camera frame rate (30 to 60 fps), live checks run on every third frame off the UI thread.
- **Job:** "Photograph a page and get a flat, readable scan, or a multi-page PDF."
- **Inputs:** camera (live), photos (HEIC, JPG), or a batch of photos as pages.
- **On drop:** page quad, orientation (text direction for upright rotation),
  color vs mono page, text language hint for OCR, glare and shadow areas.
- **Options:** Mode (auto, color, grayscale, black and white; default auto by
  page content). Output (PDF, JPG). Searchable text (on if a text language is
  found; default on). Page size (auto, A4, Letter). Corners (auto, draggable).
- **Preview:** the flattened page, the detected quad, live capture
  checks (glare, blur, cut-off corner) and an auto-capture when steady.
- **Output:** one multi-page PDF, "scan 2026-10-13.pdf", pages reorderable.
- **Result card:** "4 pages, A4, searchable, 612 KB."
- **Edge cases:** page on a busy desk (quad from edges plus a trained
  quad model, with manual fix); curved book pages (dewarp is v2, we say
  "flattened, not dewarped"); HEIC with depth (IPC-3, ignored); sensor
  noise in low light; multi-page order from mixed orientations; page edge
  cut off (refuse and ask for a retake).
- **Quality bar:** corner error under 1% of page diagonal on a corpus of 200
  phone photos, OCR character error rate no worse than Apple Notes or Microsoft
  Lens scans of the same photos, and speed under 1 s per page on a mid-range
  phone.
- **Engine:** OS first, own model second. Page quad: iOS VisionKit
  `VNDocumentCameraViewController`, and macOS and iOS Vision
  `VNDetectDocumentSegmentationRequest`. Android: ML Kit Document Scanner
  (`GmsDocumentScanning`). Windows: none found. Linux: fallback. Fallback
  (any platform where the OS engine is absent or fails the quality bar test in
  CI): classical edge and line quad detection plus a small corner regression
  model trained on permissive data (no shipped model is named yet;
  DocAligner-class models need a license check). Text layer OCR: OS first,
  own model second. macOS and iOS: Apple Vision `VNRecognizeTextRequest`, and
  `RecognizeDocumentsRequest` (iOS and macOS 26). Android: ML Kit Text
  Recognition v2. Windows: `Windows.Media.Ocr` (`OcrEngine`), and Windows AI
  Text Recognition on Copilot+ PCs. Linux: fallback. Fallback: PaddleOCR PP-OCR
  ONNX (Apache-2.0). Own code on every platform: perspective warp, adaptive
  threshold.
- **Surfaces:** panel, phone camera flow, share sheet, CLI
  `panoply image scan *.jpg --out scan.pdf`, MCP `image_document_scan`.

## 2.10 ID card scan

- **Module:** ID card scan. **Pattern:** D.
- **Budgets:** standard. Shares the 2.9 quad stack and its model rules. Camera preview follows the camera frame rate (30 to 60 fps), live checks run on every third frame off the UI thread.
- **Job:** "Front and back of my ID on one page, ready to print or upload."
- **Inputs:** two photos (or one with both sides), or camera for each side.
- **On drop:** finds the card quad, aspect ratio (ISO ID-1 85.6 x 54 mm),
  which side is front (face, text density), and flags glare on the laminate.
- **Options:** Layout (both on A4 stacked at true size default; or each as its
  own image). Size (true card size, or enlarged to fit width). Mask numbers
  (off by default; one tap runs 2.11). Border (thin gray, on).
- **Preview:** page layout with the card at exactly 85.6 x 54 mm.
- **Output:** PDF or JPG, "name (id).pdf".
- **Result card:** "2 sides, A4, true size, 280 KB, glare warning on back."
- **Edge cases:** holographic glare (warn and suggest tilt, never fake);
  rounded corners with a dark desk (keep the corner alpha); non-ID-1 documents
  such as an A5 paper ID (use measured quad aspect); card with a photo
  that is itself a face, so face detection must not be confused with the
  holder's live selfie.
- **Quality bar:** card quad error under 1% of the long side on 100 phone
  photos, true-size error under 0.5 mm on the printed PDF.
- **Engine:** shares the 2.9 quad stack, `printpdf`-class writer.
- **Surfaces:** panel, phone camera, CLI `panoply id card front.jpg back.jpg`,
  MCP `id_card_scan`.

## 2.11 Mask ID numbers

- **Module:** Mask ID numbers. **Pattern:** C (editor with auto-detect).
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback PaddleOCR models (about 15 MB per language, size shown before the first download) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them.
- **Job:** "Hide everything but the last 4 digits before I send my ID."
- **Inputs:** images and PDFs of ID cards, utility bills, forms.
- **On drop:** OCR finds digit runs and matches known ID shapes (12-digit
  Aadhaar style, 10-character PAN style, passport numbers, 16-digit cards,
  SSN-style), by regex over OCR text plus a layout hint; boxes appear
  pre-drawn.
- **Options:** Keep last (4 default, 0 to 6). Style (solid black default;
  blur and pixelation are not offered because they can be reversed).
  Also mask (checkboxes for QR codes, barcodes, MRZ lines, signatures; default
  QR and barcode on, since an Aadhaar-style QR carries the number).
- **Preview:** the image with boxes the user can drag or delete, and a
  "what a recipient sees" view.
- **Output:** a flattened new image or PDF; for PDFs the text under the box is
  removed from the file, not just covered, and the result says so.
- **Result card:** "5 areas masked, text removed from PDF, metadata stripped."
- **Edge cases:** digits split into groups by spaces or hyphens; a vertical or
  rotated number; low OCR confidence (box shown in orange for review, never
  silently skipped); PDF with an OCR text layer (remove it, then verify with a
  redaction check, section 14); GPS and thumbnails in the photo (strip, IPC-9;
  EXIF thumbnails often keep an unmasked copy).
- **Quality bar:** recall on the corpus of synthetic and real ID samples at
  least 99% for digit runs of known formats (our target), with a leak test that
  re-OCRs the output and the PDF text layer and finds zero unmasked
  full numbers.
- **Engine:** OS first, own model second. macOS and iOS: Apple Vision
  `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` (iOS and macOS 26).
  Android: ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese,
  Korean). Windows: `Windows.Media.Ocr` (`OcrEngine`), and Windows AI Text
  Recognition on Copilot+ PCs. Linux: fallback. Fallback model (any platform
  where the OS engine is absent or fails the quality bar test in CI):
  PaddleOCR PP-OCR detection and recognition via ONNX Runtime (Apache-2.0).
  Own code on every platform: regex library, rasterized flatten.
- **Surfaces:** panel, right-click "Mask ID numbers", phone share, CLI
  `panoply id mask card.jpg --keep 4`, MCP `id_mask_numbers`.

## 2.12 DPI and print size

- **Module:** DPI and print size. **Pattern:** B.
- **Budgets:** standard. Tag-only mode rewrites header bytes and never decodes pixels.
- **Job:** "Set this image to print at 10 x 15 cm", or "what is the DPI".
- **Inputs:** images (JPG, PNG, TIFF, HEIC); PDF pages optionally.
- **On drop:** reads pixels, the DPI tag (and its unit, the many files with
  72 or 96 that mean nothing) and computes the real print size at 150, 300 and
  600 DPI plus a quality verdict.
- **Options:** Target size (mm, cm, in, or a paper such as 4 x 6 in; default
  none). Mode (just change the DPI tag without touching pixels, default;
  or resample to hit the pixel count for a DPI). DPI (300 default, 150 for
  posters).
- **Preview:** "At 300 DPI this prints 8.9 x 5.9 cm" and a ruler graphic; an
  amber warning under 150 DPI at the target size.
- **Output:** "name (300dpi).jpg"; tag-only mode edits the header without
  re-encoding the JPEG (bit-exact pixels).
- **Result card:** "pixels unchanged, 300 DPI, prints 10.2 x 6.8 cm".
- **Edge cases:** DPI in PNG `pHYs` is in pixels per metre (convert correctly);
  TIFF resolution unit none; EXIF DPI and JFIF DPI disagree (we write both);
  sensor images with 72 DPI that people think are low quality (explain);
  upscaling to reach DPI (offer 3.11 and say it invents detail).
- **Quality bar:** exact, not statistical: size error 0 pixels when resampling
  and 0 bytes changed in pixel data for tag-only mode, checked by hash of
  the decoded pixels.
- **Engine:** header editing (`img-parts`), `fast_image_resize`.
- **Surfaces:** bar ("300 dpi"), panel, CLI `panoply image dpi a.jpg --dpi 300`,
  MCP `image_dpi_print_size`.

---

# Section 3. Images

## 3.1 Convert

- **Module:** Convert (image). **Pattern:** A. The generic route lives in the
  Universal converter (FLOWS.md example 3); this spec is the image leg only.
- **Budgets:** standard, except RAW development: peak memory scales with sensor size (about 8 bytes per pixel, so a 45 MP file is about 360 MB). Tiled demosaic is the target for staying inside 300 MB. All other formats decode and encode in strips.
- **Job:** "Turn this image into that format."
- **Inputs:** HEIC, JPG, PNG, WebP, AVIF, JXL, TIFF, BMP, GIF, ICO, RAW (DNG,
  CR2, CR3, NEF, ARW, RAF, ORF), SVG; single or many, mixed.
- **On drop:** detects the real format from bytes, bit depth, color space,
  alpha, animation, auxiliary images (IPC-3), and ranks targets by source:
  HEIC to JPG, WebP to PNG, PNG to JPG, anything to WebP or AVIF.
- **Options:** Target format (top 4 buttons). Quality (default by format:
  JPG 85 MozJPEG, WebP 80, AVIF chosen by SSIMULACRA2 target, PNG lossless).
  Alpha flatten color (white, shown only when needed). Keep metadata (default on,
  off for web targets).
- **Preview:** estimated size per target and a "what changes" line (loses alpha,
  loses animation, loses gain map).
- **Output:** "name.jpg" next to the original, "name (1).jpg" on clash.
- **Result card:** size before and after, "kept: color profile, EXIF; dropped:
  depth map, Live Photo video".
- **Edge cases:** HEIC from iPhone with depth and gain map (IPC-3: we render
  the SDR primary, the gain map is applied faithfully when the target is
  Ultra HDR JPEG, AVIF or JXL); HEIC burst or multi-image (export each image or
  the primary, ask once); Live Photo (IPC-4); CMYK JPEG to PNG (IPC-2, so
  colors do not go neon); 16-bit PNG to JPG (IPC-5); animated WebP to GIF (IPC-6,
  with palette limits warned); transparent PNG to JPG (IPC-8); P3 to sRGB
  (IPC-2); huge panorama to WebP (IPC-7); TIFF with multiple pages (export all);
  RAW with embedded preview versus full demosaic (default full develop with
  camera profile, "fast preview JPG" option); ICO with several sizes.
- **Quality bar:** pixel-exact for lossless to lossless paths (tested by hash of
  decoded pixels on the corpus); lossy paths at no more than 1 generation of
  loss; HEIC to JPG colors match macOS Preview within delta E 1 on the corpus;
  must beat ImageMagick defaults (profile handling) and iLoveIMG and
  CloudConvert-class sites (they strip color profiles).
- **Engine:** codec set in the pipeline contract; RAW through LibRaw
  (LGPL-2.1 or CDDL) or `rawler`, verify the license; SVG through resvg.
- **Surfaces:** bar ("heic to jpg"), rename-to-convert, right-click, share sheet,
  CLI `panoply image convert in.heic --to jpg`, MCP `image_convert`.

## 3.2 Compress

- **Module:** Compress (image). **Pattern:** B.
- **Budgets:** standard. The first preview is encoded from the visible 200% crop, so it appears under 100 ms. The full-image size and score refine in the background.
- **Job:** "Make this image smaller without it looking worse, or under a size."
- **Inputs:** JPG, PNG, WebP, AVIF, JXL, GIF, TIFF, HEIC; single or many.
- **On drop:** measures how the file was encoded (JPEG quality estimated from
  quantization tables, chroma subsampling, whether it is already MozJPEG-like),
  whether PNG is photographic or flat art, whether alpha is used, EXIF weight.
- **Options:** Target (Smallest that looks the same, default; Under a size;
  Quality slider). Keep format (default) or allow format change (off). Strip
  metadata (on, GPS always listed). Lossy PNG (on for flat art, off for
  photos).
- **Preview:** before and after split at 200%, the SSIMULACRA2 score and the
  predicted bytes; a "difference" toggle. The first preview is encoded from the
  visible crop only, and the full-image figures refine in the background.
- **Output:** "name (compressed).jpg"; if no gain, the original is kept and we
  say "already well compressed, saved 2%", never inflate.
- **Result card:** "3.2 MB to 610 KB, 81% smaller, quality score 82".
- **Edge cases:** progressive JPEG from phones; JPEG already at quality 60
  (do not recompress blindly, estimate the original quantization and avoid
  double-quantization banding); PNG with 16-bit (IPC-5; lossy mode quantizes to
  8-bit and says so); animated PNG and WebP (IPC-6); CMYK JPEG (keep CMYK or
  convert, ask once); EXIF orientation preserved or baked (IPC-1); P3 images
  keep the profile unless "web" is chosen (IPC-2); under-a-size target below
  achievable (report the floor).
- **Quality bar:** at equal SSIMULACRA2 (we publish thresholds, for example
  score 80) our output is no bigger than TinyPNG and Squoosh (MozJPEG) on a
  corpus of 200 real JPEGs and 200 real PNGs, and the byte count under a size
  target lands within 2% below the target.
- **Engine:** MozJPEG with trellis, `oxipng` plus our OKLab quantizer (see
  the pipeline note), libwebp, libavif; per-image quality search driven by
  SSIMULACRA2 (BSD-2 Rust port).
- **Surfaces:** bar ("compress image", "make this under 200 kb"), right-click,
  CLI `panoply image compress a.jpg --under 200kb`, MCP `image_compress`.

## 3.3 Format shootout

- **Module:** Format shootout. **Pattern:** B.
- **Budgets:** exception on first preview: AVIF at slow speed cannot finish in 100 ms. Rows stream in as each codec finishes (JPEG and WebP first, from a tiled sample). The 10 s per image cap and the CPU budget keep the machine responsive.
- **Job:** "Which format is smallest for this image, at the same look?"
- **Inputs:** one or many images, any raster format.
- **On drop:** classifies content (photo, flat graphic, screenshot, line art,
  has alpha, animated) to order the candidates and skip silly ones (no lossy for
  tiny icons).
- **Options:** Quality bar (SSIMULACRA2 target, default 80; or "visually
  lossless" 90). Candidates (JPG, WebP, AVIF, JXL, PNG; default all that the
  destination supports, destination chosen as "web", "email", "archive").
  Time budget (default 10 s per image; AVIF at slow speed is capped).
- **Preview:** a table of format, size and score with the winner highlighted,
  and an image slider for any row.
- **Output:** the winner as "name (webp).webp", or all as a folder.
- **Result card:** "AVIF 142 KB beats WebP 171 KB and JPG 238 KB at the same
  quality score."
- **Edge cases:** JXL not supported by the user's destination (Safari and
  Chrome support moves, so destination is a data file with dates); alpha and
  animation rule out JPG; HDR gain map (only AVIF, JXL and Ultra HDR JPEG
  qualify, IPC-3); P3 (formats with ICC support only); huge files (candidates
  run on a tiled sample first, then the winner on the full image, IPC-7).
- **Quality bar:** the chosen winner is the true smallest in at least 95% of
  a brute-force search (our target), and the module is faster than running
  Squoosh manually per format, which is the closest competitor.
- **Engine:** the Compress search loop over each codec, in parallel within the
  CPU budget.
- **Surfaces:** panel, CLI `panoply image shootout a.png`, MCP
  `image_format_shootout`. Recommend merging into Compress as a "Try every
  format" toggle; see the summary.

## 3.4 Optimize for web

- **Module:** Optimize for web. **Pattern:** A.
- **Budgets:** standard. Chained Resize and Compress. The watched-folder surface is event driven, zero polling.
- **Job:** "Make these ready for my website in one click."
- **Inputs:** a folder or many images.
- **On drop:** finds the largest dimension and display hints (if the folder
  contains HTML or Markdown that references the images, the rendered width is
  read from `width` or CSS where explicit), alpha, photos versus graphics.
- **Options:** Max width (default 2000 px, or the 2x of the referenced display
  width when known). Formats (WebP plus AVIF fallback, default WebP). Responsive
  set (off; when on: 480, 768, 1280, 1920). Strip metadata (on). Keep color
  profile (sRGB conversion, on).
- **Preview:** totals: "42 images, 86 MB to 7.4 MB" and a worst offender list.
- **Output:** "optimized/" folder next to the originals, same names, optional
  `srcset` snippet copied to the clipboard.
- **Result card:** total before and after, number of files, snippet copied.
- **Edge cases:** images with text (avoid chroma subsampling 4:2:0 for UI
  screenshots, use 4:4:4 or lossless WebP); EXIF orientation baked (IPC-1);
  P3 photos converted to sRGB so colors match across browsers (IPC-2); animated
  GIF to animated WebP or video (IPC-6, the GIF to MP4 offer); SVGs go to 3.29;
  transparent PNG kept transparent (IPC-8); LCP hero image gets the larger
  set.
- **Quality bar:** equal SSIMULACRA2 to the originals at or above 80 for
  photos, and total bytes no larger than Squoosh with matching settings or an
  ImageOptim-class pipeline across a corpus of 20 real sites' image folders.
- **Engine:** the Resize and Compress engines chained; `srcset` generator.
- **Surfaces:** bar ("optimize for web"), watched folder, right-click, CLI
  `panoply image web ./images --max 2000`, MCP `image_optimize_web`.
  Recommend implementing as a saved chain (Resize then Compress) rather than a
  separate engine.

## 3.5 Resize

- **Module:** Resize. **Pattern:** B.
- **Budgets:** standard.
- **Job:** "Make this exactly 1080 x 1350, or half size, or Instagram size."
- **Inputs:** any raster images, single or many.
- **On drop:** reads pixels, aspect ratio, DPI, subject (face or salient
  region, for fit), and whether this is up or down scaling.
- **Options:** Mode (pixels, percent, longest side, preset; default longest side
  or the last used). Preset (130+ platform sizes in a data file with source
  link, "last verified" date). Fit (contain, cover with smart crop, stretch;
  default contain). Resampling (auto: Lanczos3 for down, Mitchell-like for up).
  Do not enlarge (on).
- **Preview:** result dimensions, the crop box if cover, and an estimated file
  size.
- **Output:** "name (1080x1350).jpg" in the same format.
- **Result card:** "4032 x 3024 to 1080 x 810, 1.9 MB to 190 KB".
- **Edge cases:** resizing in linear light so dark and bright detail keeps
  correct brightness (IPC-2, the cause of dark, muddy resizes); premultiplied
  alpha (IPC-8); animated images resized frame by frame (IPC-6); 16-bit stays
  16-bit (IPC-5); EXIF orientation applied before sizing (IPC-1); platform
  presets that changed (shown with a verified date and an edit option); huge
  panoramas (tiled, IPC-7).
- **Quality bar:** on a downscale corpus, SSIM against a 4x supersampled
  reference at least equal to libvips Lanczos3 and Photoshop-like bicubic
  sharper, no halos (ringing under 1% overshoot), and 130+ presets each with
  a source link. Must beat iLoveIMG on color accuracy (linear light) and on
  not re-encoding more than once.
- **Engine:** `fast_image_resize` (Lanczos3, SIMD), lcms2 for color.
- **Surfaces:** bar ("resize to 1080"), panel, right-click, CLI
  `panoply image resize a.jpg --long 1080`, MCP `image_resize`.

## 3.6 Crop

- **Module:** Crop. **Pattern:** C (editor) with an auto proposal.
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback face detector (YuNet, under 1 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. The small saliency model (target under 10 MB) has no OS engine, so it loads on first use and unloads after 60 s idle on every platform.
- **Job:** "Cut this to a shape, a ratio, or around the subject."
- **Inputs:** any raster images; batch applies the same ratio per image.
- **On drop:** finds faces and the salient subject, horizon and thirds, and
  proposes a crop for the last used ratio.
- **Options:** Ratio (free, 1:1, 4:5, 3:2, 16:9, 9:16, passport, social presets;
  default last used). Smart (on: keeps the subject and faces inside, default
  on for batch). Exact pixels (optional).
- **Preview:** the crop box with a rule-of-thirds grid and the output pixel size.
- **Output:** "name (crop).jpg"; for JPEG with MCU-aligned boxes we offer a
  lossless crop (no re-encode), snapping the box to 8 or 16 px.
- **Result card:** dimensions and "lossless crop" when applicable.
- **Edge cases:** EXIF orientation (IPC-1, crop boxes are in the displayed
  orientation); lossless crop of a progressive JPEG (supported) and of HEIC
  (grid items, otherwise re-encode and say so); animated (IPC-6); a face cut
  in half by the proposed crop (reject and shift); batch with mixed
  orientations (portrait and landscape get a rotated ratio, not a stretched one).
- **Quality bar:** the smart crop keeps all detected faces fully inside in
  at least 99% of a corpus of 500 photos (our target), and the subject box
  agrees with a human-chosen crop at IoU of at least 0.7 on average; lossless
  crops are pixel-identical in the kept area.
- **Engine:** Faces: OS first, own model second. macOS and iOS: Apple Vision
  `VNDetectFaceRectanglesRequest`. Android: ML Kit Face Detection. Windows:
  `Windows.Media.FaceAnalysis` (`FaceDetector`, detection only). Linux:
  fallback. Fallback model (any platform where the OS engine is absent or
  fails the quality bar test in CI): the face detector of 3.16 (YuNet, MIT).
  Saliency: OS engine: none. Own model on every platform: U2-Net-small class
  saliency (Apache-2.0, verify weights). Own code: `mozjpeg`/`jpegtran`-class
  transform.
- **Surfaces:** panel, bar ("crop 4:5"), CLI `panoply image crop a.jpg --ratio
  4:5 --smart`, MCP `image_crop`.

## 3.7 Rotate and flip

- **Module:** Rotate and flip. **Pattern:** A.
- **Budgets:** standard. Lossless transforms stream one MCU row at a time.
- **Job:** "Fix a sideways photo, or rotate or mirror it."
- **Inputs:** any raster images and folders.
- **On drop:** reads the EXIF orientation (all eight values) and shows what the
  photo will look like; finds files whose pixel data and tag disagree with
  what viewers show (the "sideways on one app only" bug).
- **Options:** Action (auto-orient default when a tag is present; rotate 90, 180,
  270; flip horizontal or vertical). Lossless (on).
- **Preview:** the result thumbnail.
- **Output:** same name in place of a copy "name (rotated).jpg"; orientation
  tag reset to 1 after baking.
- **Result card:** "Rotated 90 degrees, lossless, orientation tag fixed".
- **Edge cases:** lossless JPEG transforms need MCU-aligned edges; images whose
  size is not a multiple of 8 or 16 either trim the odd edge row (default,
  reported in pixels) or re-encode losslessly-as-possible on request (jpegtran
  semantics); HEIC rotated through `irot` (container, no re-encode); mirrored
  orientation values 2, 4, 5, 7 handled; Live Photo (IPC-4, keep paired video
  orientation in sync); RAW rotates the sidecar, not the file; animated (IPC-6).
- **Quality bar:** lossless JPEG rotation is bit-exact on the entropy-coded
  coefficients (verified by decoding coefficients, not pixels), and tag-only
  auto-orient does not change a single pixel byte.
- **Engine:** `mozjpeg-sys` transform API (`jpegtran`), container edits for HEIC.
- **Surfaces:** right-click, bar, CLI `panoply image rotate a.jpg --auto`, MCP
  `image_rotate_flip`.

## 3.8 Remove background

- **Module:** Remove background. **Pattern:** B.
- **Budgets:** exception on peak memory and first preview, on the fallback path only. OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. BiRefNet light has about 170 MB of weights and the full variant about 900 MB (full only with a GPU or Neural Engine). Peak for the fallback is capped at 2 GB on CPU for the light variant, because activations at 1024 px dominate. The original shows at once with a progress line and the result arrives in under 3 s for 12 MP. MODNet (about 26 MB) is the fast portrait path.
- **Job:** "Cut the subject out cleanly with hair-level edges."
- **Inputs:** JPG, PNG, WebP, HEIC and more; single or many.
- **On drop:** classifies the subject (person, product, animal, graphic) and
  picks the model path; reads the HEIC portrait matte if present (IPC-3) as a
  prior; checks resolution (the model runs at 1024 px, the alpha is refined at
  full resolution).
- **Options:** Result (transparent PNG, or on a color from 3.10; default
  transparent PNG/WebP). Edge refine (auto: guided-filter matting on hair and
  fur, default on). Edge color decontamination (on; removes the old background
  color from edge pixels). Keep shadow (off).
- **Preview:** subject on checker and on white with a "refine" brush
  (restore or erase) for the rare miss.
- **Output:** "name (cutout).png", WebP with alpha for web.
- **Result card:** "cutout done in 1.4 s, edge accuracy checked, 612 KB PNG".
- **Edge cases:** hair against a similar-colored background; glass and
  transparent objects (alpha, not binary); a subject cut off by the frame;
  two or more subjects (keep all by default, tap to drop one); images with
  a white subject on white; 16-bit PNG (IPC-5, mask at 8-bit, color stays
  16-bit); CMYK or P3 (convert once, IPC-2); animated WebP (frame by frame
  with temporal smoothing, IPC-6); huge input (model at 1024, upsample alpha
  with the full-resolution image as guide, IPC-7); halos from JPEG
  compression fringes.
- **Quality bar:** on DIS5K-style and human-matting test images (and our
  corpus of 200 real product and portrait photos), mean absolute error
  and gradient error on the alpha at least equal to BiRefNet's published
  numbers, hair-fringe visually no worse than remove.bg and Photoroom on
  our blind side-by-side of 100 pairs (we publish the pairs); runtime
  under 3 s on a recent laptop without a GPU for 12 MP. This also beats the
  upload round trip they require.
- **Engine:** OS first, own model second. macOS and iOS: Apple Vision
  `VNGenerateForegroundInstanceMaskRequest` (iOS 17, macOS 14). Android: ML Kit
  Subject Segmentation (beta). Windows: Windows AI Image Object Extractor
  (`ImageObjectExtractor`, Copilot+ PCs only, hint-based), else none. Linux:
  fallback. Fallback model (any platform where the OS engine is absent or
  fails the quality bar test in CI): BiRefNet (MIT, light variant for CPU and
  phones, full variant on GPU and Apple Neural Engine via ONNX Runtime or Core
  ML). On every platform our own guided-filter alpha refine and
  decontamination run after the cutout. Weights license: MIT on
  the project's own cards, but training sets such as DIS5K carry academic
  terms; verify before the first release. Not used: RMBG-2.0 (CC BY-NC 4.0,
  commercial use needs a BRIA agreement). Fallback for speed: MODNet
  (Apache-2.0) on portraits.
- **Surfaces:** bar ("remove background"), panel, right-click, phone share,
  CLI `panoply image cutout a.jpg`, MCP `image_remove_background`.

## 3.9 Keep only what I say

- **Module:** Keep only what I say. **Pattern:** B.
- **Budgets:** exception on peak memory. Three models run in sequence (Florence-2, SAM 2 small, then the 3.8 refine). Florence-2 and SAM 2 have no OS engine, so they download on first use and unload after 60 s idle on every platform. The 3.8 refine step uses OS engines where the 3.8 quality bar test picks them (no download, no resident memory for us), else the 3.8 fallback model. Each model is released as soon as its stage ends, so peak is the largest single stage, not the sum. Load per job and 60 s unload as in 3.8.
- **Job:** "Keep the dog and remove everything else."
- **Inputs:** one or many images; batch uses the same phrase on each.
- **On drop:** nothing special beyond 3.8; runs a fast pass to list the
  objects it can name ("person, dog, bench") so the user can tap a chip instead
  of typing.
- **Options:** Phrase (text, default empty; chips from detections). Mode (keep,
  or remove this and keep the rest). Edge refine (as 3.8). Tap-to-add and
  tap-to-remove points.
- **Preview:** the mask overlay and the cutout; wrong object means one tap to
  add or subtract.
- **Output:** as 3.8.
- **Result card:** "kept: dog (1 of 3 candidates)".
- **Edge cases:** phrase matches several objects (keep all, show chips);
  phrase matches none (say so, show what it found); ambiguous words ("the
  red one"); very small objects (crop zoom before segmenting); phrases in
  Hindi, Bengali, Spanish (translate the phrase through the on-device language
  model, English-only detectors otherwise).
- **Quality bar:** correct object chosen on the first try on at least
  85% of a labelled set of 300 prompts with COCO-style categories (our
  target), and mask IoU at least 0.9 after one correction tap. Competitors
  that do this: Photoroom and Canva's prompt tools, which upload.
- **Engine:** Phrase grounding and prompted masking: OS engine: none. Own
  model on every platform. Florence-2 (MIT) for phrase grounding to a box, then SAM 2 small
  (Apache-2.0 for code and weights per Meta's repo; a secondary source
  claims the Hiera backbone weights carry a non-commercial term, so verify
  against Meta's checkpoint page before shipping) for the mask, then the 3.8
  refine, which follows the per-platform ladder of 3.8 (OS first, BiRefNet as
  fallback). Each model is released as soon as its stage ends, so only one is in
  memory at a time. Grounding DINO (Apache-2.0) is the alternative detector.
- **Surfaces:** panel, bar ("keep the dog"), CLI
  `panoply image keep a.jpg --prompt "the dog"`, MCP `image_keep_only`.
  Recommend merging into 3.8 as a "describe what to keep" field; see summary.

## 3.10 Change background

- **Module:** Change background. **Pattern:** B.
- **Budgets:** as 3.8 for the cutout (OS engines first, no download and no resident memory for us; the 3.8 fallback model only where the quality bar test picks it). Depth has no OS engine: the optional Depth Anything V2 Small (about 100 MB) downloads on first use, loads only for depth-aware blur and unloads after 60 s idle, on every platform. Compositing is standard.
- **Job:** "Put this subject on white, blur the background, or use another
  picture."
- **Inputs:** one or many images (or the output of 3.8).
- **On drop:** runs 3.8 and, for person photos, estimates the original light
  color and direction roughly (average edge color) to relight edges.
- **Options:** Background (solid color, blur, gradient, image, studio white;
  default white for products and passports, blur for people). Blur strength
  (default depth-like, scaled to image size). Shadow (soft contact shadow, on
  for products). Edge blend (auto, decontaminate with the new color).
- **Preview:** result live, with the subject movable and scalable.
- **Output:** "name (new bg).jpg" or PNG.
- **Result card:** background chosen, size, "edge color matched".
- **Edge cases:** hair semi-transparency (alpha composite in linear light,
  IPC-2, otherwise dark fringes); depth blur from HEIC depth map is used when
  present (IPC-3, otherwise a distance-from-subject falloff); background image
  with a different DPI or color profile (convert first); transparent PNG
  input (IPC-8); animated input (IPC-6).
- **Quality bar:** no visible fringe or halo at 300% on 100 test composites;
  solid-color result passes the 2.1 passport background uniformity checks;
  must beat Photoroom and remove.bg on edge color when the new background is
  far from the old.
- **Engine:** Cutout: the per-platform ladder of 3.8 (OS first, BiRefNet as
  fallback). Depth: OS engine: none. Own model on every platform: optional
  Depth Anything V2 Small (Apache-2.0; the larger sizes are CC BY-NC, so only
  Small ships) for depth-aware blur. Own code: linear-light compositing,
  Gaussian and bokeh-style disc blur.
- **Surfaces:** panel, bar ("blur background"), CLI
  `panoply image background a.jpg --color white`, MCP `image_change_background`.

## 3.11 Upscale

- **Module:** Upscale. **Pattern:** B.
- **Budgets:** exception on time and model size, on the fallback path only. OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. Real-ESRGAN weights are a few MB (general v3) to about 65 MB (x4plus). Peak scales with tile size, not image size: input is decoded and output written in strips, tiles overlap and blend. The cursor-area preview tile renders in seconds, not 100 ms.
- **Job:** "Make this small or soft image bigger and sharper."
- **Inputs:** JPG, PNG, WebP, HEIC and more; single or many.
- **On drop:** classifies content (photo, anime or flat art, screenshot or text,
  face-heavy), measures noise and JPEG blockiness, and picks the model and a
  denoise strength from that; estimates the output size and warns when the
  result would exceed memory or format limits (IPC-7).
- **Options:** Scale (2x or 4x; default 2x, 4x when the longest side is under
  1000 px). Model (auto, photo, art; default auto). Denoise (auto). Faces
  (enhance, off by default, on for old photos, see 3.13).
- **Preview:** a 400 px tile at 200% before and after, rendered in seconds on
  the area under the cursor; whole image runs on Return.
- **Output:** "name (4x).png", or JPG or WebP for web; very large results as
  tiled TIFF.
- **Result card:** "800 x 600 to 3200 x 2400, 14 s, no faces changed".
- **Edge cases:** already high-resolution input (say "no benefit" above about
  2000 px unless 2x is explicit); JPEG blocking amplified by the model (a
  JPEG-artifact cleanup pass first); alpha (upscale color and alpha
  separately, IPC-8); 16-bit (IPC-5, model runs on 8-bit, color detail
  reapplied from the original low frequencies); animated (IPC-6, refused above
  a frame count and size, with a message); tiling seams (overlap and blend);
  text that models hallucinate (screenshot mode uses a conservative model);
  P3 (IPC-2).
- **Quality bar:** on a 100-image corpus (downscaled from real 4K sources, then
  upscaled and compared to the original), LPIPS and SSIM at least equal to
  Upscayl with the same Real-ESRGAN class models, tile seams invisible, and
  runtime no worse than Upscayl on the same GPU. We cannot beat it on model
  quality, since it uses the same model family; the win is the content-aware
  model choice and no hallucinated text.
- **Engine:** OS first, own model second. Windows: Windows AI Image Super
  Resolution (Copilot+ PCs only). macOS and iOS: none for still images
  (`VTFrameProcessor` in VideoToolbox is video only). Android: none. Linux:
  none. Fallback model (any platform where the OS engine is absent or fails
  the quality bar test in CI, so macOS, iOS, Android and Linux always):
  Real-ESRGAN family (BSD-3-Clause; `realesr-general-x4v3` small
  model for CPU and phones, `RealESRGAN_x4plus` and the anime 6B model for GPU),
  run through ONNX Runtime or NCNN with tiling. SwinIR (Apache-2.0) is a
  possible second model. Upscayl itself is not used (its code is AGPL); only
  model files whose license we have read ship.
- **Surfaces:** bar ("upscale"), panel, right-click, CLI
  `panoply image upscale a.jpg --scale 4`, MCP `image_upscale`.

## 3.12 Enhance

- **Module:** Enhance. **Pattern:** B.
- **Budgets:** standard for the classical path. Denoise and deblur have no OS engine for still images (`VTFrameProcessor` temporal noise filter is video only), so NAFNet runs on tiles on every platform (variant size in MODEL.json, shown before the first download), downloads on first use and unloads after 60 s idle. Peak scales with tile size.
- **Job:** "Fix a dark, noisy, soft or off-color photo in one go."
- **Inputs:** JPG, PNG, HEIC, RAW, TIFF; single or many.
- **On drop:** analyses histogram (clipping, black and white points), white
  balance (cast estimate), noise level (ISO-like, from flat-region variance),
  sharpness (blur kernel estimate), and which of these exceed a threshold;
  only those corrections are enabled.
- **Options:** Auto fix (on). Exposure (EV, default from analysis). White
  balance (temperature and tint, default from analysis, with a "pick a gray"
  eyedropper). Denoise (default by noise level). Sharpen (default by blur).
  Strength (0 to 100, default 60, one slider if the user wants a single control).
- **Preview:** before and after slider with the changed regions highlighted on
  request.
- **Output:** "name (enhanced).jpg" in the original format; RAW outputs to JPG
  or DNG-linear TIFF.
- **Result card:** the list of what was changed: "exposure +0.7 EV, white
  balance cooled 300 K, noise reduced, sharpened".
- **Edge cases:** already good photos (the module says "nothing needed" and
  does nothing instead of overcooking); skin tones and sunset (white balance
  guards: gray-world is not applied to scenes dominated by one hue); HDR gain-map
  photos (adjust the SDR base and the gain map together, never flatten, IPC-3);
  P3 and 16-bit work in float (IPC-2, IPC-5); faces shift in contrast limits;
  RAW needs camera color matrix, else generic.
- **Quality bar:** on a labelled set of 200 photos from phones with real
  defects, a blind A/B test against Google Photos auto-enhance and Apple's
  auto adjustments where we are preferred or tied at least half the time
  (our target); no clipped highlights added; denoise keeps edge SSIM at least
  0.95 relative to a clean reference on synthetic noise.
- **Engine:** classical Rust for exposure, white balance and local contrast;
  denoise and deblur: OS engine: none. Own model on every platform: NAFNet
  (MIT), run through ONNX Runtime on tiles.
- **Surfaces:** bar ("fix this photo"), panel, right-click, CLI
  `panoply image enhance a.jpg`, MCP `image_enhance`.

## 3.13 Restore old photo

- **Module:** Restore old photo. **Pattern:** B.
- **Budgets:** exception on peak memory. Up to four models (scratch net, LaMa about 200 MB, GFPGAN about 330 MB, Real-ESRGAN) run one after another and each is released when its step ends, so peak is the largest single model plus tiles. The scratch net, LaMa and GFPGAN have no OS engine here, so they download on first use and unload after 60 s idle on every platform. The optional upscale step uses Windows AI Image Super Resolution on Copilot+ PCs when the quality bar test picks it (no download, no resident memory for us), else Real-ESRGAN, which downloads on first use and unloads after 60 s idle. If the GFPGAN audit fails, the face step does not ship.
- **Job:** "Fix scratches, fading and blur in an old scanned photo."
- **Inputs:** scans and phone photos of prints (JPG, PNG, TIFF, HEIC).
- **On drop:** detects black and white versus sepia versus faded color, finds
  scratch and fold masks (thin high-contrast structures), counts faces and
  their size, and suggests whether to colorize (off by default).
- **Options:** Scratches and tears (on, auto mask, editable with a brush).
  Faces (enhance, on if faces are large enough, see edge cases). Fade (restore
  contrast and color, on). Colorize (off, offered only for black and white,
  marked as a guess). Upscale (2x optional).
- **Preview:** before and after with the scratch mask overlay and each step
  toggleable.
- **Output:** "name (restored).png" or JPG.
- **Result card:** steps applied and a note on any face reconstruction.
- **Edge cases:** face enhancement can change who a person looks like, so
  faces are blended at 60% by default with the original and the result card says
  "faces enhanced"; faces below about 64 px skip the model; photos of prints
  with glare or a visible edge (crop first, using 2.9's quad); damaged
  areas larger than the inpainter's reliable size (we mark them as "invented"
  in the preview); JPEG scans with heavy blocking; colorization is never
  default; 16-bit TIFF scans (IPC-5).
- **Quality bar:** on 100 real damaged scans, a blind test against Remini-class
  and Photoroom restore tools where faces keep identity (measured by face
  embedding similarity to the unrestored face above a threshold we publish) and
  scratch removal leaves no ghost lines at 200%.
- **Engine:** OS engine: none for the scratch mask, the fill and the face
  step. Own model on every platform: scratch mask by morphology and a small
  segmentation net (trained by us if no permissive one is found; Microsoft's
  "Bringing Old Photos Back to Life" code is MIT, weights terms to verify),
  LaMa (Apache-2.0) for filling, GFPGAN v1.4 for faces (Apache-2.0 code; its
  bundled third-party parts and its training-data terms must be audited,
  verify). Optional upscale: OS first, own model second. Windows: Windows AI
  Image Super Resolution (Copilot+ PCs only). macOS, iOS, Android and Linux:
  none for still images. Fallback model (any platform where the OS engine is
  absent or fails the quality bar test in CI): Real-ESRGAN (BSD-3). The models run one after another and each is released when
  its step ends, so only one is in memory at a time. CodeFormer is excluded (S-Lab license, non-commercial).
  If the GFPGAN audit fails, v1 ships without face enhancement and says so.
- **Surfaces:** panel, right-click, CLI `panoply image restore a.jpg`, MCP
  `image_restore_photo`.

## 3.14 Object eraser

- **Module:** Object eraser. **Pattern:** C (editor).
- **Budgets:** exception on peak memory and first preview. SAM 2 small (about 185 MB) has no OS engine, so it downloads on first use and loads on the first tap on every platform. Erase: OS engines (Windows AI Object Erase on Copilot+ PCs) need no download and no resident memory for us. The fallback LaMa (about 200 MB) downloads on first use and loads on the first stroke, never on drop, only on platforms where the quality bar test picks it. Inpainting runs on a crop around the mask, so peak scales with the crop, not the image (fallback path). Both models unload 60 s after the last edit. Preview under 2 s for 12 MP, not 100 ms.
- **Job:** "Paint over something and make it disappear."
- **Inputs:** one image at a time (batch only through 3.15 for watermarks).
- **On drop:** nothing heavy. The one-tap segmenter loads on the first tap and
  the inpainting model on the first stroke, never on drop.
- **Options:** Brush (size, auto-scaled to the image). Tap to select an object
  (SAM-based). Expand mask (default 8 px, so shadows and halos go too).
  Sensitivity for "also remove its shadow" (off by default).
- **Preview:** live result after each stroke (under 1 s for 12 MP on a laptop
  GPU, using a cropped context window around the mask).
- **Output:** "name (erased).jpg" in the original format; full-resolution edit
  (inpaint in a crop around the mask and blend back, never resample the whole
  image).
- **Result card:** area changed and "rest of the image untouched, byte for
  byte pixels outside the mask" (verified by hash).
- **Edge cases:** large holes in repeated textures (offer another
  candidate with a new seed on the model that supports it; LaMa is
  deterministic, so we offer a second pass); objects at the image edge;
  16-bit and P3 (IPC-2, IPC-5; inpaint a copy in 8-bit sRGB and blend the
  detail back); HEIC with gain map (inpaint SDR and gain map consistently,
  IPC-3); animated images unsupported (message); EXIF kept (IPC-9).
- **Quality bar:** on a 100-image corpus of masks (people on beaches, wires, signs)
  a blind comparison against Cleanup.pictures and Photoshop's Remove tool where
  we are rated acceptable on at least as many; pixels outside the mask
  unchanged; first preview in under 2 s.
- **Engine:** Erase: OS first, own model second. Windows: Windows AI Object
  Erase (Copilot+ PCs only). macOS, iOS, Android and Linux: none found.
  Fallback model (any platform where the OS engine is absent or fails the
  quality bar test in CI): LaMa (big-lama, Apache-2.0; training data is
  Places2, whose terms differ between cards, verify), MI-GAN (reported MIT,
  verify) for phones. Tap-to-select: OS engine: none. Own model on every
  platform: SAM 2 small (Apache-2.0, verify Hiera weights). Diffusion
  inpainting is not shipped in v1 because of license (OpenRAIL-M restrictions)
  and size.
- **Surfaces:** panel, phone, right-click "Erase object", CLI
  `panoply image erase a.jpg --mask mask.png`, MCP `image_object_eraser`.

## 3.15 Remove watermark

- **Module:** Remove watermark. **Pattern:** C (editor with auto-detect).
- **Budgets:** standard for the batch-median path (no model). The inpainting path adds a text detector and a fill. OS engines: no download, no resident memory for us. The fallback LaMa (about 200 MB) and PaddleOCR text detector download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. The median is computed band by band, so memory stays flat across a batch. The watched-folder surface is event driven.
- **Job:** "Remove the watermark, logo, date stamp or AI badge from my own
  pictures."
- **Scope and policy.** This is for content the user owns or has the right to
  edit: their own photos with a camera date stamp, their own exports with a
  logo they added, their own AI-generated images with a visible badge. The
  first run shows one sentence about this and asks the user to confirm; we
  cannot verify ownership, so the design avoids helping with scraping: no
  fetching from URLs, no bundled library of stock-site watermark templates, no
  "detect site" feature, no naming of third-party agencies in the UI. Invisible
  watermarks and signed provenance (for example SynthID or C2PA) are neither
  detected nor altered here; C2PA is handled in 3.19 and left intact unless
  the user removes it there. We do not claim it defeats invisible marks.
- **Inputs:** one or many images (batch is the point of the module).
- **On drop:** auto-detects candidates: (a) text overlays by a text detector;
  (b) repeated marks across the batch at the same position (median of aligned
  images, a classical and very accurate method when the batch shares a mark;
  computed band by band over at most 21 images chosen for spread, so memory
  stays flat);
  (c) corner stamps (date and time digits in orange or white near an edge,
  OCR plus color); (d) semi-transparent logos by contrast with the local
  background. Candidates appear as boxes the user accepts or edits.
- **Options:** Regions (auto boxes, plus a brush to add or erase). Apply to batch
  (same position, on when the batch has a shared mark; per-image otherwise).
  Fill (auto: inpainting; or blur or solid for flat areas). Expand (4 px).
- **Preview:** result for the current image, a strip of the other images'
  results; a flicker toggle.
- **Output:** "name (clean).jpg", same format.
- **Result card:** "12 images cleaned, mark at bottom right, pixels outside
  the mask unchanged".
- **Edge cases:** tiled diagonal watermarks across the whole image (the model
  works in tiles with a larger context, but we warn that large coverage cannot
  be restored faithfully); watermarks over faces or text (we show "invented
  detail" regions); alpha-blended marks where the median method recovers the
  underlying pixels (preferred over inpainting when it applies); images
  re-encoded since marking (mask edges are fuzzy, expand); HEIC, P3 (IPC-2,
  IPC-3); animated (IPC-6, per-frame with the shared mask only).
- **Quality bar:** on a self-made corpus (we add our own marks to 100 CC0
  photos so ground truth exists), PSNR and SSIM in the marked region within the
  best of LaMa-class results, and no visible trace at 200% for the batch-median
  path; batch of 50 same-mark images in under 60 s on a laptop. Competitors
  to beat on ease: web watermark removers that upload files.
- **Engine:** Text region detection: OS first, own model second. macOS and
  iOS: Apple Vision `VNRecognizeTextRequest`. Android: ML Kit Text Recognition
  v2. Windows: `Windows.Media.Ocr` (`OcrEngine`), and Windows AI Text
  Recognition on Copilot+ PCs. Linux: fallback. Fallback model (any platform
  where the OS engine is absent or fails the quality bar test in CI):
  PaddleOCR text detection (Apache-2.0). Own code on every platform: template
  and contrast detectors for region proposal, classical alpha-matte
  estimation across the batch for repeated marks. Fill: OS first, own model
  second. Windows: Windows AI Object Erase (Copilot+ PCs only). macOS, iOS,
  Android and Linux: none found. Fallback model: LaMa (Apache-2.0, see 3.14).
- **Surfaces:** panel, watched folder, right-click, CLI
  `panoply image unmark ./photos --auto --yes-i-own-these`, MCP
  `image_remove_watermark` (the tool description states the scope, and the
  server refuses URL inputs).

## 3.16 Blur faces and plates

- **Module:** Blur faces and plates. **Pattern:** B (with editable boxes).
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback face detector (YuNet, under 1 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. Plates have no OS engine, so our own plate detector (target under 50 MB, set when it is trained) loads on first use and unloads after 60 s idle on every platform. Tiled detection keeps peak flat on large images.
- **Job:** "Hide faces and license plates before I share this."
- **Inputs:** images, single or many; screenshots of video frames included.
- **On drop:** detects faces (all sizes including profile and masked), plates,
  and optionally screens and documents; lists counts ("6 faces, 2 plates").
- **Options:** What (faces, plates; default both). Style (strong blur plus
  noise, solid, pixelate large; default strong blur; light pixelation is not
  offered because it can be partly reversed by deblurring). Keep one face (tap
  to leave a person unblurred, default none). Padding (20%).
- **Preview:** boxes with toggles; a "what a recipient sees" view.
- **Output:** "name (blurred).jpg"; metadata strip is on by default and the
  embedded EXIF thumbnail is rebuilt, because it can contain the original
  unblurred image.
- **Result card:** "6 faces and 2 plates hidden, EXIF thumbnail replaced, GPS
  removed".
- **Edge cases:** tiny faces in crowds (run tiled at higher resolution, IPC-7);
  faces turned away or in sunglasses; plates in motion blur and at angles;
  plates of other countries (detector trained on diverse data, and manual boxes
  as backstop); HEIC with depth or portrait matte (the depth map holds the
  face shape, so it is dropped, IPC-3); Live Photo video still shows the face
  (IPC-4: the card warns the paired video is not blurred and offers to drop
  it); P3 (IPC-2).
- **Quality bar:** face recall at least 95% at 0.5 false positives per image
  on a corpus of 300 crowd and street photos (our target); plate recall tested
  on our own corpus; blurred regions fail a deblur attack test (a public
  deblurring model cannot recover an identifiable face by embedding match).
- **Engine:** faces: OS first, own model second. macOS and iOS: Apple Vision
  `VNDetectFaceRectanglesRequest`. Android: ML Kit Face Detection. Windows:
  `Windows.Media.FaceAnalysis` (`FaceDetector`, detection only). Linux:
  fallback. Fallback model (any platform where the OS engine is absent or
  fails the quality bar test in CI): YuNet (MIT, OpenCV Zoo) or MediaPipe
  BlazeFace (Apache-2.0); SCRFD and other InsightFace weights are
  non-commercial and are not used. Plates: OS engine: none. Own model on every
  platform. No permissively licensed model was confirmed in this pass,
  so we train our own detector (RT-DETR or YOLOX, both Apache-2.0; not
  Ultralytics YOLO, which is AGPL-3.0) on openly licensed plate data we have
  audited. This is the main risk of the module; faces ship first.
- **Surfaces:** panel, right-click, bar ("blur faces"), CLI
  `panoply image blur a.jpg --faces --plates`, MCP `image_blur_faces_plates`.

## 3.17 Metadata

- **Module:** Metadata. **Pattern:** I (inspect with fixes).
- **Budgets:** standard. Lossless segment rewrite streams at disk speed with no pixel decode.
- **Job:** "What is hidden in this photo, and remove or change it."
- **Inputs:** any image, single or many.
- **On drop:** the report appears immediately: camera and lens, date taken,
  GPS on a map (named), software, owner and copyright, thumbnails, depth maps,
  Live Photo content ID, XMP edit history, ICC profile, C2PA presence, maker
  notes size; a privacy summary line ("contains location and serial number").
- **Options:** Action (Strip all, Strip location only, Strip private (location,
  serial, owner, thumbnails), Edit fields). Default for sharing is "strip
  private"; default for archive is none. Keep orientation and ICC (always,
  because stripping them changes how the image looks).
- **Preview:** the field list with before and after, grouped by Exif, IPTC, XMP,
  maker notes; edits shown inline.
- **Output:** a copy "name (clean).jpg" (lossless: no pixel re-encode, only
  segment rewriting for JPEG, PNG chunks, WebP RIFF and HEIC items).
- **Result card:** "removed GPS, serial number, 2 thumbnails; image data
  untouched".
- **Edge cases:** orientation tag must survive stripping (or be baked first,
  IPC-1), a classic bug in naive strippers that turns portraits sideways;
  thumbnails inside EXIF and MPF (multi-picture) segments that still show
  the original; Live Photo and motion photo content IDs link the video (IPC-4);
  HEIC auxiliary images (IPC-3); sidecar XMP files for RAW; timestamps in
  file system times (optional reset); a plate or face in the thumbnail after
  a crop (we rebuild it).
- **Quality bar:** zero residual GPS tags or text after a strip (verified
  by scanning every segment and by running ExifTool over the output on 500
  files from different cameras); pixel data hash identical; and a field
  coverage matching ExifTool for common tags. We compare to ExifTool (the
  reference), and to iLoveIMG-class sites that re-encode.
- **Engine:** `kamadak-exif` or `nom-exif` for read, `little_exif` and
  `img-parts` for write, `quick-xml` for XMP, own HEIC item rewriter.
- **Surfaces:** bar ("what is in this photo"), right-click, share-sheet
  "strip location", CLI `panoply image meta a.jpg --strip private`, MCP
  `image_metadata`.

## 3.18 Add location

- **Module:** Add location. **Pattern:** B.
- **Budgets:** standard. The optional offline gazetteer is memory-mapped and downloaded on request (size shown first). Place-name search over the network is opt-in and labelled.
- **Job:** "Put GPS into photos that lack it."
- **Inputs:** images; optional GPX or location history exports from the user's
  own device (Google Takeout JSON, Apple Health route GPX, Strava GPX).
- **On drop:** reads the time taken (with offset) and groups photos by day;
  finds photos with GPS and without; proposes a track from a dropped GPX or a
  neighbor photo taken within 10 minutes.
- **Options:** Source (pick on a map, place name search, copy from nearby photo,
  match to a GPX track; default GPX if present, else nearby photos). Time offset
  (camera clock error, default 0, with a helper "this photo was actually at
  this place"). Overwrite existing (off).
- **Preview:** map with pins and the batch list.
- **Output:** GPS tags written into copies; name search needs the network and
  is opt-in and labelled; coordinates never leave the device otherwise
  (an offline gazetteer of place names ships as an optional download).
- **Result card:** "128 photos located from track, 4 had no match within 15 min".
- **Edge cases:** time zone ambiguity (no offset in old EXIF; assume the device
  zone, ask); HEIC and RAW sidecar writes (XMP sidecar for RAW, the
  container for HEIC); videos in Live Photos (IPC-4: write the same GPS to the
  paired MOV via the Video module); altitude and heading optional; privacy
  warning before writing a home address into photos to be shared.
- **Quality bar:** coordinates written round-trip exactly (decimal degrees to
  EXIF rationals within 1e-6 degrees) and are read correctly by Apple Photos
  and Google Photos on a test set; match rate equals a naive nearest-timestamp
  baseline or better.
- **Engine:** the Metadata writer, `gpx` crate, an offline tile or gazetteer
  (OpenStreetMap data, ODbL, attribution required).
- **Surfaces:** panel, CLI `panoply image geotag ./trip --gpx trip.gpx`, MCP
  `image_add_location`.

## 3.19 Content credentials

- **Module:** Content credentials. **Pattern:** I.
- **Budgets:** standard.
- **Job:** "Is there provenance data in this file, and keep or remove it."
- **Inputs:** JPG, PNG, WebP, AVIF, HEIC, TIFF, MP4 and more with a C2PA
  manifest.
- **On drop:** parses the C2PA manifest: issuer, signer, claim generator
  (camera, Photoshop, an AI tool), actions (created, edited, AI-generated),
  ingredients, and verifies the signature and certificate chain against the C2PA
  trust list, shown as valid, invalid, untrusted signer or tampered since signed.
- **Options:** Action (View, default; Remove; Keep in a sidecar file when another
  module edits the image). Re-signing is not offered. Show raw JSON (off).
- **Preview:** a human report, not JSON: who made it, with what, what was
  done, whether it is intact.
- **Output:** copy with the manifest removed ("name (no credentials).jpg"),
  lossless.
- **Result card:** "Manifest found: signed by X, valid; removed in the copy,
  pixels unchanged".
- **Edge cases:** other modules that re-encode break the manifest hash (any
  edit makes the signature invalid; the host warns "editing will invalidate
  content credentials" before the first edit, and Compress and Resize offer
  to keep the data in a sidecar); manifests inside HEIC and AVIF (supported
  by the spec); soft bindings and watermarks (read only: invisible watermarks
  are out of scope); self-signed manifests (shown as untrusted, not as fake);
  XMP-only "AI generated" tags that are not C2PA (separately listed as
  unsigned claims).
- **Quality bar:** verification results agree with the C2PA conformance
  reference tools on the official conformance test files and the open
  samples; zero false "valid" on tampered files.
- **Engine:** `c2pa-rs` (the C2PA open-source SDK, MIT or Apache-2.0).
- **Surfaces:** right-click "Inspect credentials", CLI
  `panoply image credentials a.jpg [--remove]`, MCP `image_content_credentials`.
  Recommend merging with 3.20; see summary.

## 3.20 Was this edited?

- **Module:** Was this edited? **Pattern:** I.
- **Budgets:** standard for the classical checks. The optional AI-image classifier has no OS engine, so it is our own model on every platform (size shown before the first download), downloaded on first use and unloaded after 60 s idle. None is chosen yet.
- **Job:** "Does this image show signs of editing or AI generation."
- **Inputs:** one image or a few; JPG, PNG, WebP, HEIC, TIFF.
- **On drop:** runs the evidence checks and shows each as a finding with a
  confidence word and what it means, never a single verdict: metadata
  (software tag, editing history in XMP, inconsistent timestamps), C2PA (3.19),
  JPEG evidence (double compression and quantization table history,
  inconsistent compression grids that suggest splicing), noise residual
  inconsistency across regions, copy-move detection, resampling traces, and
  a model score for AI-generated imagery when a permissive detector is
  available.
- **Options:** Depth (quick, thorough; default quick). Region of interest
  (draw). Show heat maps (off).
- **Preview:** the image with evidence overlays and a plain list.
- **Output:** a report (PDF or text) if asked; no image is modified.
- **Result card:** "3 signals: edited in Photoshop (metadata), double JPEG
  compression in the lower left, no AI-generation signature found. Not proof."
- **Edge cases:** social media recompression destroys most forensic traces
  (we say so when the file shows a recompression chain and lower the
  confidence); screenshots; HEIC to JPG conversions look like double
  compression; stripped metadata is not evidence of tampering; AI detectors
  have meaningful error rates and degrade after resizing; we show the
  limitation text on every report.
- **Quality bar:** we publish precision and recall by evidence type on a
  corpus including spliced, copy-moved, recompressed and AI images, and
  the tool must at minimum equal open ones such as FotoForensics-style ELA and
  Forensically on the classical checks, and abstain (say "inconclusive") instead
  of guessing. No accuracy number is claimed until measured.
- **Engine:** own Rust for JPEG quantization analysis, noise residuals and
  copy-move (classical; ELA alone is weak so it is shown as a visual aid only),
  `c2pa-rs`; AI-image classifier: OS engine: none. Own model on every
  platform. It is optional and ships only if a permissive,
  well-evaluated model is found (none was verified in this pass).
- **Surfaces:** panel, right-click, CLI `panoply image forensics a.jpg`, MCP
  `image_edit_check`.

## 3.21 Cull photos

- **Module:** Cull photos. **Pattern:** H (find) with review.
- **Budgets:** exception on model size. CLIP ViT-B/32 image tower (about 350 MB, quantized about 90 MB) and a small saliency model (about 5 MB) have no OS engine, so they download on first use and unload after 60 s idle on every platform. Faces: OS engines need no download and no resident memory for us. The fallback MediaPipe Face Landmarker (about 4 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. Peak under 600 MB (target, all models loaded). Embeddings are cached on disk, not held. The watched-folder surface is event driven.
- **Job:** "Go through my shoot, flag the blurry, blinking and duplicate shots,
  and pick the best of each burst."
- **Inputs:** a folder or a selection of photos (JPG, HEIC, RAW).
- **On drop:** groups into bursts and near-duplicates (capture time within a few
  seconds plus visual embedding similarity), then scores each photo: sharpness
  on the subject (not the whole frame, so shallow depth of field is not
  punished), eyes open and expression per face, exposure, subject in frame.
- **Options:** Strictness (low, normal, high; default normal). Keep per
  group (best one default; keep top 2). Action on rejects (flag only, default;
  move to a folder; send to Trash through the OS). Include RAW pairs (RAW and
  JPG pairs are treated as one).
- **Preview:** a grid by group with the pick marked and the reason ("sharpest,
  eyes open") and one-key override.
- **Output:** flags or labels in a sidecar (XMP rating and label) and optional
  folder moves; nothing is deleted without a confirm.
- **Result card:** "412 photos, 38 bursts, 61 blurry, 14 blink, 97 near
  duplicates flagged".
- **Edge cases:** HEIC burst and Live Photos (the key photo is used, IPC-4);
  intentional motion blur and low-key art (a "keep artistic" strictness);
  eyes closed in a laugh versus a blink (blink score uses lid opening ratio,
  not a binary); RAW sharpness measured on the embedded full-size preview for
  speed; many people (best group score, not a single face); EXIF time zero
  (fall back to file time).
- **Quality bar:** agreement with human picks on a labelled set of 200 bursts
  (we publish the set), measured as top-pick match rate, and flagged-blurry
  precision against human labels; faster than 20 photos per second on a laptop
  for JPEG. Compared with Apple Photos' key photo and Adobe Lightroom's
  auto-cull on the same set; no accuracy claimed before it is measured.
- **Engine:** Eye openness and smile: OS first, own model second. macOS and
  iOS: Apple Vision `VNDetectFaceRectanglesRequest` and
  `VNDetectFaceLandmarksRequest`. Android: ML Kit Face Detection. Windows:
  `Windows.Media.FaceAnalysis` (`FaceDetector`, detection only, no
  landmarks). Linux: fallback. Fallback model (any platform where the OS
  engine is absent or fails the quality bar test in CI, and always on
  Windows for landmarks): MediaPipe Face Landmarker (Apache-2.0). Duplicates:
  OS engine: none. Own model on every platform: a CLIP-class embedding
  (OpenAI CLIP ViT-B/32, MIT) plus perceptual hashes. Sharpness: own code on
  the saliency region, where saliency has OS engine: none and the own model
  is U2-Net-small class (Apache-2.0, verify weights).
- **Surfaces:** panel, watched folder, CLI `panoply image cull ./shoot`, MCP
  `image_cull_photos`.

## 3.22 Watermark

- **Module:** Watermark. **Pattern:** B.
- **Budgets:** standard. The watched-folder surface is event driven.
- **Job:** "Put my name or logo on these pictures."
- **Inputs:** images (many is typical); a text, PNG or SVG logo.
- **On drop:** finds low-detail corners and areas (so the mark avoids faces and
  busy regions), measures brightness there to pick light or dark ink.
- **Options:** Mark (text or logo; default last used, text from saved name).
  Position (corner, center, tiled; default bottom right with 3% margin).
  Size (percent of the short side, default 12%). Opacity (default 60%).
  Auto contrast (on, flips ink color per image).
- **Preview:** first image with the mark, draggable, applied to all.
- **Output:** "name (marked).jpg".
- **Result card:** count and position.
- **Edge cases:** mixed orientations and sizes (scale relative); logo SVG
  rendered at the final size, not scaled raster; transparent PNG logos blended
  in linear light (IPC-2, IPC-8); non-Latin text (shaped, see 2.5); animated
  (IPC-6); P3 source (mark color converted); the mark covering a face (shift).
- **Quality bar:** the mark is pixel-crisp at 100% and consistent in relative
  size across a mixed batch within 1 px; batch of 500 images runs without
  re-encoding more than once; must match iLoveIMG Watermark options and add
  auto contrast.
- **Engine:** `tiny-skia` or own compositing, `cosmic-text`, resvg.
- **Surfaces:** panel, watched folder, CLI
  `panoply image mark ./photos --text "(c) Name"`, MCP `image_watermark`.

## 3.23 Add text

- **Module:** Add text. **Pattern:** C.
- **Budgets:** standard.
- **Job:** "Put a caption, label or meme text on a picture."
- **Inputs:** one image (batch for the same caption position).
- **On drop:** offers meme layout (top and bottom, outlined) if the image is
  roughly square or landscape and the last use was a meme; otherwise a free text
  box. Finds the best low-detail region for the first text box.
- **Options:** Text, font (default Inter; meme default Impact-like open font),
  size auto-fit, color with auto contrast, outline, background plate, alignment.
- **Preview:** direct editing on the canvas.
- **Output:** "name (text).jpg".
- **Result card:** size and format.
- **Edge cases:** right-to-left and complex scripts (Arabic, Devanagari,
  Bengali, Thai) with real shaping and Noto fonts, plus bidirectional text;
  emoji (color fonts, COLR or SVG-in-OpenType); text exceeding the frame
  (auto-wrap); font licensing (only OFL fonts bundled); animated images
  (IPC-6); linear-light text blending for outlines (IPC-2).
- **Quality bar:** correct shaping on a test set of 200 complex-script strings
  compared with the platform text engine (CoreText and DirectWrite), and
  no clipped glyphs at any size.
- **Engine:** `cosmic-text` (HarfBuzz-class shaping via `rustybuzz`), `swash`,
  Noto fonts (OFL).
- **Surfaces:** panel, bar ("caption"), CLI
  `panoply image text a.jpg --top "TOP" --bottom "BOTTOM"`, MCP `image_add_text`.

## 3.24 Collage

- **Module:** Collage. **Pattern:** C.
- **Budgets:** standard. Each photo is decoded at its cell size (JPEG scaled decode), never at full size, so 50 photos stay under 300 MB.
- **Job:** "Arrange these photos in a grid or layout."
- **Inputs:** two or more images.
- **On drop:** orders by EXIF time, detects aspect ratios, and picks a layout
  that wastes least cropping; crops use the face-aware crop of 3.6.
- **Options:** Layout (auto, grids, justified rows, freeform; default auto).
  Aspect (1:1, 4:5, 16:9, A4 for print). Spacing and corner radius (defaults
  small). Background color (default white).
- **Preview:** live canvas with drag to swap.
- **Output:** JPG or PNG at the target size; print size in 300 DPI mode.
- **Result card:** photo count and pixel size.
- **Edge cases:** mixed orientations and aspect (justified layout avoids
  extreme crops); HEIC with Live Photos (still only, IPC-4); mixed color
  spaces (all converted to one working space, IPC-2); transparent PNG (IPC-8);
  one huge panorama (kept intact in a wide cell, IPC-7).
- **Quality bar:** layout never crops a detected face and the output is
  pixel-identical to a reference render of the same layout (golden test).
- **Engine:** own layout solver (justified rows and guillotine layouts),
  compositor.
- **Surfaces:** panel, right-click on a selection, CLI
  `panoply image collage *.jpg --layout auto`, MCP `image_collage`.

## 3.25 Screenshot beautifier

- **Module:** Screenshot beautifier. **Pattern:** B.
- **Budgets:** standard.
- **Job:** "Make this screenshot look good for a post or docs."
- **Inputs:** screenshots (PNG, JPG, WebP); clipboard.
- **On drop:** detects the window or UI rectangle and its rounded corners and
  title bar, display scale (2x retina), dark or light UI, and offers a matching
  gradient from the screenshot's dominant colors.
- **Options:** Background (gradient default from the image, solid, wallpaper
  image, transparent). Padding (default 8% of the width). Shadow (soft, on).
  Corner radius (auto from detected window). Device frame (none default;
  browser, laptop, phone; frames are our own drawings, no trademarked art).
  Size preset (X, LinkedIn, Open Graph 1200 x 630).
- **Preview:** live.
- **Output:** PNG (for crisp UI text) with an optional WebP.
- **Result card:** size and format.
- **Edge cases:** Retina screenshots with half-pixel sizes (keep 2x); display
  P3 screenshots (macOS screenshots are P3; convert for web, IPC-2); full-page
  scrolling screenshots very tall (IPC-7); transparent window shadows from
  macOS captures (remove the system shadow, then redraw); GIF or video
  screenshots (IPC-6 for animated, otherwise Video).
- **Quality bar:** text edges pixel-identical to the source after composition
  (no resampling at 1x padding) and shadow with no banding at 100% on 8-bit
  displays (dithered); the bar is parity with CleanShot X and Shots.so on
  options that matter, offline.
- **Engine:** own compositor with gradient dithering, `tiny-skia`.
- **Surfaces:** bar (screenshot from clipboard), CLI
  `panoply image beautify shot.png --bg auto`, MCP `image_screenshot_beautify`.

## 3.26 Color tools

- **Module:** Color tools. **Pattern:** F (instant answer) plus a picker.
- **Budgets:** standard.
- **Job:** "Give me the colors in this image, or convert and check a color."
- **Inputs:** an image, or a typed color (hex, rgb, hsl, oklch, name).
- **On drop:** extracts a palette (6 to 8 swatches by OKLab clustering), the
  dominant and accent colors, and the average color of the image.
- **Options:** Palette size (default 6). Output format (HEX default, RGB, HSL,
  OKLCH, CSS variables, Tailwind-style scale). Contrast pair (foreground and
  background to check).
- **Preview:** swatches with click to copy; WCAG 2 contrast ratio and APCA
  value for any pair, with pass or fail for AA and AAA.
- **Output:** clipboard text or a palette file (ASE, JSON, CSS).
- **Result card:** the colors and "copied".
- **Edge cases:** images in P3 or Adobe RGB (colors converted from the real
  profile, IPC-2, so the hex is what the user sees); CMYK sources; palette
  dominated by background (weights by saliency); on-screen picker across
  displays with different profiles (picks per-display, converts to sRGB).
- **Quality bar:** color conversions match CSS Color 4 reference values (tests
  on 1000 random colors within 1 unit of 8-bit); WCAG contrast is exact to 2
  decimals; palette extraction visually at least equal to Coolors and Adobe
  Color on a 50-image test.
- **Engine:** own OKLab code, lcms2, `palette` crate (MIT or Apache-2.0).
- **Surfaces:** bar ("#ff5500 to hsl", "contrast #222 on #fff"), panel, CLI
  `panoply image colors a.jpg --palette 6`, MCP `image_color_tools`.

## 3.27 Compare images

- **Module:** Compare images. **Pattern:** I.
- **Budgets:** standard.
- **Job:** "See what is different between these two images."
- **Inputs:** two images (or two folders to match by name).
- **On drop:** aligns sizes (if sizes differ, resamples the smaller or reports
  the mismatch), checks color profiles, and computes metrics.
- **Options:** View (side by side, slider, flicker, difference; default slider).
  Metric (SSIM, SSIMULACRA2, PSNR; default SSIMULACRA2 and SSIM). Threshold for
  the diff mask (default small noise ignored).
- **Preview:** the view plus a numeric panel and a heat map of differences.
- **Output:** a diff image or a report; nothing else is modified.
- **Result card:** "SSIM 0.982, SSIMULACRA2 81, 1.2% of pixels differ, largest
  region lower right".
- **Edge cases:** different color profiles (compare in a common space,
  IPC-2, and say so); different orientation tags (IPC-1); alpha (IPC-8);
  animated (compare frame by frame, IPC-6); 16-bit (IPC-5); huge images
  (tiled metrics, IPC-7); JPEG versus PNG of the same source (explains
  compression versus real change).
- **Quality bar:** SSIM and SSIMULACRA2 reproduce the reference
  implementations' outputs on their published test vectors to 4 decimals.
  This module also powers the quality bars of Compress and Convert in CI.
- **Engine:** `dssim`-class SSIM (AGPL-3.0 for the dssim crate, so we write
  our own SSIM), SSIMULACRA2 Rust port (BSD-2).
- **Surfaces:** panel, CLI `panoply image compare a.png b.png`, MCP
  `image_compare`.

## 3.28 Favicon and app icons

- **Module:** Favicon and app icons. **Pattern:** E (generator from an image).
- **Budgets:** standard.
- **Job:** "Make every icon size I need from this one picture."
- **Inputs:** one image (SVG or PNG at least 512 px preferred).
- **On drop:** checks size and squareness, transparency, and safe-area content
  (will it survive a maskable or rounded crop); flags images below 512 px.
- **Options:** Targets (web, iOS, Android, macOS, Windows, PWA; default web and
  PWA, plus the last used). Padding and background for maskable and iOS icons
  (default adds a safe zone; iOS icons must be opaque). Rounded or not (iOS and
  macOS apply their own masks, so we do not round them).
- **Preview:** each size at actual pixels and in a browser tab and a home
  screen mock.
- **Output:** a folder with `favicon.ico` (16, 32, 48), `favicon.svg`, PNGs,
  `apple-touch-icon`, Android adaptive foreground and background layers,
  `.icns`, `manifest.webmanifest`, and an HTML snippet copied to the clipboard.
- **Result card:** file count and the snippet.
- **Edge cases:** SVG with tiny details that vanish at 16 px (offer a simplified
  mono version); transparent versus opaque requirements per platform (IPC-8);
  non-square input (pad or crop, ask once); CMYK or P3 art (convert to
  sRGB, IPC-2); ICO with PNG-compressed entries.
- **Quality bar:** every output passes the platform's published icon rules
  (sizes and opacity) in an automated check, and small sizes are rendered from
  the SVG, not downscaled from a bitmap, so 16 px edges are sharp.
- **Engine:** resvg, `fast_image_resize`, `ico` crate, own `.icns` writer.
- **Surfaces:** bar ("favicon"), CLI `panoply image icons logo.svg --out ./icons`,
  MCP `image_icon_set`.

## 3.29 SVG tools

- **Module:** SVG tools. **Pattern:** A with three modes (optimize, rasterize,
  trace).
- **Budgets:** standard.
- **Job:** "Shrink this SVG, turn it into a PNG, or trace this PNG into an SVG."
- **Inputs:** SVG for optimize and rasterize; PNG, JPG for trace.
- **On drop:** SVG: finds editor cruft (Inkscape, Illustrator metadata),
  embedded raster images, fonts, scripts, unknown namespaces; raster: classifies
  as logo or photo (trace is only offered for flat art, with a warning
  for photos).
- **Options:** Optimize (default safe set: remove metadata and comments,
  precision 3 decimals, merge paths; "keep IDs" and "keep viewBox" are on).
  Rasterize (size, scale 1x to 4x, background transparent). Trace (colors count
  auto 2 to 16, smoothing, corner threshold).
- **Preview:** before and after render comparison, and size.
- **Output:** "name.min.svg", "name.png", "name (traced).svg".
- **Result card:** "12.4 KB to 3.1 KB, renders identical (0 pixel diff at 4x)".
- **Edge cases:** SVGs with scripts or external references (not fetched; scripts
  are stripped with a warning); embedded fonts and text (kept, or converted to
  outlines on request); CSS styles and animations (SMIL kept in safe mode);
  huge viewBox rasterize (IPC-7); trace with gradients (banding warning); color
  profile on embedded rasters (IPC-2).
- **Quality bar:** optimize passes a render-equality test at 4x on a corpus of
  300 real SVGs (zero pixel difference in safe mode) and is no larger than
  SVGO defaults; trace is compared with Vectorizer.AI and Potrace for fidelity
  (we claim no win on photos).
- **Engine:** `usvg` and `resvg` (MPL-2.0), own optimizer on the usvg tree
  (SVGO is Node), `vtracer` (MIT) for color tracing, Potrace is GPL so not used
  in the core.
- **Surfaces:** right-click, CLI `panoply image svg a.svg --optimize`, MCP
  `image_svg_tools`.

## 3.30 Straighten

- **Module:** Straighten. **Pattern:** B.
- **Budgets:** standard.
- **Job:** "Level a tilted horizon, or straighten a photo taken at an angle."
- **Inputs:** images, single or many.
- **On drop:** detects the dominant near-horizontal or near-vertical lines
  (horizon, buildings, doors), computes the tilt angle and a confidence, and
  checks whether the phone's gravity tag or EXIF already explains it.
- **Options:** Angle (auto, editable, default the detected angle if confidence
  is high, otherwise 0). Mode (level only, or also fix vertical perspective for
  architecture; default level only). Fill (crop to the largest inside
  rectangle, default).
- **Preview:** the straightened result with the grid and crop.
- **Output:** "name (straight).jpg".
- **Result card:** "rotated 2.3 degrees, cropped 4%".
- **Edge cases:** images with no clear lines (say "nothing to level", no change);
  slanted subjects that are meant to slant; EXIF orientation first (IPC-1);
  lossless rotation not possible (a re-encode, said plainly); HDR gain map is
  rotated with the image (IPC-3); skewed paper photos go to 2.9.
- **Quality bar:** angle error under 0.5 degrees on 200 labelled horizons
  and building photos (our target); no change on images without lines.
- **Engine:** LSD or Hough line detection (OpenCV-class or own), robust
  angle estimator, Lanczos rotation. No model needed in v1.
- **Surfaces:** panel, CLI `panoply image straighten a.jpg`, MCP
  `image_straighten`. Recommend merging into Rotate and flip (3.7) as an "Auto
  level" action, with paper skew owned by 2.9; see summary.

## 3.31 GIF maker

- **Module:** GIF maker. **Pattern:** B.
- **Budgets:** standard. Frames are streamed in two passes (palette, then encode) and never all held, so memory is constant for any clip length. The first preview is a low-fps, low-width draft.
- **Job:** "Make a GIF from these pictures or this video, small enough to send."
- **Inputs:** images (many), video clips, or screen recordings; also animated
  WebP or APNG to GIF.
- **On drop:** reads duration, frame rate, motion amount and dimensions; picks
  a frame rate and width to land near a size target (default under 10 MB,
  with other chat and email limits as presets in a data file with verified dates).
- **Options:** Width (default 480, up to 1080). Frame rate (default 12 for
  screen content, 15 for camera). Loop (forever). Palette (global or per-frame,
  auto by content). Dither (blue-noise, auto). Trim (for video; start and end).
  Output (GIF, or WebP and MP4 suggested when GIF is not required).
- **Preview:** looping preview and the estimated size, updated as the dials move.
- **Output:** "name.gif"; the result card suggests MP4 or WebP only when it has measured
  that they are smaller on this clip, and shows the measured size.
- **Result card:** "480 x 270, 36 frames, 3.1 MB, 12 fps".
- **Edge cases:** transparent frames and disposal modes; 256 colors banding on
  gradients (blue-noise dither plus per-frame palette); very long videos
  (cap, trim helper); mixed-size images (IPC-8 pad or crop, ask); HDR or
  P3 video (tone-map and convert to sRGB, IPC-2); Live Photo as input (IPC-4:
  the paired video becomes a GIF).
- **Quality bar:** at equal file size, higher SSIM than gifski output and
  ffmpeg palettegen on 50 clips (we measure; no claim before), within the
  platform size cap by default; encodes 5 s of 480 px video in under 3 s.
- **Engine:** our own quantizer and LZW writer (gifski is AGPL, `imagequant`
  GPL, so neither in the core), the `gif` crate for I/O, frame sources through
  the Video module, streamed in two passes (palette, then encode) so no clip is
  ever held whole.
- **Surfaces:** bar ("make a gif"), panel, CLI
  `panoply image gif clip.mp4 --width 480 --under 8mb`, MCP `image_gif_maker`.

## 3.32 Sprite sheet

- **Module:** Sprite sheet. **Pattern:** B with two modes (pack, unpack).
- **Budgets:** standard.
- **Job:** "Pack frames into one sheet with data, or cut a sheet into frames."
- **Inputs:** many images, or one sheet (with or without a data file); animated
  GIF, WebP or APNG for unpack.
- **On drop:** pack: trims transparent borders and measures; unpack: detects
  a grid by finding separating transparent or constant-color lines, or reads the
  JSON, XML or Aseprite data that sits next to it.
- **Options:** Pack: layout (max-rects, grid), max sheet size (default 2048,
  power of two on), padding and extrusion (default 1 px edge bleed to stop
  texture bleeding), trim and rotate (trim on, rotate off), data format
  (JSON array, Phaser, Unity, CSS, TexturePacker-compatible; default JSON).
  Unpack: grid size (auto), names.
- **Preview:** the packed sheet with cell outlines and an occupancy figure.
- **Output:** "sheet.png" plus "sheet.json"; unpack gives a folder or an
  animated GIF or WebP.
- **Result card:** "48 sprites in 1 sheet of 1024 x 1024, 83% occupancy".
- **Edge cases:** premultiplied alpha and color bleed in transparent edges
  (IPC-8, dilate color into transparent pixels); 16-bit (IPC-5); animated
  sources (IPC-6 per-frame names and durations carried into the data file);
  sprites bigger than the max size (spill to a second sheet); identical
  frames deduplicated (optional, off).
- **Quality bar:** pack occupancy within 3 points of TexturePacker's max-rects
  on a corpus of 20 real sprite sets; unpack then repack is pixel-identical.
- **Engine:** own max-rects packer (the `texture_packer` and `crunch` crates are
  permissive options), `png` crate.
- **Surfaces:** panel, CLI `panoply image sprites ./frames --out sheet`, MCP
  `image_sprite_sheet`.

## 3.33 Image to text

- **Module:** Image to text. **Pattern:** A (answer in the bar).
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback PaddleOCR (about 15 MB per language) and Tesseract data for rare languages download per language on first use with the size shown first, and unload after 60 s idle, only on platforms where the quality bar test picks them.
- **Job:** "Copy the text out of this picture."
- **Inputs:** images, screenshots, clipboard, scanned pages; many at once.
- **On drop:** detects the script and language (Latin, Devanagari, Bengali, Arabic,
  CJK and others), text orientation, columns and tables; if a page is
  skewed, straightens it first (2.9 stack).
- **Options:** Language (auto, multi-select). Layout (plain text in reading
  order, default; keep line breaks; table detection on). Output (clipboard
  default, TXT, searchable PDF).
- **Preview:** the image with text boxes and the text beside it; low-confidence
  words underlined.
- **Output:** text on the clipboard, "name.txt", or a PDF with an invisible
  text layer.
- **Result card:** "412 words, 98% mean confidence, 3 low-confidence words".
- **Edge cases:** EXIF orientation (IPC-1, sideways phone photos);
  handwriting (supported only loosely, flagged); vertical text; mixed languages
  (Hindi and English in one line); low-resolution screenshots (upscale text
  region first); 16-bit and P3 (IPC-2, IPC-5, convert to 8-bit sRGB for the
  model); huge scans (tiled, IPC-7); animated (first frame).
- **Quality bar:** character error rate on a public test set per script
  (we publish: English, Hindi, Bengali) at least equal to Tesseract defaults
  and Apple Live Text or Windows OCR on the same images; the module must be
  faster than a round trip to a cloud OCR such as Google Lens for a screenshot.
- **Engine:** OS first, own model second. macOS and iOS: Apple Vision
  `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` (iOS and macOS 26,
  paragraphs and tables). Android: ML Kit Text Recognition v2 (Latin, Chinese,
  Devanagari, Japanese, Korean). Windows: `Windows.Media.Ocr` (`OcrEngine`),
  and Windows AI Text Recognition on Copilot+ PCs. Linux: fallback. Fallback
  model (any platform where the OS engine is absent or fails the quality bar
  test in CI): PaddleOCR PP-OCR detection and recognition (Apache-2.0) via
  ONNX Runtime; Tesseract (Apache-2.0) as the fallback for rare languages the
  OS engine does not cover.
- **Surfaces:** bar (drop and Return copies), screenshot hotkey, CLI
  `panoply image ocr a.png`, MCP `image_to_text`.

## 3.34 Image to Markdown, LaTeX, code

- **Module:** Image to Markdown, LaTeX, code. **Pattern:** B.
- **Budgets:** exception on peak memory and first preview. A local document model (Granite-Docling-258M, about 500 MB, or PaddleOCR-VL) plus pix2tex have no OS engine, so they are our own models on every platform: they download on first use and unload after 60 s idle. Peak 1 to 2 GB, to be measured on CPU before the model is chosen. Code text uses the OS OCR engines of 3.33 (no download, no resident memory for us), with the fallback PaddleOCR (about 15 MB per language) only where the quality bar test picks it. Results arrive in seconds per page, not 100 ms.
- **Job:** "Turn this page, equation or screenshot of code into editable text."
- **Inputs:** images and PDF pages: notes, textbook pages, whiteboards,
  slides, screenshots of code.
- **On drop:** classifies the content type per region (heading, paragraph,
  table, math, code, figure), and sets the mode (document to Markdown, math
  to LaTeX, code to fenced block with language guess).
- **Options:** Mode (auto, Markdown, LaTeX, code). Tables (Markdown or HTML,
  default Markdown). Figures (keep as image links, default, or drop).
  Math delimiters (dollar signs or backslash).
- **Preview:** source on the left, rendered output on the right with edits, a
  per-region confidence, and a rerun for a single region.
- **Output:** clipboard, "name.md", or `.tex`.
- **Result card:** "2 tables, 5 equations, 1 code block, 3 regions low
  confidence".
- **Edge cases:** multi-column pages; equations inline with text; code with
  indentation (preserved exactly, tabs versus spaces guessed, with a check
  that the code parses when the language is known); handwriting math
  (lower confidence); rotated phone photos (IPC-1); long pages (tiled with
  overlap, IPC-7); the output must not invent text for blurred regions (the
  model is run with a confidence floor and marks them `[unclear]`).
- **Quality bar:** edit distance against ground truth on a corpus of 100
  pages, 100 formulas and 100 code screenshots, compared with Mathpix
  (cloud) and the open pix2tex and Nougat outputs; we do not claim parity
  with Mathpix until measured. Offline is the advantage.
- **Engine:** Document to Markdown and LaTeX: OS engine: none. Own model on
  every platform. It needs a local document model: Granite-Docling-258M
  (Apache-2.0, verify) or PaddleOCR-VL (verify license), and pix2tex for LaTeX
  (MIT). Code text: OS first, own model second. macOS and iOS: Apple Vision
  `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` (iOS and macOS 26).
  Android: ML Kit Text Recognition v2. Windows: `Windows.Media.Ocr`
  (`OcrEngine`), and Windows AI Text Recognition on Copilot+ PCs. Linux:
  fallback. Fallback model: PaddleOCR text (Apache-2.0). Nougat is not used
  (weights are CC BY-NC). Model size and speed must be measured on CPU before choosing;
  this module ships after 3.33.
- **Surfaces:** panel, bar ("image to markdown"), CLI
  `panoply image to-md page.png`, MCP `image_to_markdown`. Recommend merging
  with 3.33 as one OCR module with output modes; see summary.

## 3.35 What font is this?

- **Module:** What font is this? **Pattern:** B.
- **Budgets:** standard. OS engines: no download, no resident memory for us (the OCR crop step). The font-embedding model trained by us (target under 50 MB) has no OS engine, so it downloads on first use and unloads after 60 s idle on every platform. The memory-mapped index of OFL font embeddings is a few MB.
- **Job:** "Tell me the font in this image, or the closest free one."
- **Inputs:** an image or a cropped region.
- **On drop:** finds text regions, lets the user pick one, and checks that the
  crop has at least about 20 px of glyph height and a few distinct letters.
- **Options:** Search set (free fonts only, default; or my installed fonts).
  Candidates shown (5).
- **Preview:** the crop next to each candidate rendered with the same text.
- **Output:** a ranked list with licenses and download links (no automatic
  downloading); copy font name.
- **Result card:** "Closest: Inter Medium (OFL), then Roboto, Helvetica Neue
  (commercial)".
- **Edge cases:** curved or warped text; letters touching; very decorative
  scripts (we say "no close match"); logos are usually custom lettering; rotated
  text (IPC-1 and deskew); non-Latin scripts (limited coverage, said up front);
  images with heavy JPEG artifacts.
- **Quality bar:** top-5 accuracy on a test set rendered from the fonts in our
  index, then on 100 real screenshots; compared with WhatTheFont and Font
  Squirrel Matcherator, whose catalogs are larger. We do not expect to beat
  them on coverage.
- **Engine:** OCR crop: the per-platform OCR ladder of 3.33 (OS first,
  PaddleOCR as fallback). Font embedding: OS engine: none. Own model on every
  platform: an embedding model we train on synthetic renders of OFL fonts
  (Google Fonts, SIL OFL) and nearest-neighbor search. No permissive
  pretrained font recognizer was found in this pass.
- **Surfaces:** panel, CLI `panoply image font crop.png`, MCP `image_font_id`.
  Recommend deferring until 3.33 ships; see summary.

## 3.36 HDR images

- **Module:** HDR images. **Pattern:** B (view, convert, make).
- **Budgets:** standard for view and convert. Bracket merge works in strips after a low-resolution alignment, so five 48 MP brackets stay under 300 MB. Whole-frame float merging is not used.
- **Job:** "View and keep HDR photos, convert between HDR formats, or merge a
  bracket into one HDR image."
- **Inputs:** Ultra HDR JPEG and ISO 21496-1 gain-map files, Apple HEIC gain maps,
  10-bit AVIF and HEIC with PQ or HLG, JPEG XL, 16-bit TIFF and OpenEXR,
  or a bracketed set of photos.
- **On drop:** identifies the HDR representation (gain map, PQ, HLG, linear),
  peak brightness, and whether the display can show it; shows an SDR preview
  on SDR screens with the HDR headroom indicated.
- **Options:** Action (view, convert, merge; default view). Convert target
  (Ultra HDR JPEG, AVIF HDR, JXL, SDR tone-mapped JPEG; default Ultra HDR JPEG,
  because it falls back gracefully). Merge method (exposure fusion for SDR-safe
  results, or HDR radiance merge; default fusion). Tone map for SDR (default
  a hue-preserving curve).
- **Preview:** side-by-side SDR and HDR with a headroom slider.
- **Output:** "name (hdr).jpg", ".avif", or ".jxl".
- **Result card:** "gain map kept, peak 4.0x SDR white, SDR fallback included".
- **Edge cases:** compression tools that drop the gain map (the classic way
  people lose HDR, IPC-3); gain map metadata mismatch after crop or rotate
  (the map is transformed with the image); P3 primary with an sRGB fallback;
  HEIC from iPhone with a depth map next to the gain map; 16-bit EXR and
  TIFF (IPC-5); bracket alignment with handheld shots (translation and
  small rotation alignment, ghost removal).
- **Quality bar:** round trip Ultra HDR to AVIF HDR to Ultra HDR keeps linear
  luminance within 0.05 stops on the corpus; the output renders correctly in
  Chrome, Android and Apple Photos (manual test list); SDR fallback of the
  merge is no worse than Photoshop's HDR merge on a visual test of 20
  brackets, which we publish.
- **Engine:** `libultrahdr` (Apache-2.0), libavif and libjxl for HDR codecs,
  own gain-map math and Mertens fusion, `openexr` via `exr` crate. Bracket
  merge aligns at low resolution, then fuses in strips so memory stays flat.
- **Surfaces:** panel, CLI `panoply image hdr a.heic --to ultrahdr`, MCP
  `image_hdr`. Recommend moving gain-map preservation into Convert and
  Compress (it is a pipeline guarantee, IPC-3) and keeping this module for
  view and merge; see summary.

## 3.37 Film looks

- **Module:** Film looks. **Pattern:** B.
- **Budgets:** standard.
- **Job:** "Give this photo a film grain, date stamp or retro look."
- **Inputs:** images, single or many.
- **On drop:** reads exposure and contrast range to scale the look, and reads
  the real photo date for the date stamp.
- **Options:** Look (a list of presets: grain only, 35 mm color, black and
  white, faded, cross-process, date stamp; default 35 mm color; look names are
  generic, with no trademarked film names). Grain (amount and size
  by image resolution). Date stamp (on or off, orange digits, position, uses
  EXIF date, format). Strength (default 70%).
- **Preview:** live before and after.
- **Output:** "name (film).jpg".
- **Result card:** the look and strength.
- **Edge cases:** grain size must scale to the output resolution (otherwise a
  4000 px photo gets invisible grain and a 800 px photo gets mush); grain is
  applied in linear light and on luminance; no EXIF date (ask, or omit); HDR
  gain-map photos (the look applies to SDR base and map consistently, IPC-3);
  P3 (IPC-2); 16-bit with dither (IPC-5); date stamp drawn with an open
  7-segment font.
- **Quality bar:** grain statistics (power spectrum) independent of the image
  size within a tolerance; no banding in gradients at 8-bit (dithered); is
  faster than a blur-heavy filter chain, under 1 s for 12 MP.
- **Engine:** own LUT and grain code (3D LUT in float, `.cube` import for the
  user's own LUTs), no model.
- **Surfaces:** panel, bar ("date stamp"), CLI `panoply image film a.jpg
  --look 35mm`, MCP `image_film_look`.

## 3.38 E-reader optimize

- **Module:** E-reader optimize. **Pattern:** A.
- **Budgets:** standard. One page at a time through the worker pool, so a 2,000-page CBZ stays flat.
- **Job:** "Make comic and manga pages look good and load fast on my e-ink
  reader."
- **Inputs:** a folder of page images, CBZ, CBR (read), or PDF scans; outputs
  CBZ, EPUB or PDF.
- **On drop:** detects page size, spreads (two-page images), margins, color or
  grayscale, and the device from the last use or a chosen profile (Kindle,
  Kobo, reMarkable, Boox; data file with resolutions and verified dates).
- **Options:** Device (profile, default last). Grayscale (on for e-ink,
  16 levels for most panels). Margin crop (auto trim white borders, on).
  Split spreads (on). Contrast and gamma (auto for e-ink). Right-to-left reading
  (auto for manga by filename and metadata). Output format.
- **Preview:** a page as it would look on the device, with a simulated e-ink
  contrast.
- **Output:** "name (kobo).cbz" at the device resolution.
- **Result card:** "212 pages, 188 MB to 41 MB, 1264 x 1680, 16-level
  grayscale".
- **Edge cases:** CMYK scan JPEGs (IPC-2); 16-bit PNG scans (IPC-5, dither to
  the panel's levels); WebP pages in CBZ; page-order and numbering mistakes
  (sorts naturally, reports gaps); spreads that should not be split (a
  detection with manual override); huge scans (IPC-7); screentone and moire in
  manga when downscaling (an anti-moire resample, not plain Lanczos).
- **Quality bar:** moire score (power at screentone frequency) lower than plain
  Lanczos on a 20-chapter corpus, size lower than KCC defaults at equal visual
  rating; compared to Kindle Comic Converter (KCC), which is the real
  competitor here.
- **Engine:** `fast_image_resize` with a descreening filter, own 4-bit
  dithering, `zip` for CBZ, RAR reading through libarchive (BSD-2); the
  unRAR source license is restrictive and is not used; EPUB writer.
- **Surfaces:** right-click on a folder or CBZ, CLI
  `panoply image ereader ./manga --device kobo-clara`, MCP `image_ereader`.

## 3.39 Describe image

- **Module:** Describe image. **Pattern:** A (answer in the bar).
- **Budgets:** exception on peak memory, on the fallback path only. OS engines: no download, no resident memory for us. The fallback small vision-language model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it: Florence-2 captioning (about 460 MB in fp16) by default, Moondream 2 (1 to 4 GB by quantization) only on machines with the RAM. Size shown before the first download. The answer streams, first words under 1 s (target).
- **Job:** "Write alt text or a caption for this image."
- **Inputs:** images, single or many (batch writes a CSV or sidecar).
- **On drop:** classifies the content (photo, chart, screenshot, document, art)
  to pick the style: alt text is short for a photo, descriptive plus the data
  for a chart, and OCR-first for a screenshot with text.
- **Options:** Style (alt text, caption, detailed; default alt text). Length
  (about 125 characters default for alt text). Language (default device
  language). Include visible text (on). Context (a typed sentence about the
  page it sits on, optional).
- **Preview:** the text in an editable box; marks sensitive guesses (the model
  never states names of people from faces).
- **Output:** clipboard, EXIF/XMP description write (opt-in), or a CSV for a
  folder.
- **Result card:** the text and its length.
- **Edge cases:** the model must not identify people from faces (refuses and
  describes clothing and action); medical and ID images (description marked
  sensitive); charts (reads numbers from the OCR rather than guessing);
  images with embedded instructions (printed text is data, not commands to
  the model; text in the image is quoted, not obeyed); hallucination on
  blurry inputs (the text states uncertainty); EXIF orientation (IPC-1).
- **Quality bar:** human-rated on a corpus of 200 images for faithfulness and
  accessibility usefulness against alt text from Microsoft Office's automatic
  alt text and Apple's VoiceOver Recognition; CIDEr alone is not used. No claim
  until rated.
- **Engine:** OS first, own model second. macOS and iOS: none public found.
  Android: ML Kit GenAI Image Description (beta). Windows: Windows AI Image
  Description (Copilot+ PCs only, not in China). Linux: fallback. Fallback
  model (any platform where the OS engine is absent or fails the quality bar
  test in CI): a small vision-language model with a permissive license:
  Moondream 2 (Apache-2.0) or Florence-2 (MIT) captioning, evaluated for
  quality on CPU; larger models only on GPU. Qwen2-VL licensing varies by size,
  so verify before use.
- **Surfaces:** bar ("describe image"), right-click, CLI
  `panoply image describe a.jpg --style alt`, MCP `image_describe`.

## 3.40 QR and barcodes

- **Module:** QR and barcodes. **Pattern:** E (make) and A (read).
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback barcode decoder is our own code, not a model. Camera reading follows the camera frame rate. Make is under 50 ms.
- **Job:** "Make a QR code, style it, or read one from a picture or camera."
- **Inputs:** none to make; an image or the camera to read.
- **On drop:** an image reads at once: finds all QR, Data Matrix, and
  1D codes (EAN, UPC, Code 128, PDF417, Aztec), decodes, classifies the payload
  (URL, wifi, vCard, UPI payment, text) and shows a safety note for URLs
  (shows the real domain, flags punycode lookalikes) without visiting it.
- **Options:** Make: type (URL, text, wifi, contact, UPI, email, SMS;
  default URL from the clipboard), error correction (default M, H when a logo
  is added), size, colors (contrast-checked), logo, dot style. Output (SVG
  default, PNG, PDF).
- **Preview:** the code with a live scan test (the module decodes its own
  output through the reader at several scales and angles).
- **Output:** "qr.svg" and the like.
- **Result card:** "scans at 20 px per module, error correction H, 97% of the
  code area is data".
- **Edge cases:** inverted codes and low-contrast colors (refuse failing
  combinations with a reason); logos covering too much (limit by error
  correction); damaged or blurred images (try several binarizations and
  upscales); rotated and mirrored; many codes in one image (list all);
  16-bit and P3 inputs (IPC-5, IPC-2); camera on phones uses the OS scanner
  when faster.
- **Quality bar:** decode rate on a public QR benchmark set equal to or above
  ZXing and the OS decoders, and every generated code decodes on the corpus
  of 3 reader apps (manual list) at the printed size; make is under 50 ms.
- **Engine:** Reading: OS first, own decoder second. macOS and iOS: Vision
  `VNDetectBarcodesRequest`, and VisionKit `DataScannerViewController` (iOS)
  for the live camera. Android: ML Kit Barcode Scanning. Windows: none found.
  Linux: fallback. Fallback (any platform where the OS engine is absent or
  fails the quality bar test in CI): `rqrr` or `rxing` (ZXing port,
  Apache-2.0). Generating: own code on every platform, `qrcode` crate or own
  writer, SVG output via `tiny-skia`.
- **Surfaces:** bar ("qr for this url"), camera, CLI
  `panoply image qr make "https://..."` and `panoply image qr read a.png`,
  MCP `image_qr`.

---

# Cross-module notes

**Model list and licenses** (all re-checked at source before first release):

| Model | Used by | License as found | Note |
|---|---|---|---|
| BiRefNet (lite and full) | 3.8, 3.10, 3.9 refine, 2.1 | MIT | verify dataset terms (DIS5K academic) |
| MODNet | 3.8 fast portrait path | Apache-2.0 | lower hair quality, speed fallback |
| Florence-2 | 3.9 grounding, 3.39 | MIT | |
| SAM 2 small | 3.9, 3.14 | Apache-2.0 | one source says Hiera weights are non-commercial, verify |
| LaMa (big-lama) | 3.13, 3.14, 3.15 | Apache-2.0 | Places2 data terms differ between cards, verify |
| MI-GAN | 3.14 on phones | MIT (secondary source) | verify in the Picsart repo |
| Real-ESRGAN (general v3, x4plus, anime 6B) | 3.11, 3.13 | BSD-3-Clause | |
| NAFNet | 3.12 | MIT | |
| GFPGAN v1.4 | 3.13 faces | Apache-2.0 with third-party parts | audit, may cut |
| YuNet | 3.16, 3.6, 2.x faces | MIT | |
| MediaPipe Face Landmarker, BlazeFace | 3.21, 3.16 | Apache-2.0 | |
| PaddleOCR PP-OCR | 2.11, 3.15, 3.33 | Apache-2.0 | |
| Depth Anything V2 Small | 3.10 | Apache-2.0 | larger sizes are CC BY-NC, not used |
| OpenAI CLIP ViT-B/32 | 3.21 | MIT | |
| Granite-Docling, PaddleOCR-VL, pix2tex | 3.34 | Apache-2.0, verify, MIT | verify |
| Moondream 2 | 3.39 | Apache-2.0 | |
| Rejected | | RMBG-2.0 CC BY-NC 4.0; CodeFormer S-Lab non-commercial; SCRFD and InsightFace weights non-commercial; Nougat weights CC BY-NC; Ultralytics YOLO AGPL-3.0; gifski AGPL-3.0; imagequant GPL-3.0; dssim AGPL-3.0; Potrace GPL | |

Sources checked in this pass (model cards and repos, October 2026): BiRefNet on
Hugging Face (ZhengPeng7), LaMa and ONNX exports, Real-ESRGAN, SwinIR,
CodeFormer, GFPGAN, MODNet, SAM 2 licensing summaries, YuNet in OpenCV Zoo.
Where only a mirror or secondary page was found, the table says "verify".

**Open decisions this file forces:** the project license (GPL engines);
HEVC and HEIC encode; a plate detector; our own palette quantizer; the face
enhancement audit.
