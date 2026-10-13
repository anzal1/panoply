# Panoply module flows

Every module is one of nine flow patterns. The pattern decides the screen, the
keyboard model and what "one click" means. On top of the pattern, every module
gets the same shared capabilities for free, and its own spec says only what is
special about it.

## The nine flow patterns

| Pattern | Shape | One click means | Examples |
|---|---|---|---|
| **A. Instant transform** | Drop, smart defaults, run | Runs right away with the best defaults, options optional | HEIC to JPG, merge PDFs, extract audio, unzip |
| **B. Tuned transform** | Drop, live before and after preview, a few dials, run | Opens with the recommended setting already previewed, Return accepts | Compress PDF to a size, resize, remove background, upscale |
| **C. Visual editor** | Canvas or timeline you work on directly | Opens the editor with the file loaded and the likely action armed | Organize pages, sign, annotate, trim video, redact |
| **D. Guided capture** | Camera or photo, rules, live checks, fix-ups | Picks the rule set from context, checks run live | Passport photo, exam photo, document scan, signature cleaner |
| **E. Generator** | No file in, short form, file out | Last used template, filled from your saved details | QR code, invoice, resume, password, favicon set |
| **F. Instant answer** | Answer appears in the bar as you type | Return copies the answer | Calculators, unit and time zones, word count, hashes, base64 |
| **G. Always on** | Runs in the background, surfaces when needed | Toggle on once | Clipboard history, snippets, watched folders, receive files, file index |
| **H. Find** | Query, filters, results, actions on results | Results while you type, Return opens | File search, screenshot search, duplicates, big files |
| **I. Inspect** | Read-only report with one-click fixes | Report on drop, fix buttons inline | Metadata, what is this file, privacy scan, compare, redaction check |

## What every module gets (shared capabilities)

Modules never build these. The host provides them, so every module behaves the
same and quality cannot drift.

1. **Entry points.** Bar search, file drop on bar or window, right-click in
   Finder and Explorer, share sheet on phones, CLI command, MCP tool. A module
   declares its inputs once and appears in all of them.
2. **Smart defaults from the file.** On drop, the host analyses the file
   (type, size, pages, faces, text, GPS, codec, resolution) and hands the
   module a profile, so defaults fit this file, not an average file.
3. **Estimate before running.** Every transform shows the expected result
   before you commit: size, dimensions, duration, page count, time.
4. **Originals are never touched.** Output goes next to the original as
   "name (compressed).pdf", or to a chosen folder. Overwrite is an explicit
   option, and even then the original goes to the Trash, not oblivion.
5. **Batch.** One file or a thousand, same flow. Batches run in parallel within
   a CPU budget, with one progress line and per-file results.
6. **Chains.** Any result card has "Then…" to pipe the output into another
   module. A chain saves as a named action with a shortcut, a right-click item
   or a watched folder.
7. **Presets.** Any options set can be saved by name. Presets sync between
   your devices over the local network.
8. **Result card.** Open, Show in folder, Copy, Drag out, Send to device,
   Then…, Undo. Shows before and after numbers.
9. **Progress and cancel.** Anything over 300 ms shows progress. Cancel is
   instant and leaves nothing half-written.
10. **Plain errors.** "This PDF is password-protected. Enter the password or
    try Unlock PDF." Never a stack trace, always a next step.
11. **Privacy line.** Every screen states where the work ran. Modules with
    network access say so before they run.
12. **Keyboard first.** Every action reachable without a mouse. Return runs,
    Tab moves between options, Command+Z undoes the last result.
13. **History.** Every job is listed with its inputs, options and outputs, and
    can be rerun.
14. **Accessibility.** Full VoiceOver and screen reader labels, Reduce Motion,
    Dynamic Type on phones, high contrast.

## Budgets every module must meet

The philosophy (blazing fast, bang on utility, lowest memory, butter smooth)
as numbers. CI measures them for every module, built-in or community, and a
module that misses one does not ship.

