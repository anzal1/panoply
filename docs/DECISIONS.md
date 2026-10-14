# Panoply decisions

One place for every merge, split, cut and defer recommendation in the module specs, the budget exceptions that came out of the budget pass, and the few questions only you can answer.

Sources: the review section of `specs/video-and-audio.md` (section 6), the cross-cutting recommendations of `specs/docs-find-files-share-capture.md`, the "Merge, split, cut" notes of `specs/text-calc-dev-privacy-misc.md`, and the inline "Recommend" notes plus "Open decisions" of `specs/id-and-image.md` (that file has no review section). `specs/pdf.md` has no review section and no recommendations.

Rule for silence: if you say nothing, the recommendation is taken. Merges keep every bar alias, CLI verb and MCP name. Cuts and defers mark the spec as deferred, they do not delete it.

## Decisions

| # | Decision | Modules | Recommendation | Why | Default if user says nothing |
|---|---|---|---|---|---|
| 1 | Merge | Transcribe 5.7, Subtitles 4.11 | One transcript service, two entry points | Same engine (whisper.cpp plus VAD) and the same transcript editor | Merge. Both bar names stay. |
| 2 | Merge | Edit by transcript 4.12, Cut by transcript 5.8 | One module, the input type decides | Same word-level cut flow for video and audio | Merge. |
| 3 | Merge | Dictation 5.10, Dictation cleanup 5.11 | One module with a cleanup level | Cleanup is a setting of dictation, not a separate job | Merge. Level stays Light by default. |
| 4 | Merge | Bitmap subtitles to SRT 4.13, Translate subtitles 4.15, Subtitle files 4.16 | One "Subtitle tools" module with three modes | Same files, same preview | Merge. |
| 5 | Merge | Audio 4.10, Streams 4.17 | One "Tracks" module | Mute, replace, extract and volume are all track operations | Merge. |
| 6 | Merge | Crop, rotate, flip 4.5 into Resize and reframe 4.4 | One canvas, two entry points | Same canvas and the same re-encode rules | Merge. The "rotate" bar alias stays. |
| 7 | Merge | Reverse and loop 4.9 into Speed 4.8 | A mode of Speed | Both retime frames | Merge. |
| 8 | Merge | Frame rate 4.27 into Convert 4.3 | An option in Convert | A rate change is a convert option | Merge. The "30 fps" bar alias stays. |
| 9 | Merge | Split 4.7 into Trim and cut 4.1 | A multi-output mode of Trim | Same timeline and the same cut engine | Merge. |
| 10 | Merge | Stereo to mono 5.15 into Convert 5.1 | A channels option | Same convert pass | Merge. The bar alias stays. |
| 11 | Merge | Metadata engine: Info 4.33, Metadata 3.17, Metadata for any file (docs), Clean before sharing 14.1 | One metadata inspector with a lens per file type. Strip is its sharing mode | Same read, risk-rank and strip job. Three files asked for it | Merge into one engine. Per-type lenses stay. |
| 12 | Merge | Redaction detection: Redact pasted text 11.3, Auto-redact 14.2, Mask ID numbers 2.11 | One detection engine (regex, checksums, names model) | One set of detectors. PDF Redact 22 and Annotate's blur-sensitive action can use it too | Merge the engine. The three surfaces stay. |
| 13 | Merge | Screenshot search, Photo search by meaning, Capture history | One "Search images and captures" module | One index, one grid, text and meaning modes | Merge. Capture history stays as the setting for Panoply captures. |
| 14 | Merge | Instant file search, Search inside files, Metadata search, Recent everything | One find module with operators and an empty-query state | They share one index and one parser. Users do not choose between them | Merge. |
| 15 | Merge | Similar images, Duplicate finder | One "Duplicates" module with exact and visual modes | Shared keep rule and cleanup | Merge. |
| 16 | Merge | Send to device, Send text and links, Send to many | One send flow | Same code path and UI: multi-select, text as a message | Merge. |
| 17 | Merge | Archives, Split and join | One module | Split archives already exist in Archives | Merge. |
| 18 | Merge | Rename to convert into Universal converter | A toggle in the converter | Same conversion graph, no separate module needed | Merge. |
| 19 | Merge | Pin to screen into Screenshot | An action on a capture | A pin always starts from a capture | Merge. |
| 20 | Merge | Record to GIF into Screen recording | An output choice | Same capture and the same encoder | Merge. |
| 21 | Merge | Search unplugged drives into the Index | A "catalog this drive" setting plus a results filter | Same Index, no second store | Merge. |
| 22 | Merge | Percent, tip, split bill 12.8 into Natural calculator 12.1 | Trigger aliases. The short spec stays as documentation | Same parser | Merge. |
| 23 | Merge | Passwords 13.6, Password generator (top-30 #23) | One module | Same job | Merge. |
| 24 | Merge | Photo print sheets 16.1, Print sheet 2.2 | One module | Same job and the same layout code | Merge. |
| 25 | Merge | Hashes 13.4, Verify downloads 14.5, Checksums (docs) | One engine, two surfaces | Same hashing code | Merge. |
| 26 | Merge | Word count (docs), Count 11.6 | One module | They are the same module | Merge. |
| 27 | Merge | Timestamps 13.9, Dates 12.5 (and Time zones 12.4) | One shared parser. Optionally one "Time" module | The shared parser is certain. The module merge is optional | Share the parser now. Keep the modules separate. |
| 28 | Merge | Data formats 13.1, Minify and beautify 13.11 | One "format" panel with language detection. Conversions stay in 13.1 | Same panel | Merge. |
| 29 | Merge | Format shootout 3.3 into Compress 3.2 | A "Try every format" toggle | Same search loop | Merge. |
| 30 | Merge | Optimize for web 3.4 | A saved chain (Resize then Compress), not an engine | It is a chain, not a module | Ship as a named chain. |
| 31 | Merge | Keep only what I say 3.9 into Remove background 3.8 | A "describe what to keep" field | Same cutout with one extra grounding stage | Merge. |
| 32 | Merge | Content credentials 3.19, Was this edited? 3.20 | One provenance module | C2PA is one of the evidence checks | Merge. |
| 33 | Merge | Straighten 3.30 into Rotate and flip 3.7 | An "Auto level" action. Paper skew stays with 2.9 | Same transform | Merge. |
| 34 | Merge | Image to Markdown, LaTeX, code 3.34, Image to text 3.33 | One OCR module with output modes | Same input. 3.34 ships after 3.33 | Merge. |
| 35 | Split | Remove watermark (video) 4.22 | Static delogo first, tracked inpainting later | Different engines and very different risk | Split. Delogo ships early. |
| 36 | Split | Restore old video 4.19 | Deinterlace and denoise first, AI upscale later | Cheap and fast versus heavy | Split. |
| 37 | Split | Video to Live Photo 4.31 | Live Photo to video (all platforms) and video to Live Photo (Apple only) | Different platform reach | Split. |
| 38 | Split | Trim, merge, fade 5.2 | Merge ships first as pattern A, the editor later | Merge is trivial and fast | Split. |
| 39 | Split | Office to and from PDF | Two modules: PDF to Office and Office to PDF | Different engines, quality bars and failure modes | Split. |
| 40 | Split | Ebook convert | "To EPUB" and "AZW3 and MOBI output" | Our own KF8 writer is the risky part | Split. |
| 41 | Split | Email and contact files | Mail (EML, MBOX, MSG, PST) and Calendar and contacts (ICS, VCF) | Different users and outputs | Split. |
| 42 | Split | Network 13.13 | A pure subnet calculator and a "this machine" inspector | Different trust and OS support | Split. |
| 43 | Split | India paperwork | Six rows in MODULES.md | It is already six specs | Split. |
| 44 | Split | HDR images 3.36 | Move gain-map preservation into Convert and Compress. Keep the module for view and merge | Gain-map handling is a pipeline guarantee (IPC-3) | Split. |
| 45 | Defer | Teleprompter 4.32 | Move to Capture and defer | Not a file transform | Defer. |
| 46 | Defer | Audio to MIDI 5.19, BPM and key 5.18 | Ship late. Keep the BPM engine | Low demand evidence. The BPM engine is reused by Loop maker and Speed and pitch | Defer. Keep the engine. |
| 47 | Cut | Remove reverb in Remove noise 5.5 | Leave out of v1 | The file lists it as left out of the first version | Cut from v1. |
| 48 | Cut | Podcast cleanup | Not a module. A saved chain (Remove noise, Remove silence, Normalize) | It is a chain | Ship as a chain. |
| 49 | Defer | Wave 3: Dictation, Meeting recorder, stems, reframe, interpolation | Ship after waves 1 and 2. Wave 1: Convert, Audio, Streams, Info, Trim, Repair, HDR to SDR, Stereo to mono, Normalize. Wave 2: Compress video, Compress audio, Transcribe and Subtitles, Remove noise, Text to speech | Highest engineering risk comes last | Follow this order. |
| 50 | Defer | Far away transfer | Defer to a community module | Relay cost, abuse handling, and it breaks "nothing leaves your device" by default | Defer. Not in the default build. |
| 51 | Cut | Shred | Keep only with honest SSD wording, or cut. Offer a "erase the key" guide instead | A tool that cannot guarantee deletion on SSDs gives false assurance | Keep with the honest best-effort wording. |
| 52 | Defer | Search inside videos | Ship after the images index | Hours of background work, heaviest on battery | Defer. |
| 53 | Defer | 3D files: STEP support | An optional OpenCASCADE helper, later. STL repair is v1 | The helper is large | Defer STEP. |
| 54 | Defer | Translate and Summarize on phones | Only through Borrow the desktop | The models are too big for phones | Desktop only. |
| 55 | Cut | Hide menu bar icons 17.10 | Cut | It needs polling, the opposite of the budgets | Cut. |
| 56 | Defer | Per-app volume 17.7 | Defer | macOS-only value at the highest cost | Defer. |
| 57 | Defer | Raycast compatibility 17.12 | Behind a switch, after a legal review | Legal risk and a 150 MB sidecar | Off, and the switch stays hidden until the review is done. |
| 58 | Cut | Keep awake with the lid closed (17.2) | Not offered | The text-calc file marks it as not offered | Not offered. |
| 59 | Defer | Translate 11.9 | Keep, but only the language pairs we can benchmark | No quality claim without a measurement | Ship benchmarked pairs only. |
| 60 | Defer | Media saving from websites ("YouTube to MP3") | Community module, desktop only, outside store builds | Platform terms and legal risk | Defer. Not specified, not promised. |
| 61 | Defer | What font is this? 3.35 | Defer until 3.33 ships | Needs the OCR crop and an embedding model we must train | Defer. |
| 62 | Cut | Face enhancement (GFPGAN) in Restore old photo 3.13 | If the licence audit fails, v1 ships without face enhancement | Third-party parts and training-data terms are unclear | Ship without it unless the audit passes. |
| 63 | Defer | Plate detection in Blur faces and plates 3.16 | Faces first, plates after our own detector is trained | No permissively licensed plate model was confirmed | Faces first. |
| 64 | Defer | Software HEVC and HEIC encode (3.1, 4.2, video HEVC output) | OS and hardware encoders only, no x265 | x265 is GPL and HEVC is patent encumbered. Needs legal review | OS and hardware encoders only. |
| 65 | Defer | Moonshine non-English, KittenTTS 2, Parakeet (video-and-audio models table) | Do not ship until each licence is read | Sources disagree on terms | Not shipped. English Moonshine and Kokoro only. |

Totals: 65 decisions. Merge 34, Split 10, Defer 15, Cut 6.

## Exceptions to the budgets

Every module whose budget is not plain "standard". The Budgets line in each spec has the detail. "Standard" means the table in FLOWS.md: 0% idle CPU, no memory held when idle, start under 50 ms, first preview under 100 ms, UI tasks under 4 ms, 120 fps, 300 MB peak, models unload after 60 s idle.

| Modules | Exception | Cap or cost | Reason |
|---|---|---|---|
| PDF 42 Convert in; Office to and from PDF (docs) | Peak memory, first preview | LibreOffice process, target under 500 MB, started per job, killed when the queue drains. Preview when the first page is ready | Office engine and cold start |
| PDF 44 Ask your PDF; Summarize (docs) | Peak memory | Language model of about 1 to 4 GB by device RAM (Summarize about 2.5 GB at 4 bits). Unloads 60 s after the last answer | On-device LLM |
| 5.9 Meeting recorder | Peak memory, active only during a meeting | Whisper while it runs, notes model about 2.5 GB after the meeting then unloaded. Call-safe CPU budget | Offline notes and transcript |
| 5.11 Dictation cleanup | Peak memory (Rewrite level only) | Qwen3-4B class, about 2.5 GB at 4 bits. Off by default | On-device rewrite |
| 5.10 Dictation | Always-on key hook | Under 10 MB resident, zero wake-ups. Model loads on key down | System-wide hotkey |
| PDF 43 Translate PDF; 4.15 Translate subtitles; Translate document (docs); 11.9 Translate text | Peak memory | Under 600 MB including runtime, per-pair model about 100 MB class | One translation engine |
| 3.8 Remove background | Peak memory, first preview | 2 GB cap on CPU (light variant). Result under 3 s for 12 MP | Matting model at 1024 px |
| 3.9 Keep only what I say | Peak memory | Largest single stage, models released one by one | Three models in sequence |
| 3.10 Change background | Peak memory | As 3.8. Depth model only for depth blur | Cutout model |
| 3.13 Restore old photo | Peak memory | Largest single model of four, released one by one | Several models in sequence |
| 3.11 Upscale | Time, model size | Peak scales with tile size, not image size. Preview tile in seconds | Real-ESRGAN family |
| 3.14 Object eraser | Peak memory, first preview | SAM 2 small about 185 MB plus LaMa about 200 MB, loaded on first use. Preview under 2 s | Segmenter plus inpainter |
| 3.21 Cull photos | Peak memory | Under 600 MB | CLIP image tower |
| 3.34 Image to Markdown, LaTeX, code | Peak memory, first preview | 1 to 2 GB, measured before the model is chosen | Document model |
| 3.39 Describe image | Peak memory | About 460 MB (Florence-2) to 4 GB (Moondream 2) | Vision-language model |
| 3.1 Convert (RAW only) | Peak memory | About 8 bytes per pixel, 45 MP is about 360 MB. Tiled demosaic is the goal | RAW development |
| 4.11 Subtitles; 4.12 Edit by transcript; 5.7 Transcribe; 5.8 Cut by transcript | Peak memory | Whisper base about 150 MB, small about 470 MB, medium about 1.5 GB, chosen from RAM | Speech model |
| 5.6 Vocal remover (stems) | Peak memory, time | About 2 GB, set by chunk size, constant for any song length | htdemucs |
| 4.14 Smooth motion | Compute | Constant memory (a few frames as float tensors). Time estimate shown | RIFE on the GPU |
| 4.19 Restore old video | Time | Minutes to hours per clip. ETA and section option shown | Per-frame upscaler |
| 4.22 Remove watermark (video, tracked path only) | Peak memory, time | LaMa about 200 MB in a temporal window | Tracked inpainting |
| 3D files (docs) | Peak memory | About 40 bytes per triangle, 10 M triangles is about 400 MB | Mesh size |
| Photo search by meaning (docs); Search inside videos (docs); Similar images (docs, embedding path) | Peak memory, background work while charging | Under 600 MB. Runs only while charging | Embedding and speech models |
| PDF 8 Clean scans; PDF 34 Rasterize and outline fonts | Peak scales with page size | Capped at 300 MB by working in bands and tiles | Large scans and posters |
| Instant file search (docs); Search inside files (docs); Screenshot search (docs); Capture history (docs) | Always-on index or bounded background work | One thread at background priority, off on battery and during calls, zero after catch-up. Name index memory-mapped | The Panoply Index |
| Auto-organize (docs); Rename to convert (docs); File shelf (docs); Receive (docs); Clipboard sync (docs); Borrow the desktop (docs) | Always-on, event driven | Zero idle CPU, no polling, under 10 to 15 MB resident | Watchers and listeners |
| 3.3 Format shootout | First preview | Rows stream in as each codec finishes | Slow AVIF encode |
| 2.1 Passport photo; 2.6 Thumb impression; 2.8 Signature cleaner; 2.9 Document scanner; 2.10 ID card scan; 3.40 QR and barcodes; Phone scanner to desktop (docs); Screen recording (docs) | Frame rate follows the camera or capture setting | 30 to 60 fps, not 120. Everything off the UI thread | Sensor and capture rates |
| Far away transfer (docs) | Needs a server | Relay for the fallback path. Not in the default build | Hole punching can fail |
| PDF 3 Smart split; PDF 12 OCR; PDF 19 Create form; PDF 22 Redact; PDF 25 Remove watermark (flattened path); PDF 36 Navigation; PDF 38 Extract text; PDF 39 Extract tables; PDF 41 Convert out; PDF 45 Read aloud; 2.7 Documents under a size; 2.11 Mask ID numbers; 3.6 Crop; 3.12 Enhance; 3.15 Remove watermark; 3.16 Blur faces and plates; 3.20 Was this edited?; 3.33 Image to text; 3.35 What font is this?; 4.4 Resize and reframe; 4.13 Bitmap subtitles to SRT; 4.23 Blur faces or regions; 4.25 Frames; 5.3 Compress (audio); 5.5 Remove noise; 5.12 Text to speech; 5.19 Audio to MIDI; Any file to Markdown (docs); Grammar and spelling (docs); Query data files (docs, optional model); Copy text from screen (docs) | Model-backed, inside the 300 MB cap | Small model, size shown before the first download, loaded per job, unloaded after 60 s idle | Models follow the Models budget |
| 11.1 Clipboard history; 11.3 Redact pasted text; 11.4 Snippets; 12.10 Timers; 14.2 Auto-redact; 17.1 Window snapping; 17.2 Keep awake; 17.5 Battery health; 17.6 System monitor; 17.7 Per-app volume; 17.8 Keyboard remapping; 17.12 Raycast compatibility | Declared in text-calc-dev-privacy-misc.md | macOS clipboard polling only while active, 150 MB names model, always-on hooks under 5 to 15 MB, sampler under 25 MB, Raycast sidecar target under 150 MB | Already budgeted there. Not edited in this pass |

Totals: 98 modules in 31 rows. Of these, 55 have a real exception (memory, preview, always-on, frame rate or server), 31 are model-backed inside the cap, and 12 were already budgeted in the text-calc file.

## Exceptions found by the beta smoke matrix (2026-10-14)

The smoke matrix (`crates/panoply-smoke`, results in `docs/research/beta-smoke.md`) runs every module over real corpus files and measures each job's peak memory. After the fixes below, the jobs that still pass the 300 MB budget only because of a row here are these. The caps are the measured worst case plus a margin, so a regression on the same files still shows. Nothing here changes what a module promises, it records what the system decoders and the models cost. A row that names files covers those files only; the same module on an ordinary picture is held to 300 MB.

| Modules | Files | Cap | Measured | Reason |
|---|---|---|---|---|
| Every image module that decodes through ImageIO | `img-heic-rotated-cw90`, a 33 MP 10-bit HEIF in 3520 x 1600 tiles | 1800 MB | 306 to 1612 MB | One tile is the smallest unit ImageIO decodes, about 800 MB inside the system whatever the module does. Compress and Convert read it whole, ID and Resize decode it smaller, and the fill model adds its own for Erase and Remove watermark. Same finding as `image-compress-results.md` section 3 |
| Every image module that decodes through ImageIO | `img-raw-canon-7d-sraw`, `img-raw-pixel-7-pro-dng`, camera RAW | 1000 MB | 355 to 938 MB | ImageIO has no tiled RAW path. 3.1 Convert already carries a RAW exception (about 8 bytes a pixel); measured it is higher, and the other modules share the decoder |
| Every image module that decodes through ImageIO | `img-avif-gainmap-avif`, an HDR AVIF at 400 x 300 | 450 MB | 336 to 380 MB | An HDR AVIF costs ImageIO about 170 MB before any module touches it |
| 3.14 Object eraser | any picture | 1250 MB | 575 to 1156 MB | The fill model is about 575 MB resident. The working picture adds about 7 bytes a pixel up to the 24 MP limit (1025 MB at 31 MP), and a batch keeps the allocator's high-water mark between pictures. The 600 MB row above holds for pictures under about 8 MP |
| 3.15 Remove watermark | any picture the fill runs on | 950 MB | 786 to 892 MB | The same fill model as 3.14. The row for 3.15 above says "inside the 300 MB cap", which no build of the model can meet |
| 2.1 Passport photo | any photo | 450 MB | 332 to 348 MB | The cutout is Apple Vision's foreground mask, and the 2.1 Budgets line says it carries the 3.8 exception. It had no number |
| PDF 12 OCR | any file | 650 MB | 535 to 543 MB | Apple Vision is 156 MB before a page is read. A page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models, about 200 MB). The row for PDF 12 above says "inside the 300 MB cap", which holds for pages that Vision reads |
| 3.11 Upscale | a batch | 400 MB | 313 MB | The model plus the allocator keeping its high-water mark between the pictures of a batch |

What was fixed instead of excepted: Image to text and OCR on pictures held a 31 MP photograph at 8192 pixels on the long side, 660 MB. Pictures and pages are now read at no more than 6 million pixels (A4 at about 250 dpi), 156 MB plus about 20 MB per million pixels. The first thing to revisit before a public release is the model-backed rows (Erase, Remove watermark, OCR): they are the ones that could come back under the cap with work, by releasing Vision before the fallback starts and by encoding results in strips.

## Decided by the user (2026-10-13)

- **License:** permissive only. No GPL or AGPL engine in the core. Panoply ships under a permissive license (MIT or Apache-2.0). We write our own GIF quantizer and SSIM.
- **Model licenses:** clean licenses only (MIT, Apache-2.0, BSD and similar). Models with unclear, academic or non-commercial terms are out until replaced or verified.
- **No bundled language model (revised 2026-10-13).** People already have AI (Claude, ChatGPT, Cursor, local Ollama). Panoply is the tool layer their AI calls through MCP and the CLI, not another chatbot. The few text-smart features (summarize, ask your PDF, meeting notes, dictation cleanup) use, in order: the model built into the OS when present (Apple Foundation Models on macOS and iOS 26, Gemini Nano on Android, Phi Silica on Copilot+ PCs), else a model the user already runs (Ollama, LM Studio) or their own API key, else they are hidden. Zero download, zero memory cost to Panoply. The LLM benchmark was stopped.
- **OS first, own model second (2026-10-13).** For any ML or media acceleration job, use the engine built into the OS when it is present and passes the module's quality bar, and fall back to our own small model only where it does not (ladder in FLOWS.md).
- **Small task models stay.** OCR, background removal, transcription, face and document detection, noise removal and upscaling are tiny single-purpose models (single to low hundreds of MB), downloaded only when that module is first used and unloaded after. They are the engine of the utilities, not a chatbot.
- **v1 scope:** waves 1 and 2. Fast stream-copy tools plus the demand-ranked top 30. Dictation and the meeting recorder follow in an update.
- **Defaults accepted unless he objects:** HEIC and HEVC export through OS encoders only; Raycast extensions behind a switch after reading Raycast's API terms; far-away transfer as a community module; Shred kept with honest SSD wording; shared engines for OCR, translation, GIF, watermark detection, print layout and Office conversion.