| Budget | Limit |
|---|---|
| Idle cost | 0% CPU and no memory held when no job is running |
| Module start | under 50 ms from click to its screen being interactive |
| First preview | under 100 ms for instant and tuned transforms on a typical file |
| UI thread | no task over 4 ms on the UI thread; all work runs off it |
| Frame rate | 120 fps during previews, scrolling and progress, no dropped frames |
| Peak memory per job | under 300 MB for documents and images, constant for video and audio regardless of length (streamed) |
| Huge files | a 2 GB PDF or 20 GB video works within the same memory cap |
| Models | load on first use in a job, unload after 60 s idle, size shown before the first download |
| Speed | at least as fast as the native CLI tool for the same job (ffmpeg, qpdf, libvips), faster than uploading to a web tool |
| Output quality | equal or better than the named competitor on the module's quality bar |

## Engine ladder: OS first

Keep memory and compute as low as possible without losing any function. For any job that needs ML or media acceleration, use the engine built into the OS when it is present and when it passes the module's quality bar. Otherwise fall back to our own small model, downloaded on first use and unloaded after 60 s idle.

Rules:

- **Present** means the API exists on this OS version and this hardware, checked when the job starts. An engine that sends data off the device does not count as present.
- **OS engine:** no download, no resident memory for us, no model license for us to carry.
- **Fallback model:** size shown before the first download, loaded per job, unloaded after 60 s idle, clean license only (see DECISIONS.md).
- **The choice is per platform.** Each module's quality bar test runs in CI against the OS engine on each platform. The OS engine ships on a platform only if it passes. If it fails, that platform uses the fallback.
- **Linux** has no built-in ML engines, so ML jobs use the fallback there. Media acceleration still uses the OS.
- A module's Engine line lists the ladder per platform. Its Budgets line says the OS path costs nothing and gives the fallback's size.

| Job | macOS and iOS | Android | Windows | Fallback (our own) | License note |
|---|---|---|---|---|---|
| OCR | Apple Vision: `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` (iOS and macOS 26, groups text into paragraphs and tables) | ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean) | `Windows.Media.Ocr` (`OcrEngine`, needs package identity), and the Windows AI Text Recognition API on Copilot+ PCs | PaddleOCR-class model, ONNX | OS engines carry no license for us. PaddleOCR is Apache-2.0. |
| Subject and background | Apple Vision: `VNGenerateForegroundInstanceMaskRequest` (iOS 17, macOS 14) | ML Kit Subject Segmentation (beta, no SLA) | Windows AI Image Object Extractor (`ImageObjectExtractor`, Copilot+ PCs only, needs point or box hints). No general Windows API otherwise | BiRefNet (MIT weights, training data terms still under the model license rule) | OS engines carry no license for us. The fallback's license is the risk, so OS-first shrinks it. |
| Faces and landmarks | Apple Vision: `VNDetectFaceRectanglesRequest`, `VNDetectFaceLandmarksRequest` | ML Kit Face Detection | `Windows.Media.FaceAnalysis` (`FaceDetector`, `FaceTracker`, detection only, no landmarks) | YuNet (MIT), MediaPipe Face Landmarker (Apache-2.0) where landmarks are needed | OS engines carry no license for us. YuNet is MIT. |
| Speech to text | Apple `SpeechAnalyzer` with `SpeechTranscriber` (macOS, iOS 26) | `SpeechRecognizer.createOnDeviceSpeechRecognizer`, and ML Kit GenAI Speech Recognition (alpha) | Windows AI Speech Recognition (`SpeechRecognitionModel`, experimental, not for production yet), and `Windows.Media.SpeechRecognition` | whisper.cpp (MIT) with Whisper weights (MIT) | OS engines carry no license for us. whisper.cpp and the weights are MIT. |
| Translation | Apple Translation framework (`TranslationSession`) | ML Kit Translation (on-device, downloads per language) | None found. Windows Live Translation is planned, not available | Small permissive MT model, per language pair | OS engines carry no license for us. The MT model must be permissive. |
| Document scan and barcodes | VisionKit `VNDocumentCameraViewController` and `DataScannerViewController` (iOS), Vision `VNDetectDocumentSegmentationRequest` and `VNDetectBarcodesRequest` | ML Kit Document Scanner (`GmsDocumentScanning`) and ML Kit Barcode Scanning | None found for image barcodes or document scan | Own corner model plus classical quad detection, own barcode decoder (permissive) | OS engines carry no license for us. Our code is ours. |
| Text-smart features | Apple Foundation Models framework (`SystemLanguageModel`, macOS and iOS 26, Apple Intelligence devices) | Gemini Nano through ML Kit GenAI APIs (Summarization, Proofreading, Rewriting, Prompt; beta, listed devices) | Windows AI Phi Silica (Copilot+ PCs, Limited Access Feature, being replaced by Aion Instruct) | None. Else the user's own Ollama, LM Studio or API key. Else the feature is hidden | No model license for us anywhere. We ship no language model. |
| Media encode and decode | VideoToolbox | MediaCodec | Media Foundation | VA-API on Linux. Software codecs only if permissive and not patent encumbered (decision 64) | Hardware paths carry no codec license for us. |