- **Plain code first, models only where nothing else works (2026-10-13).** About 200 of roughly 270 modules use no model at all, including the top of the demand list. Of the about 74 that touch a model: about 15 need one because the model is the feature (OCR, background removal, transcription, passport face detection, erase, upscale, noise removal, stems, photo search by meaning); about 20 run plain code first with a model only as fallback (document edges, smart crop, similar photos via perceptual hashes, blank pages, ID masking via patterns and checksums, digital PDF tables, language detection); the rest are nice-to-haves that use the OS model or stay hidden. Smallest clean models per job: research/tiny-models.md.
- **When a feature's bar is too high, reverse engineer how the best tool does it (2026-10-13).** Use REA (github.com/morluto/rea, MIT) to study the benchmark tool's behavior and technique, then build our own version clean-room: one pass studies and writes a behavior and technique note with evidence, a separate pass implements from that note only, never copying code. Candidates: PDF edit text and PDF to Word (Acrobat), whole-disk instant search (Everything), background removal edges (remove.bg), dictation latency (Wispr Flow), capture feel (CleanShot X), compression ratios (iLovePDF, TinyPNG).

- **Fallback model picks (2026-10-13, research/tiny-models.md).** Smallest clean model per job, used only where the OS has no engine or fails the bar: text embeddings granite-embedding-97m-multilingual-r2 (93 MB, names Hindi and Bengali), image embeddings TinyCLIP 40M (81 MB), OCR PP-OCRv5 mobile (about 19 MB with Latin and Devanagari) plus Tesseract Bengali, background IS-Net (42 MB), erase MI-GAN (15 MB), upscale realesr-general-x4v3 (3 MB), speech Moonshine tiny for English (26 MB), IndicConformer for Hindi and Bengali, Whisper base otherwise, voice KittenTTS nano for English (26 MB) and Kokoro for Hindi, VAD plus noise Silero plus GTCRN (2.8 MB), faces YuNet (0.1 MB), layout and tables PP-DocLayout-S plus SLANet (12 MB), names GLiNER-X small (165 MB, the largest, needed only where the OS lacks names), stems htdemucs q8 (52 MB), interpolation RIFE lite (21 MB). Every model downloads only when its module is first used. Worst case, a Linux user who uses every module in every language downloads about 1 GB in total; a typical Mac user about 170 MB. EmbeddingGemma 1 fails the license gate (Gemma terms). EmbeddingGemma 2 is Apache-2.0 but 149 MB and its card still cites the Gemma prohibited-use policy, so it is only an optional higher-quality tier after a human reads that policy.

- **MiniCPM "Smart pack" deferred (2026-10-13).** MiniCPM-V 4.6 (1.3B, Apache-2.0, about 1.5 GB with its vision projector) could unlock image understanding features on devices without an OS model. Not now; revisit later with a kill test against the tiny-model pipeline.

- **Kill test 2 result: MCP is a bonus, not a pillar (2026-10-13).** Fresh Sonnet agents with only standard command-line tools passed 10 of 10 everyday file tasks on the first try (bar: 3 or more failures would make MCP a pillar). They needed 57 tool calls and about 4 minutes in total, wrote their own PDF recompressor, DOCX converter and OCR script on the fly, and only passed because they checked their own output after plausible defaults gave wrong results (GPS kept, JPEG too small, wrong keyframe trim, size cap overshoot). That was on a developer Mac with ffmpeg, poppler and Chrome installed. Consequence: Panoply is pitched to people first; CLI and MCP stay as a free extra that makes agents faster and more reliable, not a headline. Details: prototypes/agent-killtest/RESULT.md.

- **GPUI for all desktops (2026-10-13).** The user could not tell the GPUI and SwiftUI command bars apart; GPUI matched native on memory (33 vs 30 MB) and startup. Headless checks pass for Hindi, Bengali, pinyin and dead-key input and for accessibility, with a 40-line grapheme-cluster patch to GPUI Kit's input that we carry and upstream. macOS, Windows and Linux share one GPUI app on the Rust core; SwiftUI only for iOS, Compose for Android. ADR 0001 amended.