Notes on the table:

- Other ML jobs follow the same ladder when an OS engine exists: Apple `NLTagger` and ML Kit Entity Extraction for names and entities, Windows AI Image Super Resolution and Object Erase (Copilot+ PCs), `VTFrameProcessor` in VideoToolbox for frame interpolation, super resolution and temporal noise filtering on Apple Silicon, and system voices for speech output. Where no OS engine exists, the module's own small model is the engine on every platform.
- Where the OS engine is below the bar on one platform only, that platform gets the fallback and the others keep the OS engine. The module behaves the same on all of them.

## When a quality bar looks out of reach

Some bars are set by closed tools that are very hard to match (Acrobat's edit
text, Everything's search speed, remove.bg's edges, Wispr Flow's latency).
Before lowering a bar, study how the benchmark tool achieves it with REA
(github.com/morluto/rea): observe behavior first, go down to the binary only
when needed, and record the technique with evidence. Implementation is
clean-room: the person or agent who writes the code works from the technique
note, never from decompiled code, so Panoply stays original and safely open
source.

## Spec template (one per module)

```
Module:        name
Budgets:       anything special against the table above (heavier model, larger peak), with the reason
Pattern:       A to I
Job:           the sentence a user would say
Inputs:        file types, single or many, or none
On drop:       what the host detects and what it changes in the defaults
Options:       name, type, default, how the default is chosen
Preview:       what is shown before running
Output:        format, naming, location
Result card:   the numbers shown after
Edge cases:    the files that break naive tools, and what we do
Quality bar:   what we measure and which tool we must beat
Engine:        library, or for ML jobs a per-platform ladder (OS engine first, own model as fallback), see the engine ladder above
Surfaces:      bar inline, panel, right-click, phone, CLI and MCP name
```

---

## Worked example 1: Compress PDF

- **Pattern:** B, tuned transform.
- **Job:** "Make this PDF small enough to email or upload."
- **Inputs:** one or many PDFs.
- **On drop:** measure size, page count, what the bytes are (images, fonts,
  scanned pages, embedded files), and whether it is already optimized. Check
  for passwords and signatures.
- **Options:**
  - Target: Smallest that still looks good (default), Under a size (20 MB
    email, 10 MB, 5 MB, 2 MB, 1 MB, custom), or Quality level.
  - Keep text selectable: on.
  - Grayscale: off.
  - Remove: metadata, embedded files, form data (off by default).
- **Preview:** estimated size, and a page zoomed to 200% before and after, so
  the user sees the quality cost.
- **Output:** "name (compressed).pdf" next to the original.
- **Result card:** "4.2 MB to 880 KB, 79% smaller, text still selectable."
- **Edge cases:**
  - Already-compressed PDFs: say "Already small, saved 2%" rather than
    inflating the file.
  - Signed PDFs: warn that compressing breaks the signature.
  - Scanned PDFs: offer to make the text searchable at the same time.
  - Encrypted files: ask for the password, keep the encryption on output.
  - Huge files over 1 GB: stream page by page, never load whole.
- **Quality bar:** at equal visual quality (SSIM at least 0.97 on images), the
  output is no bigger than iLovePDF's "recommended" setting on a corpus of
  100 real PDFs, and it runs faster than uploading the file to them.
- **Engine:** pdfium plus own image recompression (MozJPEG, JPEG 2000 off),
  font subsetting.
- **Surfaces:** bar, panel, right-click "Compress", phone share sheet, CLI
  `panoply pdf compress --under 2mb`, MCP `pdf_compress`.

## Worked example 2: Passport photo

- **Pattern:** D, guided capture.
- **Job:** "I need a photo that passes the passport or visa rules."
- **Inputs:** one photo, or the camera.
- **On drop:** find the face, eyes, chin and crown. Measure head tilt,
  lighting evenness, background, glasses glare, and whether the eyes are
  open.
- **Options:**
  - Country and document: defaults to the region of the device, plus recent
    choices. Each rule set is a small file with size, head ratio, background,
    DPI and the source link to the official rule.
  - Background: from the rule, override allowed.
  - Output: single digital file (with the portal's KB and pixel limits), or a
    print sheet (4 x 6 in or A4, count filled automatically).
- **Preview:** the cropped photo with head guides, live checks with pass,
  warn and fail states, and a fix button for each fixable failure (tilt,
  background, exposure).
- **Output:** JPG to the rule, plus the print sheet if chosen.
- **Result card:** "India passport, 35 x 45 mm, 630 x 810 px, 182 KB, all
  checks passed."
- **Edge cases:** two faces in the photo (ask which), very low resolution
  (refuse and say why), hair or beard touching the edges, children and babies
  (relaxed rules where the country allows), religious head coverings (allowed
  where the rule allows).
- **Quality bar:** zero rejections on the official validators that exist
  (for example US State Department's photo tool) across a test set, and the
  crop matches the rule to within 1 mm.
- **Engine:** on-device face landmarks and person segmentation models, own
  rule engine.
- **Surfaces:** panel, phone camera flow, right-click "Make passport photo",
  CLI `panoply id passport --country IN`, MCP `id_passport_photo`.

## Worked example 3: Universal converter

- **Pattern:** A, instant transform.
- **Job:** "Turn this into that."
- **Inputs:** any of the 212 readable formats, single or many, mixed types
  allowed.
- **On drop:** identify the real format from the bytes, not the extension.
  Rank target formats by what people convert this type to most, then by what
  this user picked before.
- **Options:** target format (top 4 shown as buttons, rest searchable), plus
  the target's own quality options folded away.
- **Preview:** the route ("DOCX to PDF to JPG, 3 pages") and estimated size.
- **Output:** one file per input, or one file for formats that combine (PDF,
  ZIP, GIF).
- **Result card:** what converted, what was lost if anything ("Fonts
  substituted: Calibri to Carlito").
- **Edge cases:** lossy-to-lossless warnings (MP3 to FLAC gains nothing), DRM
  protected ebooks and media (say so, do not try), unknown formats (open "What
  is this file?").
- **Quality bar:** every pair in CONVERSIONS.md has a round-trip test in CI on
  real sample files. Office conversions are compared visually page by page
  against Microsoft Office output.
- **Engine:** the conversion graph in `conversions/formats.py`, shortest route.
- **Surfaces:** bar ("heic to jpg"), rename-to-convert in Finder, right-click
  "Convert to", phone share sheet, CLI `panoply convert in.heic out.jpg`, MCP
  `convert`.