- **Desktops launch together; free CI only (2026-10-14).** macOS, Windows and Linux ship together as one GPUI app (ADR 0009 amended), then Android, then iOS. CI must cost nothing. Comparing our PDF compression against iLovePDF with public test PDFs is approved. Solo for now, no collaborators to invite. The human Hindi and VoiceOver checks are deferred.

## Open questions only the user can answer (historical)

1. **Project license.** Do we allow any GPL or AGPL engine in the core? The files avoid imagequant, gifski, x264, x265, Potrace, Rubber Band, espeak-ng and dssim, and write our own quantizer and SSIM. A permissive-only rule keeps that work. Another choice could drop it.
2. **HEVC and HEIC encode.** Do we pay for or accept the legal review so HEIC and HEVC can be written, or stay on OS and hardware encoders only?
3. **Raycast compatibility.** Do we ship it at all, and only after a legal review of the Raycast API terms?
4. **Far away transfer.** Do we run a project relay (cost and abuse handling) or leave it to a community-run relay?
5. **Model licence appetite.** Moonshine non-English, KittenTTS 2, SAM 2 Hiera weights, GFPGAN and BiRefNet training data have unclear or academic terms. Do we ship with a risk, or only models with a clean licence?
6. **Size of the on-device language model.** Summarize, Ask your PDF and Meeting notes need about 1 to 4 GB. Is that acceptable as an opt-in download, or do we cap everything at a smaller model?
7. **Shred.** Keep it with the honest best-effort SSD wording, or cut it?
8. **v1 scope.** Ship waves 1 and 2 only, or also dictation and the meeting recorder at launch?

## Overlaps found during the budget pass

No review mentions these. They are not in the table above and have no default. Say the word and they become decisions.

- PDF 7 Page layouts (booklet, poster) and 16.2 Poster tiling, 16.3 Booklet do the same imposition. One engine, three entry points.
- GIF makers: Image 3.31, Video 4.26 and Record to GIF share one encoder. Record to GIF is already folded into Screen recording, so 3.31 and 4.26 are the remaining pair.
- OCR is used by PDF 12, Image to text 3.33, Subtitle OCR 4.13, Screenshot search and Copy text from screen. One OCR service and one language-model store.
- Translation is used by 11.9, Translate document, PDF 43 and Translate subtitles 4.15. One engine and one model store, so each pair downloads once.
- Watermark add and remove exist for PDF (24, 25), images (3.22, 3.15) and video (4.21, 4.22). Keep three surfaces, share the detection and inpainting code.
- PDF Convert in 42 and Convert out 41 overlap the docs module Office to and from PDF, and PDF Extract text 38 overlaps Any file to Markdown. Pick one owner for each route.
