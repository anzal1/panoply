# Module specs: documents, find, files, share, capture

Scope: MODULES.md sections 6 to 10 (59 modules). Written in the FLOWS.md
template. Shared capabilities (entry points, estimate, originals untouched,
batch, chains, presets, result card, progress, plain errors, privacy line,
keyboard, history, accessibility) are not repeated. Each spec says only what is
special to the module. "Target" means a budget we set and check in CI, not a
claim about another tool. Where a competitor is named, the bar is a head to
head test we run on our own corpus and publish.

Every spec has a `Budgets:` line after its Pattern line. It says "standard"
when the module fits the table in FLOWS.md, otherwise the exception and the
reason. Models follow the Models budget (load on first use in a job, unload
after 60 s idle, size shown before the first download). Model sizes are
approximate and re-read from each model's `MODEL.json`.

## Shared building blocks used below

Defined once here so the specs stay short.

**Panoply Index (sections 7 and 10).** One local store per user, in the app data
folder, never synced, never uploaded.
- Metadata and file table: SQLite via `rusqlite`, WAL mode. Keyed by volume id
  and file id (inode, NTFS file reference) so renames do not reindex.
- Name index: memory-mapped array of path components plus a trigram posting
  list, so any substring matches ("rep" finds "quarterly_report"). Memory-mapped
  at start (pages are reclaimable by the OS, no heap copy), so a query touches
  only the pages it needs.
- Full text: `tantivy` segments, one field per language analyzer, with
  positions for phrase search. Text is extracted by the same engines the
  modules use (pdfium text layer, `docx`/`calamine`/ODF readers, `epub`,
  `mail-parser`, plain text and code via `ignore` file typing).
- Vectors: `usearch` (Apache-2.0) HNSW, int8 quantized, one index per model.
- Embedding models, all downloaded once on first use from the release mirror:
  text and OCR text, `bge-small-en-v1.5` (MIT) for English and
  `multilingual-e5-small` (MIT) for other languages; images and text-to-image,
  SigLIP base (Apache-2.0) through `ort` (ONNX Runtime). License of each pinned
  model file is checked in CI at pin time.
- Freshness: macOS FSEvents (event id stored, so changes made while the app was
  closed or the Mac asleep replay on start). Windows NTFS USN journal for
  whole-volume speed (needs an elevated helper, offered once, optional);
  without it `ReadDirectoryChangesW` plus a parallel walk. Linux `fanotify` for
  mount-wide watches where permitted, else `inotify` with watch-limit handling,
  with an mtime sweep on start. All through the `notify` crate plus our own
  USN reader. Bursts (a git checkout touching 40,000 files) are debounced for
  250 ms and coalesced.
- Budgets: initial scan runs at background priority (macOS QoS utility,
  Windows background mode, Linux idle IO class). Content indexing uses at most
  one worker thread, pauses on battery below 20 percent, in Low Power Mode or
  Battery Saver, and while a fullscreen app or call is active. Name indexing
  stays on because it is cheap. Model-based indexing (embeddings, OCR) is off on
  battery and runs while charging. Memory: the name index is memory-mapped, the
  full-text writer heap is capped at 50 MB and released after each commit, and
  nothing is resident after catch-up beyond the file watcher.
- Privacy: content indexing never reads excluded folders. Defaults excluded
  from content (names stay searchable unless "hide completely" is set): system
  folders, app caches, `~/Library`, `node_modules`, `.git` objects, browser
  profiles, `.ssh`, `.gnupg`, keychains and password-manager vaults. Honored
  markers: `.noindex` folders on macOS, `.metadata_never_index`, the Windows
  "not content indexed" attribute, and a `.panoplyignore` file (gitignore
  syntax) in any folder. Password-protected files are never opened for content.
  One button, "Forget everything", deletes the index. The privacy line on the
  find screen shows folder count, file count, last event time and index size.

**Plain-words query parser (all find modules).** Deterministic first, no model
in the typing path. Order: operators (`in:`, `type:`, `size:`, `date:`,
`ext:`), then phrase rules (dates via `chrono` plus a relative-date grammar:
"last week", "in December", "yesterday"; sizes "over 50 pages", "under 5 MB",
"longer than 10 min"; kinds "photos", "videos", "pdfs", "spreadsheets"), then
the leftover words become the text query. Parsed pieces appear as removable chips
above the results so the user always sees how the words were read. A local model
is used only for meaning search, and only for the leftover words.

**Result actions (all find modules).** Open, Reveal in folder, Quick Look,
Copy path, Copy file, Drag out, Send to device, Then... (any module), Move to
Trash. Return opens, Command+Return reveals, Space previews.

**Share stack (section 9).** One Rust crate, `panoply-share`, serving every
share module.
- Protocol: LocalSend open protocol v2, pinned to the spec at
  github.com/localsend/protocol. Discovery by UDP multicast announce on
  224.0.0.167 port 53317, with the HTTP register fallback (`POST
  /api/localsend/v2/register`) scanning the local subnet when multicast is
  dropped. Transfer over HTTPS on the same port: `prepare-upload`, `upload`,
  `cancel`, `info`. Device identity is the SHA-256 of the device's self-signed
  certificate, stable across restarts.
- Encryption: TLS with the self-signed certificate, identity pinned by
  fingerprint. No plaintext path between two Panoply or LocalSend apps.
- Trust: first contact shows alias, device type and a four-word code derived
  from both fingerprints for visual comparison. "Trust this device" stores the
  fingerprint and then auto-accepts from it into a chosen folder. Untrusted
  senders always prompt. Optional PIN per receive session (the protocol's
  `pin` field).
- Panoply extensions ride in an optional capability field and are ignored by
  stock LocalSend, so interop never depends on them: clipboard payloads, job
  requests (Borrow the desktop), preset sync.
- QR browser fallback: the sender starts a one-time web page on a random port,
  URL carries a 128-bit random token, expires after 10 minutes or first
  completion, whichever comes first. HTTPS with a self-signed certificate gives
  a browser warning, plain HTTP gives none but is unencrypted on the LAN, so the
  default is HTTP plus token plus optional PIN for a one-time share, and the user
  can switch to HTTPS. The screen says which one is active.
- Client isolation: if a discovered peer cannot be reached back (our own probe
  from the peer, or a failed connect), the app says "This Wi-Fi stops devices
  talking to each other" and offers, in order: join a phone hotspot (QR with
  `WIFI:` credentials), start a hotspot from this computer (Windows Mobile
  Hotspot, NetworkManager on Linux, Internet Sharing on macOS which needs
  admin), Wi-Fi Direct on Android, and last, Far away transfer (labelled
  internet).

**Capture layer (section 10).** One Rust crate with a native backend per OS.
- macOS: ScreenCaptureKit for stills, windows and video, audio capture through
  it too. Permission is Screen and System Audio Recording, checked with
  `CGPreflightScreenCaptureAccess`, requested once with
  `CGRequestScreenCaptureAccess`. macOS requires an app restart after the grant,
  so the app offers "Restart Panoply" and restores the pending capture
  afterwards. macOS 15 and later re-asks periodically for apps that record
  screens, so the app handles a mid-session revoke without losing a recording
  (the file is finalized up to the last frame). Microphone and camera use their
  own prompts, asked only when the user turns them on.
- Windows: Windows Graphics Capture for windows and monitors, DXGI Desktop
  Duplication as fallback on older builds. No consent prompt exists for desktop
  apps. Elevated or secure-desktop windows (UAC, admin apps) cannot be captured
  by a non-elevated process, and DRM video appears black; both are detected and
  named in the error.
- Linux Wayland: `xdg-desktop-portal` Screenshot and ScreenCast, PipeWire
  frames, one portal prompt per session, a restore token saved so the prompt is
  not repeated where the compositor allows. Global shortcuts through the
  GlobalShortcuts portal where present. Programmatic region choice does not
  exist on Wayland, so area capture uses the portal's picker or our own overlay
  over a portal full-screen frame. Linux X11 uses XCB and XShm.
- Permission denied, everywhere: never a dead end. The screen shows one sentence
  on what failed, a button that opens the exact OS settings pane, a "Check
  again" button that polls, and a fallback that needs no permission: capture a
  Panoply window, capture the clipboard image, open an image file from disk, or
  import a screenshot made by the OS tool. No capture module may fail silently
  or produce a black image: a frame that is all black or all one value is
  detected and reported as a permission or DRM problem.

---

## 6. Documents and ebooks

### Office to and from PDF
- **Pattern:** A, instant transform.
- **Budgets:** exception on peak memory and first preview, for the Office side only. LibreOffice core (size shown before the first download) runs as a separate process with an OS memory limit (target under 500 MB). It stays warm only while files are queued and exits when the queue drains. Cold start means the first-page preview appears when ready, not in 100 ms, and never blocks the UI. PDF to Office is standard (own layout pass, streamed).
- **Job:** "Turn this Word, Excel or PowerPoint file into a PDF, or this PDF into an editable Office file."
- **Inputs:** DOCX, DOC, XLSX, XLS, PPTX, PPT, ODT, ODS, ODP, RTF, and PDF, many allowed.
- **On drop:** identify from bytes. Office input: count pages, detect macros, embedded fonts, tracked changes, hidden sheets and slides, password. PDF input: check for a text layer; a scan sets "run OCR first" and a PDF with real text keeps layout analysis on.
- **Options:** direction (from the type, never asked); PDF/A on or off (off; on when the file name contains "archive" or the user did it last time); Excel: fit sheet to one page wide (on when any column would split); PowerPoint: slides per page 1, include notes off; Word: keep tracked changes visible off (shows accepted view; on if the file has revisions, with a warning chip).
- **Preview:** page count and a first-page render. For PDF to Office, a side by side of the source page and the layout we recovered, before writing.
- **Output:** "name.pdf" or "name.docx" next to the original. Excel to PDF with several sheets makes one PDF with bookmarks per sheet.
- **Result card:** "12 pages, 640 KB. Fonts substituted: Calibri to Carlito (2 runs)."
- **Edge cases:** missing fonts (named, substituted with metric-compatible Carlito, Caladea, Liberation); macros never run, `.docm` converts with macros dropped and says so; huge spreadsheets (print area ignored warning, 1 M row sheet capped by page count, ask); password files ask; SmartArt and charts rendered by the engine and compared to a Word render; PDF to DOCX of two-column pages keeps columns as real columns, not text boxes.
- **Quality bar:** page-by-page visual diff against Microsoft Office's own export on a 200-file corpus (SSIM at least 0.95 and equal page count on at least 90 percent of files, tracked in CI and published). For PDF to Word, text order correct on a two-column and a table-heavy test set versus iLovePDF's output.
- **Engine:** LibreOffice core (headless, MPL-2.0), downloaded on first use and run as a separate process that stays warm only while files are queued and exits when the queue drains; PDF to DOCX by our own layout pass over `pdfium` text objects, `docx-rs` writer; Excel through `calamine` for read checks.
- **Surfaces:** bar ("word to pdf"), right-click "Convert to PDF", phone share sheet (phone sends to the desktop core when LibreOffice is absent, see Borrow the desktop), CLI `panoply doc topdf in.docx`, `panoply doc frompdf in.pdf --to docx`, MCP `doc_to_pdf`, `pdf_to_office`.

### Any file to Markdown
- **Pattern:** A, instant transform.
- **Budgets:** standard, plus OCR for scans and speech for audio. OS engines: no download, no resident memory for us. The fallback models (PaddleOCR is about 15 MB per language, the whisper.cpp model size is shown before the first download) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them.
- **Job:** "Give me clean text from this file for my notes or an AI tool."
- **Inputs:** PDF, DOCX, XLSX, PPTX, HTML, EPUB, EML, images (OCR), audio (transcript if the speech model is installed), ZIP of any of these; many allowed.
- **On drop:** detect structure: headings by font size and style, tables, lists, footnotes, columns, scanned pages. Sets heading levels and table style (pipe tables for narrow tables, HTML tables for merged cells).
- **Options:** tables as pipe or HTML (auto); images: drop, link to extracted files, or alt text from OCR (drop); page markers (off); one file per input or one combined file (per input; combined when more than 5 inputs from one folder).
- **Preview:** the Markdown rendered beside the source, with token count for AI use ("about 4,100 tokens").
- **Output:** "name.md", images in "name.assets/" when kept.
- **Result card:** "38 pages to 9,420 words, 14 tables, 6 images extracted."
- **Edge cases:** scanned PDFs (OCR on, flagged "text is from OCR, check numbers"); multi-column reading order; running headers and footers repeated on every page (detected by position repeat and removed); slide decks (one H2 per slide, notes under it); huge Excel (each sheet a table, capped rows with a note).
- **Quality bar:** reading order and table fidelity measured against a hand-labelled set of 100 documents, compared with Microsoft's markitdown on the same set; we match or beat on table cell accuracy and strip repeated headers it keeps. Over 200 pages in under 2 s on text PDFs.
- **Engine:** OS first, own model second. Two ML jobs, one ladder each. OCR for scans and images: macOS and iOS use Apple Vision `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` on iOS and macOS 26 (paragraphs and tables). Android uses ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean). Windows uses `Windows.Media.Ocr` (`OcrEngine`), and the Windows AI Text Recognition API on Copilot+ PCs. Linux uses the fallback. Speech to text for audio: macOS and iOS use Apple `SpeechAnalyzer` with `SpeechTranscriber` (macOS and iOS 26). Android uses `SpeechRecognizer.createOnDeviceSpeechRecognizer`, and ML Kit GenAI Speech Recognition (alpha). Windows uses Windows AI Speech Recognition (`SpeechRecognitionModel`, experimental, not production yet) and `Windows.Media.SpeechRecognition`. Linux uses the fallback. Fallback models (any platform where the OS engine is absent or fails the quality bar test in CI): a PaddleOCR-class model (Apache-2.0, ONNX) for OCR, and whisper.cpp (MIT) for speech. Non-ML parts: `pdfium-render`, `docx-rs`/own OOXML reader, `calamine`, `html2md`, `epub`, `mail-parser`.
- **Surfaces:** bar, right-click "Copy as Markdown", CLI `panoply md in.pdf`, MCP `file_to_markdown`. This is the agent-facing flagship of the section.

### Markdown to DOCX, HTML, PDF
- **Pattern:** B, tuned transform (the style is a live dial).
- **Budgets:** standard. Pandoc runs as a separate process per job and exits with it. Typst compiles in process.
- **Job:** "Turn this Markdown into a styled Word file, web page or PDF."
- **Inputs:** `.md`, `.markdown`, pasted text, or a folder of Markdown files.
- **On drop:** detect front matter (title, author, date), math, code fences with languages, tables, footnotes, images by relative path, a table of contents marker. Front matter fills the title block.
- **Options:** target (DOCX, HTML, PDF; last used); style (Article, Report, Letter, Plain; default Article); page size (A4 or Letter from device region); code theme; table of contents (on if 4 or more headings); self-contained HTML with images inlined (on).
- **Preview:** live rendered page, switches style in place.
- **Output:** "name.docx" and so on next to the source.
- **Result card:** "7 pages, 2 images embedded, math rendered, 3 broken image links."
- **Edge cases:** broken relative images (listed, not silent); wide tables (landscape section automatically in DOCX/PDF); right-to-left and CJK text (fonts chosen by script); math (LaTeX to native Word equations in DOCX, MathML in HTML).
- **Quality bar:** DOCX opens in Word with real heading styles and a working navigation pane and no "repair" prompt on 100 test files; PDF matches Pandoc's LaTeX output for page count within one page while needing no TeX install.
- **Engine:** `pandoc` (GPL-2.0, run as a separate downloaded process, never linked) for DOCX and HTML; PDF through Typst (Apache-2.0) from Pandoc's Typst writer, so no LaTeX is needed.
- **Surfaces:** bar, right-click, CLI `panoply md render in.md --to docx --style report`, MCP `markdown_render`.

### Spreadsheet convert
- **Pattern:** A, instant transform.
- **Budgets:** standard. Streaming readers and writers keep memory under 200 MB for a 500 MB CSV.
- **Job:** "Turn this CSV, Excel or JSON into the other."
- **Inputs:** CSV, TSV, XLSX, XLS, ODS, JSON, NDJSON, Parquet; many allowed.
- **On drop:** sniff encoding (UTF-8, UTF-16, Windows-1252, Shift-JIS), delimiter, quote style, header row, date formats, number locale ("1.234,56" vs "1,234.56"), leading-zero columns, sheet count.
- **Options:** target; sheet (all, as separate files for CSV); delimiter (detected); encoding out (UTF-8 with BOM only when the target is CSV opened in Excel on Windows); header row (detected); JSON shape (array of objects, or columns); keep formulas as formulas (on) or values.
- **Preview:** first 20 rows as the target will read them, with columns whose type was guessed marked.
- **Output:** "name.xlsx"; one file per sheet for CSV.
- **Result card:** "3 sheets, 48,210 rows, 2 columns kept as text to preserve leading zeros."
- **Edge cases:** leading zeros and long IDs (never turned into numbers or scientific notation; this is the bug that ruins Excel's own CSV open); dates parsed in the wrong locale (ambiguous dates flagged); multi-line cells; merged cells (unmerged and filled); formulas with external links (kept, flagged); 1 M row limit of XLSX (splits and says so).
- **Quality bar:** zero silent data changes on a 100-file corpus of ugly CSVs, verified by round trip hash of every cell as text; faster than opening the file in Excel for a 500 MB CSV (streaming, constant memory under 200 MB).
- **Engine:** `csv`, `calamine` (read), `rust_xlsxwriter` (write), `serde_json`, `arrow`/`parquet`, `encoding_rs`, `chardetng`.
- **Surfaces:** bar ("csv to xlsx"), rename-to-convert, right-click, CLI `panoply sheet convert in.csv out.xlsx`, MCP `sheet_convert`.

### Ebook convert
- **Pattern:** A, instant transform.
- **Budgets:** standard. Pandoc runs as a separate process per job. Books are processed one chapter at a time.
- **Job:** "Make this book readable on my Kindle, Kobo or phone."
- **Inputs:** EPUB, MOBI, AZW3, FB2, PDF, DOCX, HTML, TXT, CBZ; many allowed.
- **On drop:** detect DRM (named, refused, never circumvented), reflowable or fixed layout, cover, series metadata, languages, embedded fonts, whether a PDF has real text or is a scan. Target defaults to EPUB (what Kindle's Send to Kindle and Kobo accept); AZW3 is offered when the user has picked it before.
- **Options:** target (EPUB default, AZW3, MOBI legacy, FB2, PDF); PDF input: reflow text (on) or keep fixed pages as images (when the PDF is a scan or has more than half the area in images); strip headers and page numbers from PDF input (on); fix cover and metadata from the file name when missing (on); font embedding (off).
- **Preview:** cover, title block, chapter list, and a sample page rendering.
- **Output:** "title.epub" next to the original.
- **Result card:** "412 pages to 38 chapters, cover kept, no DRM."
- **Edge cases:** DRM protected ("This book is DRM protected. We do not remove DRM."); PDFs with two columns and footnotes (reflow keeps footnotes at chapter end); broken EPUBs (validated and repaired, report lists fixes); comic archives become fixed-layout EPUB; huge image books.
- **Quality bar:** output passes `epubcheck` with zero errors on a 100-book corpus (the public-domain Project Gutenberg set plus fixtures), and chapter structure matches Calibre's output for those books; conversion is faster than Calibre's `ebook-convert` on the same set. MOBI is written as AZW3/KF8; the old MOBI 6 target is kept only for old e-readers.
- **Engine:** `epub`/`zip`/`quick-xml` for EPUB, `mobi` crate for reading, our own KF8 writer, `pandoc` for FB2, DOCX and HTML to EPUB, `pdfium` for PDF reflow. Calibre is the benchmark, never a dependency (GPL-3.0, Python).
- **Surfaces:** bar, right-click "Convert book", phone share sheet, CLI `panoply book convert in.epub out.azw3`, MCP `ebook_convert`.

### Merge documents
- **Pattern:** C, visual editor (an ordered list you drag).
- **Budgets:** standard.
- **Job:** "Join several Word, text or Markdown files into one document, in order."
- **Inputs:** DOCX, ODT, MD, TXT, HTML, many. PDFs go to the PDF merge module and are redirected, not handled here.
- **On drop:** order by natural file name (chapter 2 before chapter 10); read each file's styles, headers, footers, page size, orientation. If styles differ, sets "Use the first document's styles".
- **Options:** order (natural name); style source (first doc); page break between documents (on); heading level shift (off; offered when each file starts with H1); table of contents (off); output type (same as first).
- **Preview:** the ordered list with page counts and a thumbnail strip of the join points.
- **Output:** "merged.docx" in the first file's folder.
- **Result card:** "5 files, 84 pages, styles unified, 2 style conflicts resolved."
- **Edge cases:** conflicting style names with different formatting (renamed, not silently overwritten); numbered lists restarting; comments and tracked changes kept per file; mixed portrait and landscape sections kept as sections; footnote numbering continues.
- **Quality bar:** opens in Word with no repair prompt on 100 merge sets; list numbering and footnote count equal to the sum of inputs; result no larger than inputs combined minus duplicated styles and fonts.
- **Engine:** OOXML part-level merge in Rust (`docx-rs` plus our style remapper); non-DOCX inputs go through `pandoc` first.
- **Surfaces:** panel, right-click on several files "Merge documents", CLI `panoply doc merge a.docx b.docx -o all.docx`, MCP `doc_merge`.

### Compare documents
- **Pattern:** I, inspect.
- **Budgets:** standard. Paragraph hashes stream first, and the word diff runs only on changed paragraphs.
- **Job:** "Show me exactly what changed between these two versions."
- **Inputs:** exactly two files of the same family (DOCX, PDF, TXT, MD, ODT), or two of any mix (converted to text first).
- **On drop:** decide which is older by modified time and revision metadata, and label them. Detect whether each has tracked changes already, whether PDFs have a text layer, and language (to choose the word splitter).
- **Options:** granularity (word default, sentence, character); ignore whitespace and case (off); ignore formatting-only changes (on); include tables and footnotes (on); output (view only, tracked-changes DOCX, redline PDF).
- **Preview:** the report itself: side by side with inserts, deletes and moves coloured and a change list on the left with counts.
- **Output:** a tracked-changes DOCX where the changes are real `w:ins` and `w:del` revisions the user can accept or reject in Word; or a redline PDF.
- **Result card:** "41 insertions, 28 deletions, 3 moved paragraphs, 2 formatting-only changes ignored."
- **Edge cases:** moved paragraphs (detected, not shown as delete plus insert); PDFs with different line breaks (compared as paragraphs, not lines); scans (OCR first, flagged); tables with added rows; very large files (streamed paragraph hashing, then word diff only on changed paragraphs).
- **Quality bar:** on 50 pairs with known edits, recall and precision of reported changes at least 98 percent by word; produced DOCX opens in Word with all revisions acceptable; time for two 300-page documents under 3 s, and no worse than Word's own Compare.
- **Engine:** `similar` (Myers and patience diff) at paragraph then word level, own OOXML revision writer, `pdfium` text.
- **Surfaces:** panel, right-click on two selected files "Compare", CLI `panoply doc compare old.docx new.docx --out changes.docx`, MCP `doc_compare`.

### Word count and reading time
- **Pattern:** F, instant answer (with a drop mode).
- **Budgets:** standard. Under 16 ms per keystroke. The CJK dictionaries (jieba, lindera) load only when those scripts appear.
- **Job:** "How many words is this, and how long to read it?"
- **Inputs:** typed or pasted text in the bar, or any readable file, many.
- **On drop:** extract text through the same engines as Any file to Markdown, skip headers, footers, code blocks and references by default as an option, and detect language so CJK counts characters.
- **Options:** include footnotes (off); include code blocks (off); reading speed (238 words per minute silent default, 150 aloud; editable; sources are settings, not claims); character limit target (shown only if the user typed one, e.g. 280).
- **Preview:** the answer appears while typing: words, characters with and without spaces, sentences, paragraphs, reading and speaking time.
- **Output:** answer copied on Return; table CSV for a batch.
- **Result card:** "1,204 words, 6,610 characters, 5 min read, 8 min aloud."
- **Edge cases:** hyphenated and apostrophe words, URLs and emoji (counted by Unicode segmentation rules, rules listed in the panel), CJK and Thai (no spaces, dictionary segmentation), Word's own count differences (a "match Word" toggle).
- **Quality bar:** counts equal Microsoft Word's on a 50-file corpus when "match Word" is on, and equal UAX 29 word boundaries otherwise; under 16 ms per keystroke up to 1 M characters.
- **Engine:** `unicode-segmentation`, `jieba-rs` for Chinese, `lindera` for Japanese.
- **Surfaces:** bar inline ("count words" then paste), panel, right-click, CLI `panoply text count file.docx`, MCP `text_count`.

### Grammar and spelling
- **Pattern:** B, tuned transform (suggestions are the dials).
- **Budgets:** standard. harper-core is small and runs in process. The optional rewrite feature is off by default. OS engines: no download, no resident memory for us. We ship no language model, so there is nothing for us to download or unload; where no OS engine or user-provided engine exists, the rewrite feature is hidden.
- **Job:** "Check my writing for mistakes without sending it anywhere."
- **Inputs:** pasted text, TXT, MD, DOCX; one at a time.
- **On drop:** detect language and dialect (en-US, en-GB, en-IN, others), document register (email, essay, code comments), and skip code blocks and URLs.
- **Options:** dialect (detected); rule groups (spelling, grammar, punctuation on; style off); custom dictionary (shared with the clipboard and snippets modules); correct in place (off; suggestions only).
- **Preview:** text with underlines and a list of fixes, each with accept and ignore, and "Accept all safe fixes".
- **Output:** the corrected text copied, or "name (checked).docx" with changes as tracked changes.
- **Result card:** "23 issues, 19 fixed, 4 ignored."
- **Edge cases:** names and jargon (learned into the dictionary on ignore); mixed languages (per-paragraph detection); very long documents (checked in chunks, results stream in); false positives get a "Not an error" button that feeds a local rule exclusion list.
- **Quality bar:** false positive rate lower than Harper's published defaults on our 100-document clean set (no flagged errors on professionally edited text beyond a set rate we fix to 1 per 1,000 words), and checking speed under 100 ms for a 1,000-word page. Beats the OS spellchecker by catching grammar, not only spelling.
- **Engine:** `harper-core` (Apache-2.0, Rust, offline) first; it is rule based and always the base. Optional rewrite suggestions, off by default: OS first, own model second. macOS and iOS: Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices). Android: Gemini Nano through ML Kit GenAI APIs (Proofreading, Rewriting; beta, listed devices). Windows: Phi Silica (Copilot+ PCs only, being replaced by Aion Instruct). Linux, and any platform where the OS engine is absent: the user's own Ollama, LM Studio or API key. Else the rewrite feature is hidden. We ship no language model (DECISIONS.md), so there is no fallback language model.
- **Surfaces:** bar ("check grammar" with clipboard), panel, CLI `panoply text check file.md`, MCP `text_check`.

### Translate document
- **Pattern:** B, tuned transform.
- **Budgets:** shares the engine ladder and the exception of Translate text (11.9). OS engines: no download, no resident memory for us. The fallback model (a per-pair Opus-MT model of about 100 MB class) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. The peak under 600 MB including the runtime applies to the fallback path only. Layout work is standard.
- **Job:** "Translate this document and keep it looking the same."
- **Inputs:** DOCX, PPTX, XLSX, PDF with text, MD, TXT, EPUB, HTML.
- **On drop:** detect source language per paragraph (mixed documents stay mixed), page layout, text that must not be translated (code, numbers, names, URLs, formulas), and fonts for the target script.
- **Options:** target language (from device locale or last used); source (auto); formality where the model supports it (off); glossary (a CSV the user keeps, applied as fixed terms); keep layout (on); bilingual output with original beneath each paragraph (off).
- **Preview:** first page translated beside the source, plus word count and estimated time on this device.
- **Output:** "name (fr).docx" in the same format.
- **Result card:** "34 pages, English to French, 11,200 words, 4 min, layout kept, 2 fonts replaced for the target script."
- **Edge cases:** text growth (German and Finnish overflow slide boxes; text is shrunk up to 10 percent, then flagged); right-to-left targets (layout mirrored for tables and lists); PDF (translated through DOCX route when layout is simple, otherwise the translation is overlaid as a new text layer and flagged); languages with no model (named, with the nearest pivot via English).
- **Quality bar:** formatting equal to source on 100 test files by structural diff of styles and run boundaries; translation quality reported as chrF on a public parallel test set per language pair next to the models we ship, and never claimed above that measure.
- **Engine:** OS first, own model second. macOS and iOS: Apple Translation framework (`TranslationSession`). Android: ML Kit Translation (on-device, downloads per language). Windows: none found, so the fallback. Linux: the fallback. Fallback model (any platform where the OS engine is absent, lacks the language pair, or fails the quality bar test in CI): CTranslate2 (MIT) running Opus-MT models (CC-BY-4.0, attribution in the app) per language pair, downloaded on first use (about the size of one model per pair); optional larger multilingual model for rare pairs. Run-level text replacement in Rust so formatting stays, on every path.
- **Surfaces:** panel, right-click "Translate", phone (small pairs only, otherwise Borrow the desktop), CLI `panoply doc translate in.docx --to fr`, MCP `doc_translate`.

### Summarize
- **Pattern:** B, tuned transform.
- **Budgets:** OS engines: no download, no resident memory for us. We ship no language model, so there is no model for us to download or unload, and the old bundled-model memory exception is gone. When the user supplies their own Ollama, LM Studio or API key, that engine runs in its own process under its own memory rules, and Panoply holds none. Long documents are map-reduced by section, so memory does not grow with length. Where no engine exists the feature is hidden. Phones use the OS engine or Borrow the desktop.
- **Job:** "Give me the short version of this."
- **Inputs:** any readable file (via Any file to Markdown), pasted text, many (one summary each, or one combined).
- **On drop:** extract text, count tokens, detect kind (paper, contract, meeting notes, email thread, article) and pick a template.
- **Options:** length (5 bullets default; one paragraph; one page); style (bullets, TL;DR, action items for meeting notes, key terms and dates for contracts); language (same as source); quote sources (on: each bullet gets a page reference).
- **Preview:** streaming summary with the source passage highlighted when a bullet is hovered.
- **Output:** text copied, or "name (summary).md" next to the file.
- **Result card:** "34 pages to 180 words, 5 bullets with page references, 22 s on this device."
- **Edge cases:** documents longer than the model context (map-reduce by section, each section cited); scanned files (OCR first, flagged); legal and medical text (a fixed footer "This is a summary, not advice"); hallucinated numbers (every number in the summary must appear in the source or it is dropped and the check shown).
- **Quality bar:** every number, date and name in a summary is present in the source (automatic check, target 100 percent, failing outputs are regenerated once and then flagged); runs within the idle-memory budget by loading the model only while working and unloading after 60 s.
- **Engine:** OS first, then the user's own engine, else the feature is hidden. macOS and iOS: Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices). Android: Gemini Nano through ML Kit GenAI Summarization (beta, listed devices). Windows: Phi Silica (Copilot+ PCs only, being replaced by Aion Instruct). Linux, and any platform where the OS engine is absent: the user's own Ollama, LM Studio or API key. Else the summarize feature is hidden. We ship no language model (DECISIONS.md), so there is no fallback language model. A phone without an OS engine can Borrow the desktop when the paired desktop has one of the engines above. Map-reduce by section and the number check stay the same on every path.
- **Surfaces:** bar ("summarize" plus drop), panel, right-click, CLI `panoply doc summarize paper.pdf --bullets 5`, MCP `doc_summarize`.

### Email and contact files
- **Pattern:** I, inspect (with convert actions).
- **Budgets:** standard. MBOX streams by offsets, under 150 MB for a 5 GB file. libpff is a downloaded helper process for PST, started per job.
- **Job:** "Open this email, calendar or contact file and get what is inside."
- **Inputs:** MSG, EML, MBOX, PST (read only), ICS, VCF; many.
- **On drop:** detect the kind, count messages, events, contacts, attachments, encodings and time zones; flag S/MIME or encrypted mail.
- **Options:** action (view default; export attachments; convert to PDF, Markdown, CSV, JSON; merge VCF; split MBOX into EML); include headers (short); time zone for events (the event's own, shown with local time beside).
- **Preview:** the report: a message list or event table, with the body rendered safely (remote images blocked, scripts never run).
- **Output:** per action; PDFs get a header block with From, To, Date, Subject and attachment list.
- **Result card:** "1,204 messages, 311 attachments (2.1 GB), 14 inline images."
- **Edge cases:** MBOX files of several GB (streamed, indexed by offsets, never loaded whole); broken encodings (charset guessed per part); winmail.dat (TNEF unpacked); recurring events with exceptions (expanded in the table, kept in ICS output); vCard 2.1 to 4.0 differences; HTML mail tracking pixels (blocked and counted).
- **Quality bar:** attachments extracted byte-identical on a 100-message corpus from Outlook, Gmail Takeout and Thunderbird; a 5 GB MBOX lists in under 5 s on SSD with memory under 150 MB.
- **Engine:** `mail-parser` (Apache-2.0), `cfb` for Outlook MSG, `tnef`, `icalendar`/`ical`, `vcard`, own MBOX streaming reader, `libpff` through a downloaded helper for PST.
- **Surfaces:** panel, right-click, CLI `panoply mail open box.mbox`, `panoply mail extract`, MCP `mail_inspect`, `mail_extract_attachments`.

### Query data files
- **Pattern:** C, visual editor (a query box over tables).
- **Budgets:** standard, with one setting: DuckDB runs with a 256 MB memory limit and spills to a temp folder, so files larger than RAM stay inside the cap. The optional plain-words box uses an OS engine or the user's own engine: no download and no resident memory for us. We ship no language model, so there is nothing to unload.
- **Job:** "Run SQL on this CSV, JSON or Excel file."
- **Inputs:** CSV, TSV, JSON, NDJSON, XLSX, ODS, Parquet, SQLite; many (each becomes a table).
- **On drop:** infer schema (types, nullability, key candidates), show row counts, name tables from file names, and write a starter query (`select * from sales limit 100`). With 2 or more files, suggest joins on columns with matching names and value overlap.
- **Options:** the SQL; plain-words box ("total by month") that drafts SQL with the local model, always showing the SQL before running (off until the model is installed); output (table, CSV, XLSX, JSON, chart suggestion).
- **Preview:** result grid with query time and row count, live while typing for queries under 200 ms.
- **Output:** the result saved next to the source as "name (query).csv".
- **Result card:** "1,204,331 rows scanned in 0.4 s, 12 rows returned."
- **Edge cases:** files larger than RAM (DuckDB streams from disk); mixed-type columns (read as text, flagged); duplicate column names; locale numbers; read-only always (the file is never modified, writes go to a new file); queries that would return 10 M rows (grid is paged).
- **Quality bar:** a 1 GB CSV `group by` returns in under 5 s on an M-class laptop, within 1.5x of the `duckdb` CLI; no more rows scanned than needed; the original untouched byte for byte.
- **Engine:** DuckDB (MIT) embedded via `duckdb-rs` with `memory_limit` set to 256 MB and a temp folder for spill (DuckDB's default is most of RAM), with its `excel` and `sqlite` extensions bundled; text-to-SQL, optional and OS first: macOS and iOS use Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices), Android uses Gemini Nano through ML Kit GenAI Prompt API (beta, listed devices), Windows uses Phi Silica (Copilot+ PCs only, being replaced by Aion Instruct), Linux and any platform without an OS engine use the user's own Ollama, LM Studio or API key, else the plain-words box is hidden. We ship no language model (DECISIONS.md), so there is no fallback language model.
- **Surfaces:** panel, bar ("query sales.csv"), CLI `panoply data query sales.csv "select ..."`, MCP `data_query` (read only, row limited by default).

### 3D files
- **Pattern:** C, visual editor (viewer first, repair as an action).
- **Budgets:** exception on peak memory: mesh size sets it, about 40 bytes per triangle (1 M triangles is about 40 MB, 10 M about 400 MB), so very large meshes go over 300 MB. The viewer uses level of detail and repair works per shell. OpenCASCADE is a downloaded helper process, started per job.
- **Job:** "Look at this 3D model and make it printable."
- **Inputs:** STL, OBJ, 3MF, PLY, GLB, STEP/STP (view).
- **On drop:** load the mesh, measure bounding box and units guess (mm vs inches by size), triangle count, manifold check, holes, flipped normals, self-intersections, thin walls under a printer minimum, loose shells.
- **Options:** repair (on for printable formats when problems exist); printer profile (build volume, minimum wall, nozzle; default generic 220 mm bed); unit (guessed); orientation helper (off); decimate to a triangle count (off).
- **Preview:** orbit view with problem areas highlighted in colour and a count by type.
- **Output:** "name (repaired).stl" or 3MF; STEP to STL conversion with tolerance setting.
- **Result card:** "Closed 14 holes, flipped 312 normals, removed 3 loose shells. Watertight, 96 x 40 x 22 mm."
- **Edge cases:** huge meshes (10 M triangles, rendered with level of detail); non-manifold edges that cannot be auto-fixed (shown and listed, not guessed); STEP assemblies (each part listed); models in inches loaded as 25x too small (unit prompt).
- **Quality bar:** repaired output is watertight and manifold on 100 known-broken Thingiverse-style meshes, validated by a manifold checker and by slicing in PrusaSlicer headless with no errors; opens a 1 M triangle STL to first frame in under 1 s.
- **Engine:** `stl_io`, `tobj`, `gltf`, `3mf` writer, `manifold3d` (Apache-2.0) for repair and booleans; STEP through OpenCASCADE (LGPL-2.1) as a downloaded helper because of its size; `wgpu` viewer.
- **Surfaces:** panel, right-click, CLI `panoply 3d repair in.stl`, MCP `mesh_repair`, `mesh_info`.

### Resume builder
- **Pattern:** E, generator.
- **Budgets:** standard. Typst compiles in process and the live preview is debounced.
- **Job:** "Make a clean resume and check that it passes applicant tracking."
- **Inputs:** none; or an old resume (PDF, DOCX) to import.
- **On drop:** parse an imported resume into sections (contact, summary, experience, education, skills) with dates and employers; flag what could not be parsed instead of dropping it.
- **Options:** template (single column ATS-safe default; two column; compact); paper (A4 or Letter by region); font (embedded, ATS-safe set); sections and order; saved details from the personal profile (name, email, phone, links) filled automatically; target job text (optional, paste a posting to see keyword gaps).
- **Preview:** live page, with the ATS check beside it.
- **Output:** PDF (with a real text layer and correct reading order) plus DOCX and plain text on request; "name resume.pdf".
- **Result card:** "1 page, ATS check: 11 of 12 passed, one issue: date format inconsistent."
- **Edge cases:** more than 2 pages (suggests cuts, never shrinks text below 9 pt); non-Latin names and scripts; employment gaps (no judgment, optional label); photos (off for US, offered for regions where common).
- **Quality bar:** the PDF text extracts in correct order with `pdftotext` and with our own reader on all templates; the ATS check rules are listed and linked, each a measurable parse test (single column reading order, no text in images or tables for core fields, standard heading names, contact in body not header). We do not claim "passes ATS" as a score from any vendor.
- **Engine:** Typst (Apache-2.0) templates, `docx-rs` for the Word version, own parser for import.
- **Surfaces:** panel, bar ("resume"), CLI `panoply resume build data.yaml`, MCP `resume_build`.

### Printables
- **Pattern:** E, generator.
- **Budgets:** standard.
- **Job:** "Print letterhead, business cards, graph paper or lined paper."
- **Inputs:** none; optional logo and contact details.
- **On drop:** a dropped logo is analysed for transparency, aspect and colours; sets the card colour accents and warns below 300 DPI at print size.
- **Options:** kind (graph, dot grid, lined, isometric, music staff, planner, letterhead, business card); paper (by region); spacing (5 mm, 1/4 inch by region); margins; line colour (light grey default); bleed and crop marks for cards (on when the kind is business card); cards per sheet (auto, 10 on A4 and Letter).
- **Preview:** the page at true scale with a ruler.
- **Output:** PDF at print size, vector.
- **Result card:** "A4, 5 mm graph, vector, prints at 100 percent. Cards: 10 per sheet, 3 mm bleed."
- **Edge cases:** printer margins that cut edges (a "printable area" check against the chosen paper); card sizes that differ by country (85 x 55 mm and 3.5 x 2 in, listed); CMYK for print shops (off; offered as PDF/X-1a option).
- **Quality bar:** a 1 cm ruler on the preview measures 10 mm +/- 0.1 mm when printed 100 percent (test print kit shipped as a PDF), and files are vector.
- **Engine:** Typst templates, `lopdf` for cropmarks.
- **Surfaces:** panel, bar ("graph paper"), CLI `panoply print paper --kind graph --size a4`, MCP `printable_make`.

---

## 7. Find anything on your device

All modules below read the Panoply Index defined above. Index, freshness,
budgets, privacy, query parsing and result actions are defined once there; each
spec adds only what is special to the module.

### Instant file search
- **Pattern:** H, find (backed by G for the index).
- **Budgets:** exception: always on (the Panoply Index). Idle CPU 0 percent, freshness is event driven. The name index is memory-mapped with OS-reclaimable pages, target under 100 bytes per file (1 M files is under 100 MB). The first scan runs at background priority and is bounded.
- **Job:** "Find any file by name, the moment I start typing."
- **Inputs:** a query; no file. A dropped file or folder scopes the search to it.
- **On start:** first run lists volumes and proposes folders (Home, Desktop, Documents, Downloads, external drives on, system folders off). The name index builds first and is searchable within seconds while the content index continues. On Windows, offers the one-time elevated helper for USN journal reading and says what it does and that it can be removed; declining falls back to a parallel walk, slower on first scan only. Whole-disk is the default on macOS only after the user grants Full Disk Access, with the same guided deny flow as capture (button to Privacy and Security, "Check again", search the granted folders meanwhile).
- **Options:** scope (everywhere, this folder, this volume); match mode (substring default, prefix, whole word, regex with `re:`, wildcard); case (smart case); include hidden and system files (off; `hidden:` operator); sort (relevance by name match quality, recency, then path depth; name, size, date on header click); excluded folders list (editable, with live count of files removed from search).
- **Preview:** results update per keystroke with path, size, modified, and match highlight; Space shows a quick preview; a count "1,204,331 files indexed, updated 2 s ago".
- **Output:** a result list; selection actions per "Result actions". Export results to CSV.
- **Result card:** n/a (find modules show the result list); "Open" logs to History.
- **Edge cases:** rename storms and git checkouts (coalesced); network shares and cloud placeholders (OneDrive, iCloud files not downloaded are listed by name, content never forced to download, marked with a cloud icon); case-insensitive and normalization-insensitive match for macOS NFD names; path longer than 260 characters on Windows; bind mounts and symlink loops; files deleted while listed (row greys out, never opens a missing file); volumes unplugged mid-search.
- **Quality bar:** on a 1 M file test volume, keystroke to first page of results under 16 ms at p95 (the VISION budget); cold start to searchable from a saved index under 300 ms; initial name scan of 1 M files finishes while the user waits under what Everything takes on the same NTFS volume with the USN path (head to head, published), and on macOS beats Spotlight on completeness: on a seeded corpus of 10,000 files (including dotfolders, source trees, odd extensions) we return every file and Spotlight's miss list is the number we report. Idle CPU 0 percent, resident memory for the name index under 100 bytes per file target.
- **Engine:** the Panoply Index name index and SQLite; `notify`, own USN reader (`windows` crate), `ignore` for walks, `jwalk` parallelism.
- **Surfaces:** the bar is the home of this module (global shortcut then type), panel, CLI `panoply find "quarterly report"`, MCP `file_find` (returns paths and metadata only, never contents).

### Search inside files
- **Pattern:** H, find.
- **Budgets:** exception: bounded background work. Content indexing uses at most one worker thread at background priority, pauses on battery below 20 percent, in low-power modes and during fullscreen apps or calls, and costs 0 after catch-up. The tantivy writer heap is capped at 50 MB and released after each commit.
- **Job:** "Find the document that has these words in it."
- **Inputs:** a query; optional scope.
- **On start:** after the name index is built, content indexing starts at the lowest priority on the folders the user chose (default Documents, Desktop, Downloads). It shows progress as "Indexed 48,200 of 91,000 documents, paused on battery". Search works on what is already indexed, with a banner while incomplete.
- **Options:** scope; kinds (documents, code, email, ebooks; all); match (words, exact phrase in quotes, `near:`, fuzzy for typos on by default with edit distance 1); language analyzers (detected per file); include scanned PDFs through OCR (off by default because of cost, one toggle with an estimated time and battery impact).
- **Preview:** result rows with the matching line, a highlighted snippet, page number or line number, and the file's path.
- **Output:** list; opening a PDF result jumps to the page and highlights the hit; copy snippet with citation ("report.pdf, p. 12").
- **Result card:** n/a; the footer shows "312 matches in 41 files, 6 ms".
- **Edge cases:** huge files (text capped per file at a configurable limit, default the first 5 MB of text, with the cap noted per file in the index); binary files skipped by content sniffing; code with identifiers (camelCase and snake_case split so "parseUrl" matches "parse url"); encrypted PDFs and password Office files (listed under "not searchable", never opened); stale entries after edits (re-extracted on the file event within 2 s of the write settling); email attachments indexed only if the mail file is in scope.
- **Quality bar:** on a 50 GB mixed corpus, query p95 under 30 ms; index size at most 25 percent of the text size (target); recall equals ripgrep's on plain text and code for the same terms (test), and beats Spotlight on PDFs and Office files with multiple word forms ("invoices" finds "invoice"); initial indexing never exceeds one core.
- **Engine:** `tantivy`, with extractors named in the Index section, `lingua` for language detection, `rust-stemmers`.
- **Surfaces:** bar with a `in files:` prefix or the Tab key from file search, panel, CLI `panoply find --content "wifi password"`, MCP `file_search_content` (returns snippets, not whole files, and a flag to require user confirmation before returning text from folders marked private).

### Screenshot search
- **Pattern:** H, find.
- **Budgets:** exception: bounded background work. OCR of new screenshots runs when charging or on the first idle minute, one at a time, with OS OCR where available. OS engines: no download, no resident memory for us. The fallback models (PaddleOCR is about 15 MB per language, SigLIP for meaning mode is off by default) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. SigLIP has no OS engine, so it always loads per job.
- **Job:** "Find the screenshot with this text in it."
- **Inputs:** a query; the screenshot folders (macOS screenshot location, Pictures/Screenshots on Windows, Pictures on Linux) and Panoply's own captures are on by default.
- **On start:** detects screenshots by filename pattern, PNG with screen-sized pixel dimensions and no EXIF camera data, and Panoply's own tag. Runs OCR on each new screenshot when charging or on first idle minute, the text goes into the Index full-text store so "wifi password" is a text query and ordinary search syntax works.
- **Options:** scope folders; date (chips); OCR languages (from device locale, plus detected); include phone screenshots synced to the device (on); text and meaning mode (text only default; meaning adds SigLIP vectors for "that graph about sales").
- **Preview:** a thumbnail grid with the matched words boxed on the image; Space for full size.
- **Output:** list; actions include Copy text, Copy image, Annotate, Pin to screen, Redact then share.
- **Result card:** n/a.
- **Edge cases:** dark mode UIs and tiny text (OCR run at 2x scale when the first pass finds fewer than expected characters per area); screenshots of passwords and keys (a privacy tag set by a pattern scan, hidden from the grid in screen share mode, text never leaves the index); code and terminal screenshots (monospace-tuned recognition); very large scrolling captures (tiled OCR); images in another language (script detected); thousands of old screenshots (backfilled newest first, resumable).
- **Quality bar:** on a 500-screenshot labelled set, a top-3 hit for a text query from the screenshot in at least 95 percent of known-item queries (target, reported with the set), compared with Spotlight's image text search on the same set on macOS; OCR time per screenshot under 400 ms with OS OCR on an M-class chip; query under 16 ms.
- **Engine:** OS first, own model second. OCR: macOS and iOS use Apple Vision `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` on iOS and macOS 26. Android uses ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean). Windows uses `Windows.Media.Ocr` (`OcrEngine`), and the Windows AI Text Recognition API on Copilot+ PCs. Linux uses the fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): a PaddleOCR-class model (Apache-2.0) via `ort`. Meaning mode (image embeddings): OS engine: none verified for image meaning search. Own model on every platform: SigLIP image embeddings (Apache-2.0) via `ort`.
- **Surfaces:** bar ("find screenshot wifi password"), panel, phone (photo library screenshots album), CLI `panoply find --screenshots "order id"`, MCP `screenshot_search`.

### Photo search by meaning
- **Pattern:** H, find.
- **Budgets:** exception: model-backed background work, only while charging, with a first-run estimate. SigLIP base (image tower about 370 MB in fp32, about 95 MB int8, size shown before the first download) loads per job and unloads after 60 s idle. There is no OS engine for this job, so the model is the engine on every platform and the peak and load figures apply everywhere. Peak under 600 MB (target). Queries read only the small int8 vector index, memory-mapped.
- **Job:** "Find the photo I am describing in plain words."
- **Inputs:** a query; photo folders and, on phones, the photo library (permission asked when the module is turned on).
- **On start:** embeds each photo once with the image model (thumbnail at model resolution, never the full image) and stores 768 or smaller int8 vectors. Runs while charging. A first-run estimate is shown ("42,000 photos, about 35 minutes while charging"). Faces are not indexed by default; "People" is a separate opt-in that clusters faces locally and never names anyone without the user typing a name.
- **Options:** scope; date and place chips from the query parser ("in Goa last December" becomes a GPS and date filter, then meaning ranks inside it); strictness slider for matches; include videos by keyframes (off); exclude hidden or private albums (honors the OS hidden album).
- **Preview:** photo grid ranked by similarity with the score hidden unless asked, a "More like this" button.
- **Output:** list; Send to device, Make collage, Make passport photo, Remove background actions.
- **Result card:** n/a.
- **Edge cases:** words the model handles badly (counts, text in the image, left and right), so a query with quoted text routes to Screenshot search style OCR instead and says so; RAW and HEIC (decoded through the same codecs as the Images module); duplicates and burst shots (grouped); sensitive categories ("my passport", "receipts", "ID") are normal queries and the answers stay local; photos on a cloud-only library (not downloaded, listed as unindexed with a count).
- **Quality bar:** on a labelled 2,000-photo set with 100 queries, recall at 10 of at least 80 percent (target), reported with the set and compared against OS photo search (Apple Photos, Google Photos' on-device equivalent where testable); text-to-image query p95 under 25 ms at 100,000 photos; index at most 1 KB per photo.
- **Engine:** OS engine: none verified for image meaning search. Own model on every platform: SigLIP base (Apache-2.0) through `ort`, with Core ML and DirectML execution providers, `usearch` HNSW; OpenCLIP LAION models as an alternative (MIT).
- **Surfaces:** bar ("photos red car at night"), panel, phone app (primary), CLI `panoply find --photos "red car at night"`, MCP `photo_search`.

### Metadata search
- **Pattern:** H, find.
- **Budgets:** standard. Extractors run in the same index walk, so there is no extra scan.
- **Job:** "Find files by facts about them: where, when, how long, which camera."
- **Inputs:** a query in plain words or operators.
- **On start:** the Index already holds metadata; this module adds extractors in the same walk: EXIF (camera, lens, GPS, date taken), PDF info (pages, author, producer), audio tags, video (duration, codec, resolution), Office properties. No extra scan.
- **Options:** the parser turns "photos taken in Goa in December" into kind photo, place Goa (offline reverse geocoding from a bundled place database), month December; "PDFs over 50 pages" into type, pages greater than 50; "videos longer than 10 min"; "files from this camera" (the camera model of a selected photo); "documents by this author". Every phrase maps to a chip, editable. Dates default to the local calendar and time zone, and photos use the date taken, not the modified date.
- **Preview:** results with the matched field shown ("Goa, 15.3 N 74.0 E, 12 Dec 2024").
- **Output:** list; export to CSV with all metadata columns.
- **Result card:** n/a.
- **Edge cases:** photos with no GPS (listed under "no location" when asked, not guessed from filename); wrong camera clocks (a time offset per camera, set once); ambiguous place names (Paris, Texas) resolved by disambiguation chips; time zones on date taken (EXIF offset tag where present, otherwise local, flagged); stripped metadata (counted).
- **Quality bar:** every parsed phrase has a test; at least 95 percent of 200 hand-written queries produce the intended filter on first try (target, measured on our query set and published); correct results versus ExifTool as the ground truth on a 5,000 photo set, zero mismatches on GPS and date.
- **Engine:** `kamadak-exif`, `lofty` (audio tags), `lopdf`/`pdfium` info, `mp4parse`/ffprobe-equivalent in our media crate, bundled GeoNames (CC-BY) for places.
- **Surfaces:** bar, panel, CLI `panoply find "photos from goa december"`, MCP `file_find_by_metadata`.

### Similar images
- **Pattern:** H, find (with a cleanup action).
- **Budgets:** standard on the hash-only path. The embedding path uses the SigLIP model of Photo search, with the same load and unload rules. No OS engine exists for it, so it loads on first use and unloads after 60 s idle on every platform.
- **Job:** "Find photos that are the same picture, edited or resized."
- **Inputs:** a folder, a set of photos, or one photo ("find copies of this").
- **On start:** none beyond the photo embeddings from Photo search by meaning plus a cheap perceptual hash computed at scan time. Without the embedding model installed it runs on the hash only and says it finds only resizes and recompressions.
- **Options:** similarity level (identical-looking, edited, same scene); group by (date, folder); keep rule for cleanup (largest resolution, then oldest, then not in Downloads; shown, never applied without a click); include videos (off).
- **Preview:** each group as a row of images with size, resolution, date and the proposed keeper; checkmark for what would be removed.
- **Output:** groups; cleanup moves to Trash, never deletes; a report CSV.
- **Result card:** "318 groups, 1,044 extra copies, 3.2 GB reclaimable. Moved 1,044 to Trash."
- **Edge cases:** burst photos (similar, not duplicates; a "burst" label and a keep-best-by-sharpness option); crops and screenshots of a photo (matched by embedding, flagged lower confidence); RAW plus JPEG pairs (kept together, never offered as duplicates); live photos; photos in two libraries with different edits.
- **Quality bar:** on a labelled set with resized, recompressed, cropped and filtered variants, precision at least 98 percent at the "edited" level and recall at least 90 percent (targets, reported), zero false auto-selections on RAW plus JPEG pairs; scan speed at least 500 photos per second for hash-only mode.
- **Engine:** `image_hasher` (pHash and dHash), `usearch` nearest neighbor. Embeddings: OS engine: none verified for image meaning search. Own model on every platform: SigLIP embeddings via `ort`.
- **Surfaces:** panel, right-click "Find similar", phone, CLI `panoply dupes images ~/Pictures --level edited`, MCP `images_similar`.

### Duplicate finder
- **Pattern:** H, find.
- **Budgets:** standard. Each byte is read at most once, with constant memory.
- **Job:** "Find files I have more than once, and clean up safely."
- **Inputs:** folders or volumes, many.
- **On start:** the Index already holds size and file id. Candidates are files of equal size, then equal first and last 64 KB hash, then full BLAKE3 hash; nothing is called a duplicate before the full hash matches. Hardlinks and clones (APFS clonefile, Btrfs reflinks) are detected as the same data, not extra space.
- **Options:** scope; minimum size (1 MB default); keep rule (oldest, newest, in the preferred folder, shortest path); never touch folders (system, apps, `.git`, photo libraries and package bundles); action (Trash default, replace with hardlink, or just list).
- **Preview:** groups with the proposed keeper and reclaimable space per group and in total; Quick Look for any copy.
- **Output:** a cleanup that moves to Trash and records an undo; a CSV report.
- **Result card:** "2,114 duplicate files, 18.4 GB reclaimable. Moved 2,114 to Trash. Undo available."
- **Edge cases:** files in use or open (skipped and listed); photo libraries and app bundles (excluded by default because deleting inside them corrupts them); same content on two drives (kept, listed as intentional backup when the user marks a drive as a backup); zero-byte files (excluded); files changing during the scan (rechecked by size and mtime before action); permissions blocked.
- **Quality bar:** zero false positives (every duplicate verified by full hash; tested on a corpus with deliberate same-size near-matches); no file removed while another copy was missing at action time (re-verify keeper exists and matches immediately before the move); scan of 1 TB across 400,000 files reads each byte at most once, and finishes limited by disk speed, compared with `fclones` and `rmlint` timing on the same volume.
- **Engine:** `blake3`, our walker, `trash` crate, reflink detection through `reflink-copy`.
- **Surfaces:** panel, right-click on a folder, CLI `panoply dupes ~/Documents --min 1mb --dry-run`, MCP `files_duplicates` (list only; actions require a confirmation token returned to the user).

### Big file finder
- **Pattern:** H, find (with a map view).
- **Budgets:** standard.
- **Job:** "Show me what is eating my disk."
- **Inputs:** a volume or folder; default the system volume.
- **On start:** reads sizes from the Index (instant) and a fast tree walk for allocated size, correcting for sparse, compressed, cloned and cloud-placeholder files; no content reading.
- **Options:** view (treemap, sunburst, list); minimum size; group by (folder, type, age, app); "Not touched in a year" filter; hide system folders (on); known space hogs recognised and labelled ("Xcode DerivedData", "Docker images", "node_modules", "iOS backups", "Downloads installers") with a safe-to-delete note and a link to the owner app's own cleanup where deletion is not safe.
- **Preview:** the map with sizes; hover shows path and last used.
- **Output:** deletes go to Trash; "Archive to external drive" moves and leaves a catalog entry (Search unplugged drives).
- **Result card:** "Freed 22.6 GB: 14 files to Trash, 3 caches cleared."
- **Edge cases:** APFS snapshots and purgeable space (shown as separate, never deletable here); sparse VM disk images (allocated vs apparent size both shown); hardlinks counted once; permission-denied folders (listed with size unknown, with a grant button); folders that change during the scan.
- **Quality bar:** totals match `du` (apparent and allocated) within 0.1 percent on a test volume; scan of 1 M files in a time bounded by the Index read (target under 3 s on SSD) and faster than DaisyDisk and WinDirStat on the same volume (head to head); the map renders at 120 fps.
- **Engine:** the Index, `jwalk`, `treemap` layout (squarified) rendered natively.
- **Surfaces:** panel, bar ("what is using my disk"), CLI `panoply disk big ~ --min 1gb`, MCP `disk_usage`.

### Recent everything
- **Pattern:** H, find (opens as an empty-query state).
- **Budgets:** standard.
- **Job:** "What did I just download, save or screenshot?"
- **Inputs:** none; time window.
- **On start:** nothing extra. It is a view of the Index sorted by a "last interesting event" time: created or first seen, not modified, so rewrites by a sync tool do not push old files up. Sources: Downloads, Desktop, screenshots, Documents, folders the user adds, and Panoply outputs.
- **Options:** window (today default, 7 days, 30 days); kinds (all); group by (time, source app where the OS records it, such as the quarantine "downloaded from" attribute on macOS and Zone.Identifier on Windows); exclude folders.
- **Preview:** a timeline with thumbnails, the source ("downloaded from example.com" when the OS records it), and size.
- **Output:** list; actions "Move to...", "Rename", "Send to device", plus Then...
- **Result card:** n/a.
- **Edge cases:** partial downloads (`.crdownload`, `.part` shown as "downloading" and updated when finished); files re-saved by a sync client (not counted as new); the Trash and cache folders excluded; clock changes.
- **Quality bar:** a file appears in the list within 1 s of the download finishing, tested across Safari, Chrome, Firefox and Edge on the three desktop OSes; zero entries for sync-only rewrites on a 7-day test.
- **Engine:** the Index event stream, `xattr` and NTFS alternate data streams for origin.
- **Surfaces:** bar (empty query, or "recent"), panel, phone (Files), CLI `panoply recent --since 1d`, MCP `files_recent`.

### What is this file?
- **Pattern:** I, inspect.
- **Budgets:** standard. Files over 100 GB are read at the header and tail only.
- **Job:** "I have a file with no extension or a strange one. What is it and how do I open it?"
- **Inputs:** one or many files, any type.
- **On drop:** read magic bytes and structure (not the extension), identify container formats down to the real type (a renamed ZIP that is a DOCX, an APK, an EPUB), list embedded streams, entropy, and signs of risk (executables, macros, double extensions, mismatch between extension and content).
- **Options:** none by default; "Open with Panoply" suggestions listed by module that reads the type; "Open with" suggestions of installed apps.
- **Preview:** the report: real type, confidence, size, created by (tool version when stored), a content peek (first text lines, image, or hex), and one-click actions.
- **Output:** a report; "Rename to the right extension" action (the original name kept in History).
- **Result card:** "Real type: EPUB e-book (ZIP container). Extension .zip is wrong. 3 modules can open it."
- **Edge cases:** encrypted blobs (high entropy, no header, reported as "likely encrypted or compressed, no known format"); truncated files; polyglots; files over 100 GB (header and tail only); never executes the file.
- **Quality bar:** correct real type on at least 98 percent of a 2,000-file corpus of mislabelled and extensionless files (target), better than `file` plus extension guess on Office subtypes and ZIP-based formats; under 50 ms per file.
- **Engine:** `infer`, `tree_magic_mini` plus the freedesktop shared-mime-info database, own ZIP, OLE and RIFF structure walkers.
- **Surfaces:** bar, drop on the bar (the fallback when no module claims a type), right-click, CLI `panoply what file.bin`, MCP `file_identify`.

### Search unplugged drives
- **Pattern:** H, find (with G for catalog upkeep).
- **Budgets:** standard. A catalog walk runs only after the user says yes, on request or on mount. Thumbnails are capped in size.
- **Job:** "Find a file on a drive that is not plugged in."
- **Inputs:** an external drive or network volume to catalog; then queries.
- **On start:** when a new volume is mounted the app asks once: "Catalog this drive?" If yes, it walks the drive and stores names, sizes, dates, metadata and a thumbnail for images and video frames (optionally text excerpts for documents), keyed by the volume UUID and a user name ("Wedding backup"). On unmount the entries stay, tagged offline.
- **Options:** content depth (names and metadata default; plus text; plus thumbnails); re-scan on mount (on, using the drive's file events when it was last mounted via the saved event id where the file system supports it, else a quick mtime walk); retention (forever until removed).
- **Preview:** results show the drive name, a plug icon and "offline", and what to do: "Plug in Wedding backup to open".
- **Output:** results; Open works the moment the drive is mounted; the action "Copy this file's path" always works. Export the full catalog to CSV for printing.
- **Result card:** n/a.
- **Edge cases:** two drives with the same name (volume UUID distinguishes, name disambiguated); drives that changed while off (flagged "last seen 12 Mar, may be out of date"); drives mounted at a different path (matched by UUID); a file moved off the drive (stale entry removed on re-scan); case-insensitive vs sensitive file systems; very large catalogs (millions of rows stay inside the same Index, thumbnails capped by size).
- **Quality bar:** catalog 1 TB and 500,000 files in a time bounded by directory read speed, searchable offline in under 16 ms; catalog size at most 200 bytes per file for names and metadata (target); compared with the paid catalog tools (NeoFinder, Cathy-class) on feature and speed, public numbers only from our own runs.
- **Engine:** the Index with a volume table, `jwalk`, thumbnails via the Images module.
- **Surfaces:** bar, panel, CLI `panoply find --offline "wedding"`, `panoply drives catalog /Volumes/Backup`, MCP `drives_search`.

### Search inside videos
- **Pattern:** H, find.
- **Budgets:** exception: the heaviest background module. Speech to text and SigLIP (about 95 MB int8, no OS engine, so on every platform) run only while charging, one video at a time, at the lowest priority, resumable, with the cost shown before it starts. OS speech engines: no download, no resident memory for us. The whisper.cpp fallback (base or small, about 140 to 470 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. SigLIP loads per job and unloads after 60 s idle. The review recommends shipping it after the images index.
- **Job:** "Find the video where someone said this, or this scene appears."
- **Inputs:** a query; video folders.
- **On start:** the module is off until the user adds folders, then shows the cost ("1,240 videos, 310 hours, about 9 hours of processing while charging") before starting. Two passes: speech to text with timestamps (stored in the full-text store), and keyframes every few seconds embedded with SigLIP. Runs at the lowest priority, only while charging, one video at a time, resumable.
- **Options:** folders; languages for speech (detected, or fixed to save time); scene detection (on, keyframes at cuts) or fixed interval; include audio files and voice memos (on); speaker labels (off).
- **Preview:** results with a timestamped snippet or a keyframe strip; click starts playback at the second.
- **Output:** a list; "Clip this moment" sends the span to Trim video; copy transcript.
- **Result card:** n/a.
- **Edge cases:** music and noise (language detection says "no speech", skipped, saves time); screen recordings (OCR on keyframes added, off by default); very long recordings (processed in chunks, searchable as they finish); videos with multiple audio tracks (all indexed); changed or replaced files (hash by duration and size plus partial content).
- **Quality bar:** transcript word error rate reported per language on public sets for the shipped model (not claimed beyond that), and a known-phrase query finds the right video within 2 s of the actual time in at least 95 percent of 200 test queries on clean speech (target); processing speed faster than real time by at least 10x on an M-class chip with the small model; query under 25 ms across 1,000 hours of transcript.
- **Engine:** OS first, own model second. Speech to text: macOS and iOS use Apple `SpeechAnalyzer` with `SpeechTranscriber` (macOS and iOS 26). Android uses `SpeechRecognizer.createOnDeviceSpeechRecognizer`, and ML Kit GenAI Speech Recognition (alpha). Windows uses Windows AI Speech Recognition (`SpeechRecognitionModel`, experimental, not production yet) and `Windows.Media.SpeechRecognition`. Linux uses the fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): `whisper.cpp` (MIT) via `whisper-rs` with the base or small multilingual model (MIT), voice activity detection with Silero (MIT). Keyframe meaning: OS engine: none verified for image meaning search. Own model on every platform: SigLIP via `ort`. Non-ML: `tantivy` for transcripts.
- **Surfaces:** bar ("find video where I say launch date"), panel, CLI `panoply find --video "launch date"`, MCP `video_search` (returns timestamps and snippets).

---

## 8. Files

### Batch rename
- **Pattern:** C, visual editor (a table of old and new names).
- **Budgets:** standard.
- **Job:** "Rename a lot of files by a rule, and see it before it happens."
- **Inputs:** files or folders, many.
- **On drop:** reads file kinds and metadata. Photos suggest `{date taken}_{n}` and videos `{date}_{time}`; music suggests `{track} {title}`; a folder of "IMG_0001" style names suggests numbering by date; scanned names like "Scan 14" suggest the title from the first line of OCR text if the user opts in.
- **Options:** pattern with tokens (`{name}`, `{n:3}`, `{date:YYYY-MM-DD}`, `{exif.camera}`, `{parent}`, `{tag.artist}`), find and replace with regex, case style, extension case, remove characters, sequence start and step, sort order for numbering (name, date, selection order).
- **Preview:** old name, new name and a conflict marker for every row, live as the pattern is typed; conflicts (two files to one name) and illegal characters (per target OS) shown in red and block Run.
- **Output:** files renamed in place; the old to new map is saved so Undo restores every name.
- **Result card:** "412 renamed, 0 conflicts. Undo available."
- **Edge cases:** case-only renames on case-insensitive file systems (done through a temporary name); names that exceed 255 bytes; Unicode normalization (NFC chosen, NFD names compared safely); names Windows forbids (CON, trailing dot, trailing space) when the folder will reach a Windows machine; files in use; the rename crossing a sync client (renames not copies); chained rename swaps (a to b and b to a) handled.
- **Quality bar:** a 100,000 file rename plan previews in under 500 ms and executes with zero partial states (atomic per file, journal for crash recovery, Undo verified in tests); beats Windows Explorer batch rename and PowerToys PowerRename on token support (we measure on a list of 30 real-world patterns we publish); no data loss under kill during run.
- **Engine:** `regex`, `chrono`, `kamadak-exif`, `lofty`, a write-ahead journal in SQLite.
- **Surfaces:** panel, right-click "Rename...", CLI `panoply rename --pattern "{date:YYYY-MM-DD}_{n:3}" *.jpg --dry-run`, MCP `files_rename` (dry run default; apply needs a plan id).

### Auto-organize
- **Pattern:** G, always on.
- **Budgets:** exception: always on, event driven. 0% idle CPU, no polling, resident under 10 MB (target). A rule evaluates in under 1 ms per file.
- **Job:** "Keep my Downloads and Desktop tidy without me doing it."
- **Inputs:** watched folders.
- **On start:** nothing runs until the user turns on a rule set. First run offers a starter set based on a scan of the chosen folder ("63 percent of Downloads is installers and PDFs"), shown as a dry run over the existing files. Nothing moves until the user presses Apply.
- **Options:** rules as "if file matches, then do": match on type, name pattern, size, age, source (the origin URL attribute), content (a text hit), dimension (screenshot); do: move, copy, rename, tag, send to trash after N days, run a Panoply chain (convert, compress, strip metadata). Rules are ordered, the first match wins; a "Review first" mode queues the moves for a one-click approval.
- **Preview:** a dry run that shows each file and the rule that would move it, and a "Why this file moved" line in History.
- **Output:** files moved with the original relative name; a log; Undo restores the last 7 days of moves.
- **Result card:** "Moved 18 files today: 11 to Documents/Invoices, 7 to Pictures/Screenshots."
- **Edge cases:** partial downloads (waits until the file is stable for 3 s and has no `.part` suffix); files still open or being written; name collisions at the destination (suffix, never overwrite); rules that loop (a rule moving into a watched folder; detected and blocked); rule applied to a synced folder (warning); rules on removable drives (only when mounted).
- **Quality bar:** zero moves of a file that was still being written, over a 1,000-file test with large and slow downloads; a rule evaluates in under 1 ms per file; idle CPU 0 percent; covers the common Hazel rules (type, date, name, source, tag, run script) so a migration guide maps 100 percent of a published list of 25 typical Hazel rules, and runs on Windows and Linux where Hazel does not.
- **Engine:** the Index event stream, our rule engine, Panoply chains as the action set.
- **Surfaces:** panel, bar ("tidy Downloads"), CLI `panoply organize run --rules downloads.yaml --dry-run`, MCP `files_organize_plan` (plan only; apply needs confirmation).

### Archives
- **Pattern:** A, instant transform.
- **Budgets:** standard. Pack and unpack stream, and listing reads only the central directory. The RAR plugin is a downloaded helper process, started per job.
- **Job:** "Zip or unzip anything, including the odd formats."
- **Inputs:** any files and folders for packing; ZIP, 7Z, RAR, TAR, TAR.GZ, TAR.XZ, TAR.ZST, TAR.BZ2, GZ, XZ, ZST, ISO, DMG (read), CAB, ARJ, LZH, split volumes.
- **On drop:** detect from bytes. Unpack: peek at the listing for a single top-level folder (extract as is) or loose files (wrap in a folder named after the archive, the case The Unarchiver handles); detect encryption, filename encoding (CP437, Shift-JIS, GBK), zip bombs by ratio, path traversal entries, symlinks leaving the folder. Pack: default ZIP for a mixed or Windows-bound set, TAR.ZST for developer folders, 7Z for maximum compression.
- **Options:** format (from context); compression level (normal); password and AES-256 (off); split size (off; common presets 25 MB, 100 MB, 2 GB for mail and FAT32); exclude patterns (`.DS_Store`, `Thumbs.db`, `__MACOSX` by default on pack); keep permissions and timestamps (on).
- **Preview:** archive listing with sizes and the estimated unpacked size; for pack, estimated archive size.
- **Output:** "name" folder or "name.zip" next to the source; in-place extraction never overwrites silently.
- **Result card:** "Unpacked 3,412 files, 1.2 GB, to 'photos'. Filenames decoded as Shift-JIS."
- **Edge cases:** wrong filename encodings (shown as mojibake in other tools; we guess the encoding and let the user switch live); zip bombs (stop at an unpacked size ratio of 1000 to 1 or free-disk limit with a plain warning); path traversal and absolute paths (entries rejected, listed); RAR (read through UnRAR source license, see Engine; writing RAR is not offered); archives with 3 or more nested levels (offered, not unpacked recursively by default); multi-part ZIP/7Z/RAR volumes (found by sibling names); corrupt archives (extract what is readable, list failures).
- **Quality bar:** extracts every archive in a 300-file corpus of real-world and malformed samples byte-identical to the reference tool (7-Zip), zero crashes under fuzzing (cargo-fuzz in CI), zero files written outside the target folder across the traversal test set; pack speed within 1.5x of `zstd` and `7z` on the same machine; listing a 10 GB archive under 1 s. Beats The Unarchiver on Windows and Linux simply by existing there, and on macOS by encoding handling and speed on our corpus.
- **Engine:** `zip`, `sevenz-rust`, `tar`, `flate2`/`zstd`/`xz2`, `unrar` crate (UnRAR license: read only, shipped as a downloaded plugin, not linked into the core), `cab`, `chardetng` for names, `aes`/`zip` AES.
- **Surfaces:** right-click "Compress" and "Extract here", bar ("zip"), rename-to-convert for archive types, phone, CLI `panoply archive pack ./project -o project.tar.zst`, `panoply archive unpack x.rar`, MCP `archive_pack`, `archive_unpack`.

### Encrypt
- **Pattern:** B, tuned transform.
- **Budgets:** standard. Files stream in chunks with constant memory. Key derivation uses memory on purpose (Argon2id at 64 MB, or age's scrypt at about 256 MB) and frees it right after.
- **Job:** "Lock a file or folder with a password."
- **Inputs:** files and folders, many; or an encrypted file to unlock.
- **On drop:** a file with Panoply's header is detected and offered "Unlock". Folders are packed first. Detects passwords reused from the history (never stored), and checks the chosen password strength.
- **Options:** password (strength meter by `zxcvbn`, a generated passphrase offered with Password generator); one archive for many files (on); also hide file names (on, because names leak); key file instead of or with a password (off); output format ("Panoply .pnx" default, or standard encrypted ZIP AES-256 for recipients without Panoply, or age format).
- **Preview:** output name and size; for ZIP the warning that file names are visible unless the option is on.
- **Output:** "name.pnx" or the chosen format; the original is left alone and the user is reminded it is still unencrypted (option "Move original to Trash after verifying").
- **Result card:** "Locked 14 files, 88 MB, AES-256-GCM, names hidden. Verified by decrypting a test chunk."
- **Edge cases:** forgotten password (no recovery, said before encrypting and a "type it again" check); very large files (streamed in chunks, authenticated per chunk, constant memory); partially written output on cancel (removed); open file being locked; metadata leaks (size is visible, said plainly); weak password warning blocks nothing but shows it.
- **Quality bar:** the format uses `age` or a documented Argon2id plus XChaCha20-Poly1305 scheme with a written spec and test vectors; interoperable with the reference `age` tool for the age output; tamper test (flip any byte) always fails authentication; throughput at least 1 GB per second per core on an M-class chip (target), no slower than 7-Zip's AES on the same file.
- **Engine:** `age` crate (MIT/Apache) for default, `argon2`, `chacha20poly1305`, `zip` AES for compatibility; secrets zeroized with `zeroize`.
- **Surfaces:** right-click "Lock with password", bar ("encrypt"), phone, CLI `panoply encrypt secrets/ -o secrets.pnx`, MCP `file_encrypt` (password supplied by the user prompt, never in logs; MCP refuses plaintext passwords unless passed as a key file path).

### Shred
- **Pattern:** A, instant transform.
- **Budgets:** standard.
- **Job:** "Delete this so it cannot be recovered."
- **Inputs:** files and folders, many; "free space" as a target.
- **On drop:** detects the storage type and file system (SSD, HDD, APFS, NTFS, ext4, Btrfs, network, cloud-synced folder) and decides what is honest: on HDD with an in-place file system, overwrite then delete works; on SSD, flash and copy-on-write systems (APFS, Btrfs, ZFS) overwriting a file does not reliably destroy the old blocks, snapshots and wear leveling keep copies, so the module says so and recommends full-disk encryption plus key destruction, or the drive's secure erase.
- **Options:** method (single random pass default for HDD, which current guidance treats as sufficient; more passes available but not default); rename before delete (on); free space wipe (off); include Trash (on).
- **Preview:** what will be removed and an honest line: "This drive is an SSD. Shredding makes recovery harder but cannot guarantee it. For a guarantee, erase the volume's encryption key."
- **Output:** files gone, a report listing paths.
- **Result card:** "Overwrote and removed 14 files (2.1 GB). SSD: best effort, not guaranteed."
- **Edge cases:** files in a Time Machine, File History or cloud-sync (copies elsewhere listed and offered for deletion); snapshots; hardlinked files (data stays while another link exists, said); read-only and system files; confirm step requires typing the count for more than 100 files.
- **Quality bar:** on an HDD or a plain loop image test volume, a raw scan after shred finds none of the planted marker strings (automated test); on SSD and APFS, the tool never prints "securely erased", only the honest best-effort line, tested; speed limited by disk.
- **Engine:** own overwrite with `rand_chacha`, `fsync`, `std::fs`, platform calls (`F_NOCACHE` on macOS, `FILE_FLAG_WRITE_THROUGH` on Windows).
- **Surfaces:** right-click "Shred", panel, CLI `panoply shred file.pdf`, MCP `file_shred` (always asks the human to confirm; never auto-run).

### Checksums
- **Pattern:** I, inspect (with F style instant answer).
- **Budgets:** standard. Constant memory and hardware SHA instructions.
- **Job:** "Check that this download is not corrupted or tampered with."
- **Inputs:** files, many; a pasted expected hash; a checksum file (SHA256SUMS, `.sha256`, `.md5`).
- **On drop:** hash with the algorithm guessed from a pasted or neighbouring value by length (32 hex is MD5, 40 SHA-1, 64 SHA-256, 128 SHA-512). If the clipboard holds a hash, it is compared immediately. If a sibling `SHA256SUMS` exists, every file is verified against it, and a `.asc` signature is noted.
- **Options:** algorithms (SHA-256 default; MD5, SHA-1, SHA-512, BLAKE3, CRC32, XXH3); expected value; uppercase (off); verify signature through GPG or minisign if a key is present (off).
- **Preview:** progress, then Match or No match in large type with the first and last bytes aligned for a visual check.
- **Output:** values copied on Return; a `SHA256SUMS` file on request.
- **Result card:** "SHA-256 matches. 4.7 GB in 3.1 s."
- **Edge cases:** hashes pasted with whitespace, `0x`, colons or uppercase (normalized); MD5 and SHA-1 shown with a note that they are broken for security but fine for corruption checks; a folder (a manifest of per-file hashes plus a tree hash); a file that changes while hashing.
- **Quality bar:** equals `shasum`, `certutil`, `sha256sum` outputs on a test set for all algorithms; throughput within 1.1x of `b3sum` for BLAKE3 and bounded by disk for the others (hardware SHA instructions used).
- **Engine:** `sha2`, `sha1`, `md-5`, `blake3`, `crc32fast`, `xxhash-rust`.
- **Surfaces:** bar inline (paste hash, drop file), right-click "Checksum", CLI `panoply hash file.iso --sha256 --expect <hash>`, MCP `file_hash`.

### Compare folders
- **Pattern:** C, visual editor (a two-pane diff, with a sync action).
- **Budgets:** standard. The plan is held as compact entries (about 200 bytes each, so 500,000 files is about 100 MB).
- **Job:** "What is different between these two folders, and make one match the other."
- **Inputs:** exactly two folders (or a folder and a drive or an archive).
- **On drop:** detect which side is newer by last modified, flag when one side is a mounted volume or an archive, set direction (left to right when the left is the newer).
- **Options:** compare by (size and time default; content hash; name only); ignore patterns (`.DS_Store`, `.git`, thumbnails); time tolerance 2 s for FAT, 1 s for Windows daylight shifts; sync mode (none, mirror one way, update only newer, two way with a conflict list); delete extra files on the target (off).
- **Preview:** tree with states (only left, only right, newer left, newer right, different, same), counts and the exact plan: copy 214 files (3.4 GB), delete 0, overwrite 12.
- **Output:** files copied through Trash safety (overwritten and deleted files go to a recoverable ".panoply-sync-trash" or the OS Trash); a log.
- **Result card:** "Copied 214, replaced 12, 0 deleted, 3.4 GB in 41 s, verified by hash."
- **Edge cases:** case-different names on case-insensitive volumes; Unicode normalization; symlinks and hardlinks; permission and ACL differences; files changing during sync; moved or renamed files (detected by hash and size to avoid recopying); long paths on Windows; FAT 4 GB file limit.
- **Quality bar:** zero data loss in 10,000 randomized sync scenarios with crashes injected mid-copy (fuzz-style test in CI); a no-change re-compare of 500,000 files in under 2 s from the Index and cached state (target); throughput within 1.1x of `rsync` and Robocopy on the same data; compares to Beyond Compare and `rsync --dry-run` outputs on a corpus with identical classification.
- **Engine:** the Index, `walkdir`, `blake3`, `reflink-copy`, a copy engine using `copy_file_range`, `clonefile` and `CopyFileEx`.
- **Surfaces:** panel, right-click on two selected folders, CLI `panoply folders diff a b`, `panoply folders sync a b --mirror --dry-run`, MCP `folders_diff` (plan); applying requires confirmation.

### Split and join
- **Pattern:** A, instant transform.
- **Budgets:** standard. Constant memory under 20 MB.
- **Job:** "Cut a big file into parts and put it back together."
- **Inputs:** any file; or parts (`.001`, `.002`, `.part1`, `.aa`).
- **On drop:** sees whether the file is a part (by name and header) and offers Join. For a large file, offers common sizes from context: 25 MB (mail), 100 MB, 2 GB (older FAT32 and some upload caps), 4 GB minus one byte (FAT32 limit), or "N equal parts".
- **Options:** part size or count; naming (`.001` default); checksum file alongside (on); also create a self-checking joiner (off).
- **Preview:** number of parts and last part size.
- **Output:** "name.001" and so on in a folder; join writes the original name.
- **Result card:** "Split into 12 parts of 2 GB, checksums written. Joined result verified."
- **Edge cases:** missing part in a join (named by number); parts out of order; different splitters' formats (HJSplit `.001`, Unix `split` `.aa`, 7-Zip volumes); resulting file larger than the target volume (checked); overlap in progress cancel.
- **Quality bar:** a joined file hash equals the original on 100 sizes including 0, 1 byte, exact multiples of the part size and 4 GB plus 1; speed bound by disk, constant memory under 20 MB.
- **Engine:** plain Rust IO, `blake3`.
- **Surfaces:** right-click "Split..." and "Join", CLI `panoply split big.iso --size 2gb`, MCP `file_split`, `file_join`. Recommended to merge with Archives (split archives exist there already).

### Metadata for any file
- **Pattern:** I, inspect (with edit and strip actions).
- **Budgets:** standard. Lossless rewrites stream and never decode pixels.
- **Job:** "Show, edit or remove the hidden information in this file."
- **Inputs:** any file; many.
- **On drop:** reads every metadata family it can (EXIF, XMP, IPTC, ID3, PDF info, Office core and custom properties, video atoms, file system attributes and quarantine tags, document revision authors), ranks the risky items (GPS, author name, device serial, software, comments, tracked-change author, thumbnails of cropped content), and sets the strip defaults to the risky ones.
- **Options:** view; edit field (text, date, GPS picker); strip (risky only default; everything); keep color profile and orientation (on, so images do not rotate or shift colour); also strip embedded thumbnails (on); copy metadata from another file (off).
- **Preview:** a before and after table with each removed field marked and "this photo shows its location at 15.3 N, 74.0 E" in plain words.
- **Output:** "name (clean).jpg" next to the original; edits write a copy unless overwrite is chosen.
- **Result card:** "Removed 31 fields including GPS, device serial and 'Author: A. Smith'. Image pixels unchanged."
- **Edge cases:** JPEG lossless stripping (segment removal, no recompression); PDFs with history in incremental saves (rewritten fully so old text is not recoverable); Office files with hidden text and revision authors; video container metadata and chapter titles; filenames that carry data (flagged, not changed); XMP sidecars next to RAW files.
- **Quality bar:** after strip, ExifTool lists no GPS, author or serial in 200 test files across 10 formats; pixel data identical for JPEG, PNG and WebP (hash of decoded pixels); for PDFs, no stripped string is found with `strings` on the output; faster than ExifTool on 10,000 files (head to head).
- **Engine:** `kamadak-exif`/`little_exif`, `img-parts` for lossless segments, `lofty`, `lopdf`, OOXML part rewriting, `mp4` atom rewriter. ExifTool is a benchmark and a test oracle, never a dependency. Merge with the Images and Privacy modules' strip function (one engine).
- **Surfaces:** panel, right-click "Remove metadata", phone share sheet (strip before sharing), CLI `panoply meta show photo.jpg`, `panoply meta strip *.jpg`, MCP `file_metadata`, `file_strip_metadata`.

### Quick preview
- **Pattern:** I, inspect.
- **Budgets:** standard after one rule: Office previews show the embedded thumbnail or a cached first page at once, and the full render replaces it when ready. Previewers run in a sandboxed worker that exits 5 s after the last preview. Other types open in under 100 ms.
- **Job:** "Look inside any file without installing anything."
- **Inputs:** any file; arrow keys move between files in a folder.
- **On drop or select:** picks a viewer from the real type: text and code (highlighted), Markdown rendered, JSON and CSV as tables, images and RAW, PDF, Office (through the PDF route), archives (listing), 3D, audio waveform, video with scrubbing, fonts (a specimen), SQLite (tables), Parquet, certificates, and a hex view as the fallback for the rest.
- **Options:** none by default; view toggles (rendered or source); copy text; Open with.
- **Preview:** the viewer itself, opening on Space, in under 100 ms for files under 50 MB. Office files show the embedded thumbnail or a cached first page at once, and the LibreOffice render replaces it when ready.
- **Output:** none; copy and "Open in Panoply module" actions.
- **Result card:** n/a.
- **Edge cases:** huge files (windowed read, text opens at the start in under 100 ms for a 10 GB log); malformed and hostile files (rendering runs in a sandboxed worker that exits 5 s after the last preview; a crash shows a plain message, never takes the app down); password-protected files (asks); files on a slow network share (reads a first window, shows progress); DRM media (said).
- **Quality bar:** a hostile-file fuzz corpus never crashes the host process (parser fuzzing in CI); time to first paint under 100 ms for the common types on a 50 MB test set; covers all the types of the OS quick look and adds the ones it lacks on Windows and Linux (list published, count ours against macOS Quick Look and Windows Preview Handlers on the same 50 types).
- **Engine:** `syntect`, `pdfium`, `image` and RAW decoders, LibreOffice (as in Office to PDF) for Office, `wgpu` for 3D, `symphonia` for audio, platform decoders for video, `ttf-parser` for fonts.
- **Surfaces:** Space key anywhere in the panel and the bar, right-click "Preview", CLI `panoply preview file` (terminal image or text), MCP `file_preview` (text and a rendered image).

### Fonts
- **Pattern:** B, tuned transform (with an install action).
- **Budgets:** standard.
- **Job:** "Preview a font, convert it between formats, or install it."
- **Inputs:** TTF, OTF, WOFF, WOFF2, TTC, variable fonts; many; or a folder.
- **On drop:** reads family, style, weights, variable axes, glyph and language coverage, license field (fsType embedding bits), and installed duplicates.
- **Options:** target (WOFF2 for web; TTF; OTF); subset to characters or a language (off; offered when the target is WOFF2); keep hinting (off for web); install for me (user fonts folder, no admin needed) or for all users (admin); sample text editable.
- **Preview:** a specimen with the sample text at several sizes, a glyph grid and, for variable fonts, axis sliders; a size estimate for the converted file.
- **Output:** "name.woff2" next to the original; installing copies the file to the OS font folder.
- **Result card:** "TTF 412 KB to WOFF2 128 KB, 69% smaller, subset to Latin."
- **Edge cases:** fonts with embedding restrictions (installable-only, preview-only shown as a warning, conversion blocked for "no embedding" licensed fonts and said); collections (.ttc split into single fonts); duplicate family names; broken tables (repaired where safe); variable fonts to static instances.
- **Quality bar:** converted WOFF2 renders identically to the source in a browser screenshot diff for 200 fonts (zero pixel difference at the tested sizes); size within 2 percent of Google's `woff2_compress` reference.
- **Engine:** `ttf-parser`, `read-fonts`/`write-fonts` (fontations, Apache-2.0), `woff2` (Brotli via `brotli`), `fontdb` for installed-font checks, `subsetter` (fontations/typst).
- **Surfaces:** panel, right-click on a font ("Install font"), CLI `panoply font convert in.ttf --to woff2`, MCP `font_convert`.

### Rename to convert
- **Pattern:** G, always on (a trigger for the Universal converter).
- **Budgets:** exception: always on, event driven. It rides on the Index event stream, so there is no extra watcher and no extra resident memory beyond the rule set. 0% idle. The review recommends folding it into the Universal converter as a toggle.
- **Job:** "Rename photo.heic to photo.jpg in Finder or Explorer and it converts."
- **Inputs:** none; it watches rename events in chosen folders.
- **On start:** off by default. Turning it on asks which folders to watch (Desktop, Downloads, Documents by default) and explains that the original is kept. Detects the case where the extension changes from one real type to another by comparing the file's bytes (still HEIC) with the new extension (JPG), which is the signal of a rename-to-convert rather than a plain rename.
- **Options:** folders; allowed pairs (a list drawn from the Universal converter's supported pairs; image, audio, document pairs on); keep the original as "name (original).heic" (on) or send to Trash; confirm toast before converting (on for the first 5, then a one-click undo).
- **Preview:** a toast "Converting photo.heic to JPG... done, 1.8 MB" with Undo for 30 s.
- **Output:** the file at the new name is the converted file; the original kept as above.
- **Result card:** the toast, and a History entry.
- **Edge cases:** a user who really renamed a file to hide its type (confirm shown when the content has no valid conversion; otherwise the rename is left alone); Windows Explorer warns on extension changes (the toast arrives after confirmation); iCloud and OneDrive placeholder files (not converted until local); renames inside synced folders; rapid repeats; unsafe pairs (EXE to anything is never converted).
- **Quality bar:** converts within 1 s of the rename for a 5 MB photo (target); zero false triggers across 1,000 ordinary renames in a test run; zero data loss: the pre-rename bytes always recoverable in 30 s via Undo. No other tool offers this behaviour on all three desktops, which makes it a differentiating demo rather than a numbers contest.
- **Engine:** Index event stream (FSEvents, USN, fanotify) plus the Universal converter's graph.
- **Surfaces:** a background toggle (G) with a settings panel; CLI `panoply watch convert ~/Downloads`; MCP none (it is an event trigger, not an action). Recommended: fold into the Universal converter as a toggle, not a separate module.

### File shelf
- **Pattern:** G, always on.
- **Budgets:** exception: always on, event driven. A drag monitor only, 0% idle CPU, resident under 10 MB (target). Items are references, so no file data is held.
- **Job:** "Park files for a moment while I move between windows."
- **Inputs:** dragged files, folders, text and images.
- **On start:** a small shelf appears at a screen edge when a drag starts and the pointer approaches it (or by shortcut). Items are references, not copies, unless the source is about to disappear (a download in a temporary folder, a file on an ejected drive).
- **Options:** edge (left default); items kept until (cleared when emptied, or 24 hours); drag out all as a group (on); actions on a shelf item (the same Result actions, plus "Then...").
- **Preview:** thumbnails with names; a count and total size.
- **Output:** dragging out copies or moves as the target app asks; "Drop all into a folder" and "Zip all" shortcuts.
- **Result card:** n/a.
- **Edge cases:** files renamed or deleted while parked (item shows missing); drag from a browser (the file is saved to a temp location first); apps that refuse drag data; full screen apps and multiple displays; Wayland where global drag targets are limited (shelf runs as a regular always-on-top window).
- **Quality bar:** the shelf appears within 100 ms of drag start and accepts the drop on the first attempt in 100 of 100 trial drags across Finder, Explorer, Safari, Chrome, Mail and Slack (on macOS and Windows); drag-out works into the same apps. Compare against Yoink on behaviour, which we do not take numbers from.
- **Engine:** native drag and drop APIs per platform; Rust core stores references and thumbnails.
- **Surfaces:** screen-edge shelf and shortcut, no CLI (it is a GUI gesture), MCP `shelf_list`, `shelf_add` for agents handing files to the user.

---

## 9. Share between your devices

All modules use `panoply-share`, defined above: LocalSend v2 compatible
discovery and transfer, TLS with fingerprint pinning, trust by fingerprint plus a
four-word check code, QR browser fallback, and the client-isolation ladder. Each
spec adds only what is special. Surfaces for all of them: bar ("send"), panel,
phone share sheet, Android quick settings tile, CLI under `panoply share`, MCP
under `share_`.

### Send to device
- **Pattern:** G for discovery, A for the send itself.
- **Budgets:** standard. It shares the Receive listener, and discovery is event driven.
- **Job:** "Send this to my phone or laptop, right now."
- **Inputs:** files, folders, text, images; many.
- **On drop:** the device list appears sorted by recent use then by signal; one tap sends. Detects what the target can take (a stock LocalSend app accepts files and text; a Panoply device also accepts jobs and clipboard payloads) and what is in the drop: a folder is sent as a tree, a HEIC file bound for a Windows laptop offers "Convert to JPG first" as the host's smart default.
- **Options:** target (last used; several allowed, see Send to many); convert before sending (off, offered by target type); strip metadata (off, offered when GPS is present and the target is not a trusted device); PIN required (off for trusted devices).
- **Preview:** the target's alias and type, total size, estimated time from measured link speed.
- **Output:** received by the target in its chosen folder; the sender keeps the original.
- **Result card:** "Sent 4 files, 1.2 GB to Anzal's iPhone in 14 s (86 MB/s). Encrypted, direct, no server."
- **Edge cases:** the target is asleep (a wake attempt where the OS allows, else "offline"); IPv6 and multiple interfaces (VPN and virtual adapters ignored by default, the adapter list shown in diagnostics); sender or receiver changes network mid-transfer (resume by byte range for files over 100 MB); many small files (10,000 files stream in one session, not 10,000 handshakes); iOS backgrounding (the transfer continues while the app is open and asks the user to keep it open; a background URLSession is used for downloads where Apple allows); names that are illegal on the receiver (renamed with a note); client isolation, see the ladder; Android 13 and later notification permission for progress.
- **Quality bar:** the kill test passes: our client sends to and receives from stock LocalSend on macOS, Windows, Android and iOS, in both directions, with 1 file, 1 GB file, 10,000 small files, and Unicode names, all hashes equal. Throughput within 10 percent of stock LocalSend on the same Wi-Fi link (head to head, same pair of devices), and above 90 percent of an `iperf3` measure of the link. Discovery shows a nearby device in under 2 s after both are open. Measured against AirDrop only on the Apple pair for reference, never claimed to beat it.
- **Engine:** `axum` with `rustls` for HTTPS, plain UDP multicast discovery on 224.0.0.167 via `socket2`, `rcgen` for the certificate, `reqwest` client, resumable ranges.
- **Surfaces:** also the right-click "Send to" in Finder and Explorer; CLI `panoply share send photo.jpg --to "Anzal's iPhone"`, MCP `share_send` (lists devices, requires a user-approved target on first use).

### Receive
- **Pattern:** G, always on.
- **Budgets:** exception: always on, event driven. The listener sits at 0% idle CPU with no periodic beacon (it announces once at start and on network change, then answers queries). Resident under 15 MB (target). On phones it runs only while the app is open.
- **Job:** "Accept files from my other devices without doing anything."
- **Inputs:** none; incoming transfers.
- **On start:** off until turned on. When on, the device announces itself once at start and on each network change (no periodic beacon), answers discovery queries, and listens on port 53317 only on private networks (the OS network profile "private" or "home" on Windows, and a per-network allow list elsewhere). Public Wi-Fi defaults to "ask first". Shows a persistent but quiet menu bar or tray icon with the receive state.
- **Options:** receive mode (ask every time default; auto-accept from trusted devices; quick save: all devices for the next 10 minutes); save folder (Downloads/Panoply by default); sort into subfolders by sender (on); alias (device name); PIN (off); "Receive only while the app is open" (on for phones).
- **Preview:** an incoming request shows sender alias, type, file names, count and size, the check code, and Accept, Decline, Always accept from this device.
- **Output:** files to the save folder; name collisions get a suffix; images from phones are offered "Convert HEIC to JPG on arrival".
- **Result card:** "Received 3 files from Pixel 9 (45 MB) to Downloads/Panoply."
- **Edge cases:** path traversal in incoming file names (sanitized to the save folder, entries with `..` rejected); a flood of requests from one device (rate limited, auto-declined after 3 ignored); disk full (checked up front using the declared size, the sender is told plainly); the user not at the computer (a held request expires after 60 s); malicious file types (executables from untrusted senders show a warning; nothing is run); a sender spoofing an alias (identity is the fingerprint, shown next to the alias, and a changed fingerprint for a known alias triggers a warning).
- **Quality bar:** the fuzzed HTTP endpoints never write outside the save folder across the traversal test set; idle CPU 0 percent and battery impact negligible on phones through a measured idle drain test (target: no measurable difference over 8 hours against the app closed, reported); an accept click to first byte under 300 ms.
- **Engine:** the share stack, `sanitize-filename`, platform notification APIs.
- **Surfaces:** menu bar and tray, notification actions, CLI `panoply share receive --dir ~/Inbox`, MCP `share_inbox` (lists received files; no remote control of accept).

### Share by QR
- **Pattern:** A, instant transform (the output is a link on screen).
- **Budgets:** standard. The one-time server runs only while a share is active and closes when done. Zip is streamed on the fly with no temp copy.
- **Job:** "Give this file to a device that does not have the app."
- **Inputs:** files, folders, text; many.
- **On drop:** starts the one-time web server and shows a QR code and a short URL ("http://192.168.1.20:53318/x7Kp..."). Picks the correct LAN interface (skips VPN and virtual adapters) and warns when the computer has only a public or guest network.
- **Options:** direction (download from me, or "let them upload to me" with a QR for the upload page); protection (HTTP with token default, HTTPS with a certificate warning, optional PIN); expiry (10 minutes or first completion); zip many files on the fly (on; no temporary copy).
- **Preview:** the QR and URL, plus a live list of who connected (IP and browser) and progress.
- **Output:** the other device downloads in its browser; the upload mode saves into the receive folder.
- **Result card:** "iPhone downloaded 1 file (210 MB). Link closed."
- **Edge cases:** client isolation (the other device cannot reach the URL: the app probes by asking the browser to call back; if nothing connects in 20 s it suggests the hotspot step with a Wi-Fi QR); browsers that block HTTP to local addresses or warn on self-signed certificates (documented per browser, token page explains the warning); very large downloads (range requests so browsers resume); the page works with JavaScript disabled; the QR scanned by a stranger (token is single use, shows who connected, can be revoked instantly); IPv6 only networks.
- **Quality bar:** works from the stock camera apps of iOS and Android to a download with at most 3 taps; tested on current Safari, Chrome, Firefox and Samsung Internet; a 2 GB file downloads with resume after a dropped connection; the web page is under 20 KB with no external requests (verifiable). Compared with LocalSend's own web share and `python -m http.server` for setup steps, we count taps.
- **Engine:** `axum`, `qrcode` (QR generation), `async_zip` for streaming zips, `tower` rate limiting.
- **Surfaces:** bar ("share by qr"), right-click "Share with QR", CLI `panoply share qr big.zip` (prints a QR in the terminal), MCP `share_link` (returns the URL; the agent hands it to the user).

### Clipboard sync
- **Pattern:** G, always on.
- **Budgets:** exception: always on, event driven. It rides on the Clipboard history watcher (11.1), so it adds no wake-ups of its own. It holds no standing connection (each item is a short request to a paired device) and keeps no history in memory.
- **Job:** "Copy on my phone, paste on my laptop."
- **Inputs:** none; the clipboard.
- **On start:** off until turned on and until at least one device is paired by the check-code step (clipboard is more sensitive than files, so trust is required, not just discovery). Detects secrets (password manager sources, strings that look like keys and one-time codes) and does not send them unless allowed.
- **Options:** devices (a list, each toggled); content (text, links, images up to 10 MB default, files off); direction (both default; one way); sensitive items (never send, default); auto-paste vs "send on demand" (on demand on phones, because iOS restricts background clipboard reads); expiry on the receiving side (clear after 2 minutes for items flagged sensitive).
- **Preview:** a menu bar line "Synced from Pixel 9, 3 s ago" with the item, and a per-device pause.
- **Output:** the other device's clipboard, and an entry in the receiving Clipboard history marked with its source.
- **Result card:** n/a (a toast "Copied from Anzal's iPhone").
- **Edge cases:** iOS and Android restrictions (Android 10 and later block background clipboard reads, and iOS shows a paste prompt; the phone uses a share-sheet action and a keyboard or widget "Send clipboard" button, and says why); clipboard loops (an item echoed back is dropped by a content hash and origin id); large images (resized on request); rich text (plain text default, rich preserved Mac to Mac); password managers' concealed-type flag (honored on macOS `org.nspasteboard.ConcealedType` and Windows `ExcludeClipboardContentFromMonitorProcessing`); devices that leave the network.
- **Quality bar:** text appears on the other device in under 500 ms on the same Wi-Fi (target, p95 over 200 copies); zero synced items from sources flagged concealed or from password managers across a test run; no item leaves paired devices (packet capture test shows only TLS to peer IPs). Compared with Apple's Universal Clipboard and KDE Connect on feature set on the cross-platform pairs they do not cover.
- **Engine:** the share stack with the Panoply capability channel (ignored by stock LocalSend); `arboard` for clipboard access; platform clipboard listeners shared with Clipboard history (11.1). No standing connection: each item is a short request to the paired device.
- **Surfaces:** menu bar and tray toggle, phone widget and keyboard, CLI `panoply share clip send`, `panoply share clip get`, MCP `clipboard_sync_status` (read only; no remote clipboard read by agents).

### Phone scanner to desktop
- **Pattern:** D, guided capture (on the phone), with G on the desktop.
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback models (a small on-device document segmentation model, MobileNet class, target under 10 MB, and PaddleOCR OCR models) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. Camera preview follows the camera frame rate (30 to 60 fps).
- **Job:** "Scan a paper with my phone and have the PDF land on my computer."
- **Inputs:** the phone camera, or photos from the library.
- **On start:** phone side opens the camera with edge detection and a live page outline; the desktop name is a target chosen once ("Send to: MacBook"), the same trust as pairing. The page type is classified (document, receipt, ID card, whiteboard, book page) and picks the clean-up preset: black and white for text, color for receipts and IDs, shadow removal for books.
- **Options:** page count (multi-page, one tap per page); clean-up (auto; color; grayscale; black and white); OCR (on, text layer added; done on the desktop when the phone is slow); output (PDF default, JPG pages, DOCX through the converter); destination folder on the desktop; sort into a named folder by detected type (off).
- **Preview:** the cropped, deskewed page with a re-crop handle, live; sharpness and glare checks warn before the photo is accepted ("Too dark", "Blurry", "Page cut off").
- **Output:** a PDF on the desktop in the receive folder, named by the first line of text or "Scan 2026-10-13 15.04"; the phone keeps nothing unless asked.
- **Result card:** "4 pages, 520 KB, searchable. Sent to MacBook."
- **Edge cases:** curved book pages (dewarp); a page on a patterned table; low light (torch prompt); glossy glare (take-two-and-merge); desktop offline (queued on the phone, sent when found); mixed page sizes in one scan; handwriting (OCR flagged lower confidence).
- **Quality bar:** on 100 test pages photographed on phones in normal room light, edges detected within 2 percent of page width and OCR character accuracy at least 98 percent for printed text (target, measured and published), at the same or better quality than the built-in scanner of iOS Notes and Google Drive's scanner on the same photos.
- **Engine:** OS first, own model second. The scanner runs on phones, so the ladder is for iOS and Android. Document edges: iOS uses VisionKit `VNDocumentCameraViewController`, with Vision `VNDetectDocumentSegmentationRequest` for photos from the library. Android uses ML Kit Document Scanner (`GmsDocumentScanning`). OCR: iOS uses Apple Vision `VNRecognizeTextRequest` (and `RecognizeDocumentsRequest` on iOS 26). Android uses ML Kit Text Recognition v2. Fallback models (any platform where the OS engine is absent or fails the quality bar test in CI, including a desktop that scans library photos, where Windows has no OS engine and Linux uses the fallback): an on-device document corner model (a small MobileNet-class model trained or distilled with a permissive dataset, ONNX) plus classical quad detection, and a PaddleOCR-class model (Apache-2.0) via `ort`. Non-ML: OpenCV-style perspective warp through our `imageproc` code, the share stack.
- **Surfaces:** phone app (primary), phone share sheet and a Home Screen widget, desktop receive; CLI `panoply share scan --watch` (receives and files scans); MCP `scan_inbox`.

### Send text and links
- **Pattern:** A, instant transform.
- **Budgets:** standard.
- **Job:** "Send a link, address or snippet to my other device."
- **Inputs:** typed or pasted text, URLs, the page in the browser.
- **On drop:** detects the content type: a URL (title fetched from the page only if the user allows), an address or phone number (opens in Maps or the dialler on the receiver), a code or OTP (flagged sensitive, expires on the receiver), a long text (sent as a note file too). A URL for a file download offers "Send the file instead" only when the user chooses.
- **Options:** target (last used); open on arrival (links open in the default browser on the receiver, off by default for untrusted devices); expire (never; 5 minutes for sensitive).
- **Preview:** how it will look on the receiver.
- **Output:** a notification with the text and a Copy and Open action; history on the receiver.
- **Result card:** "Sent link to Pixel 9."
- **Edge cases:** very long text (over 1 MB goes as a file); non-text clipboard types; links that look like phishing from an untrusted sender (the receiver shows the full domain, never auto-opens); emoji and right-to-left text; receiver locked (the notification waits).
- **Quality bar:** under 500 ms from Return to a visible notification on the same network (target); text byte-identical on 1,000 random Unicode strings; against stock LocalSend, a text message arrives as a message (the protocol's `text/plain` preview field).
- **Engine:** the share stack's message type in `prepare-upload` (text as a file with a preview text, per the LocalSend v2 text handling), notifications by platform.
- **Surfaces:** bar ("send link"), browser share-sheet entry, phone share sheet, CLI `panoply share text "..." --to phone`, MCP `share_text`. Recommended: merge into Send to device (same code path, same UI).

### Borrow the desktop
- **Pattern:** G, always on (service on the desktop), A on the phone.
- **Budgets:** exception: opt-in service on the desktop. It rides on the Receive listener (0% idle, no extra process) and starts a module worker only when a job arrives. Jobs run under a CPU limit (default half the cores) and each module keeps its own budget.
- **Job:** "My phone cannot do this job. Let my computer do it and send the result back."
- **Inputs:** any job from any module that the phone judges too heavy.
- **On start:** off by default. The desktop must be paired and trusted and must opt in ("Let my paired devices use this computer for jobs", with a CPU limit and an "only when plugged in" switch). The phone host estimates each job (a long 4K video, a 500-page OCR, a large LibreOffice conversion, a big model) and when the local estimate exceeds a threshold of 30 s or the model is not installed, offers "Do this on MacBook" with the desktop's estimate.
- **Options:** the desktop to use (nearest by latency; a preference list); allowed modules (all; a safe list default of non-network modules); resource limit on the desktop (default half of cores); keep the files on the desktop (off; results are returned and the temporary copies removed); wake the desktop if it is asleep and Wake-on-LAN or Power Nap allow it.
- **Preview:** two lines: "On this phone: about 14 min and 40 percent battery. On MacBook: about 50 s. Transfer 120 MB."
- **Output:** the result lands where the phone job would have put it, with the same naming; the job appears in History on both devices.
- **Result card:** "Compressed on MacBook in 52 s. 640 MB to 88 MB. Returned to your phone."
- **Edge cases:** the desktop falls asleep or goes away mid-job (the phone resumes by finishing locally or waiting, chosen by the user); jobs that use files only on the phone (sent up first, shown in the estimate); a malicious paired device sending jobs (only modules on the allow list, each job runs in the module sandbox with no network, with input size limits and a per-device quota); version mismatch between phone and desktop modules (the desktop rejects with the version needed); Wi-Fi isolation (same ladder as Send to device).
- **Quality bar:** a job on the desktop returns a result byte-identical to running the same module there directly (test); end-to-end time for a 1 GB video compress phone to desktop to phone is limited by link speed plus desktop time, and under the phone's local time on a pair of mid-range phone and laptop (published per device); no job runs without a trust record and a non-expired pairing (tested).
- **Engine:** the share stack's Panoply capability channel (a `job` message with module id, options, input list), the module host with the same wasmtime sandbox on the desktop, resumable uploads, signed job envelopes (Ed25519 key pinned at pairing).
- **Surfaces:** phone result of any heavy module (a "Do this on MacBook" button), desktop service panel, CLI `panoply share serve` and `panoply share jobs`, MCP none (agents already run on the desktop).

### Temporary folder share
- **Pattern:** G, always on while active.
- **Budgets:** standard while active (idle memory under 50 MB). Nothing runs after Stop, and the port closes within 1 s.
- **Job:** "Share this folder on my network until I stop it."
- **Inputs:** one or more folders.
- **On drop:** shows the share options immediately and starts nothing until confirmed. Detects the folder size, file count and any hidden or system files (excluded by default), and sensitive-looking contents (keys, `.env`, `.ssh`) which are excluded and listed.
- **Options:** access (read only default; allow uploads; allow edit); who (any Panoply or LocalSend device I trust; or anyone with the link and PIN); duration (until I stop it default; a timer 1 h, 24 h); bandwidth limit (off); visible in a browser (on, a clean file list page with zip download for a folder).
- **Preview:** the URL, a QR, and a live panel of connected devices and files served.
- **Output:** the folder, untouched on disk; uploads (if enabled) go to a subfolder named "Uploads from others".
- **Result card:** "Shared 'Wedding photos' (4,210 files) for 38 min. 3 devices, 14 GB served. Stopped."
- **Edge cases:** the user moves, renames or deletes files while shared; symlinks outside the folder (not followed); path traversal in requests (rejected); client isolation (same ladder); computer sleeps (the share pauses and says so, keep-awake optional); a changed network address (the link updates and the QR refreshes); a browser asking for files above the shared root (not found, not forbidden, so the root is not leaked).
- **Quality bar:** zero reads outside the shared root over the traversal test set; zero stale shares after a stop (the port closes within 1 s, tested); streaming a 10 GB file with resume while idle memory stays under 50 MB (target); compared with `python -m http.server` and macOS File Sharing for setup steps and safety defaults.
- **Engine:** `axum`, `tower-http` `ServeDir` with our own path guard, the share stack's trust records.
- **Surfaces:** right-click on a folder "Share on network", bar ("share folder"), CLI `panoply share folder ~/Photos --read-only --for 2h`, MCP `share_folder` (requires user confirmation; returns the link).

### Send to many
- **Pattern:** A, instant transform.
- **Budgets:** standard.
- **Job:** "Send one file to several devices at once."
- **Inputs:** files, folders; devices selected from the list.
- **On drop:** device list with checkboxes; the previous group selection is restored ("Family laptops"); groups can be saved by name.
- **Options:** targets (a saved group default); simultaneous or sequential (simultaneous up to a measured limit, then queued); per-device conversion ("HEIC to JPG for Windows devices only", off); retry failed targets (on).
- **Preview:** per-device state and the total size multiplied by devices, with the shared Wi-Fi bandwidth estimate.
- **Output:** one transfer session per device, each with its own accept prompt on the receiving side.
- **Result card:** "Sent to 4 of 5 devices. Failed: Dad's laptop (declined)."
- **Edge cases:** one receiver slower than others (does not block the rest); a device that declines; mixed trust levels (untrusted receivers still prompt); the sender's upload bandwidth split across receivers (shown); a receiver leaving mid-way (resume if it returns within 5 minutes).
- **Quality bar:** a 1 GB file to 5 devices takes no more than the single-device time multiplied by 5 plus 10 percent in the worst case (the link is shared; measured) and in practice less on a wired uplink; 5 of 5 hashes equal.
- **Engine:** the share stack with parallel sessions and a bounded task pool (`tokio`), no multicast data path (the LocalSend protocol is unicast and stays compatible).
- **Surfaces:** panel, bar, CLI `panoply share send *.jpg --to group:family`, MCP `share_send_many`. Recommended: merge into Send to device as multi-select.

### Far away transfer
- **Pattern:** A, instant transform (opt-in internet, labelled).
- **Budgets:** exception: needs a relay server for the fallback path (internet, opt-in, labelled). Direct connection is tried first. Because it needs a server it is not in the default build and ships as a community module, as the review recommends.
- **Job:** "Send this to a device on another network, privately."
- **Inputs:** files, folders; a code, a link or a paired device.
- **On drop:** shows the privacy line before anything runs: "This uses the internet. Content is end-to-end encrypted. A relay carries bytes it cannot read." Tries a direct path first (hole punching), and falls back to the relay only if the direct path fails, and says which was used.
- **Options:** pairing (a short code phrase typed or scanned on the other side; a saved paired device for repeated use); expiry (24 h); relay (the project's default relay or a self-hosted URL); size limit (none; the relay is bandwidth capped fairly).
- **Preview:** the code phrase and a QR, a status line "Waiting for the other device", then "Direct connection" or "Via relay".
- **Output:** received like a normal Receive, into the folder.
- **Result card:** "Sent 800 MB via direct connection in 2 min 10 s. End-to-end encrypted."
- **Edge cases:** both devices behind strict NAT (relay used and said); the relay unreachable (a plain message; no hidden fallback to another service); the receiver's first-time code guesses (rate limited, code is single use with a password-authenticated key exchange so the relay never learns it); a trusted-device check for repeated sends; large transfers over metered networks (phones ask before using mobile data).
- **Quality bar:** the relay never sees plaintext (verified by a test where the relay is a recording proxy); a direct connection is used whenever it can be established, tracked per NAT type in a public test matrix; code guessing success negligible by construction (a documented PAKE with at least 40 bits of code entropy plus rate limits). Compared against Magic Wormhole and Send Anywhere on setup steps.
- **Engine:** `iroh` (Apache-2.0 and MIT, QUIC with hole punching and relays; relays are self-hostable) or a PAKE transfer on `snow`/`spake2`; the project runs a donated relay set, so this module is zero running cost to the project only if the relay is community-run. Decision pending.
- **Surfaces:** bar ("send far"), panel, CLI `panoply share far send file.zip`, `panoply share far receive <code>`, MCP `share_far` (always asks the user, since it uses the internet). Recommended: cut from v1, ship as a community module, because of relay running cost and abuse handling.

---

## 10. Capture

All capture modules use the Capture layer defined above: ScreenCaptureKit on
macOS (Screen and System Audio Recording permission, restart after grant),
Windows Graphics Capture on Windows (no prompt, elevated windows and DRM black
frames detected), and xdg-desktop-portal plus PipeWire on Wayland. Permission
denied is a guided flow with a Settings button, "Check again" and a no-permission
fallback, never a silent failure and never a black image. Each spec adds only
what is special.

### Screenshot
- **Pattern:** D, guided capture.
- **Budgets:** standard. The global shortcut listener is event driven (0% idle, resident under 10 MB). Freeze-frame holds one display frame while choosing, and scrolling capture stitches row strips to disk.
- **Job:** "Capture an area, a window, the screen or a scrolling page, now or on a timer."
- **Inputs:** none; the screen.
- **On start:** on first use runs the permission flow. Each capture starts from a global shortcut (area default; window; full screen; scrolling; timed). It detects the target: window capture reads the window bounds and shadow, a full-screen capture picks the display under the pointer, scrolling capture detects a scrollable region under the cursor.
- **Options:** mode (area, window, screen, scrolling, timed 3, 5, 10 s); include cursor (off); window shadow (on for macOS look, off for Windows); delay; destination (clipboard default plus file; a folder; the Capture history); format (PNG default, JPG or WebP when the user prefers smaller); scale (Retina at 2x kept; "downscale to 1x for sharing" toggle); freeze the screen before choosing (on).
- **Preview:** the live selection with pixel dimensions and a magnifier loupe; after capture a small thumbnail with Annotate, Copy, Save, Pin, Send to device.
- **Output:** "Screenshot 2026-10-13 at 15.04.22.png" in the chosen folder, plus clipboard.
- **Result card:** "1,280 x 760 px at 2x, 212 KB PNG, copied and saved."
- **Edge cases:** multiple displays with different scaling (capture is per display, stitched correctly across displays); HDR and wide colour displays (tone-mapped to sRGB with a note, or kept as display P3); DRM video and password fields (black regions detected and named, not hidden silently); scrolling capture on sticky headers and lazy loading (headers deduplicated by frame matching, stops at the end, caps at a page limit); menu and tooltip capture (a delay mode); rounded corners and window shadows; Wayland (portal picker for window choice, one prompt per session, restore token); macOS permission revoked mid-session (shows the guided flow, the pending capture is cancelled cleanly).
- **Quality bar:** hotkey to the selection overlay visible in under 50 ms (the VISION budget for the bar, same target here) and capture to clipboard in under 100 ms for a full 5K screen on an M-class chip; pixel-exact capture (hash equals a reference grab of the same frame); scrolling capture stitched with zero visible seams on 20 standard long pages (visual diff against the page printed to PDF). Benchmark against CleanShot X for feature parity on capture modes and ShareX for the Windows set, and against the OS tools (Command+Shift+5, Snipping Tool) on speed.
- **Engine:** ScreenCaptureKit, Windows Graphics Capture, PipeWire and portals, `image` and `png`/`oxipng`, our stitcher using feature matching on row strips.
- **Surfaces:** global shortcut, bar ("screenshot"), tray, phone (its own screenshots are searchable but capture is the OS), CLI `panoply capture shot --area --out shot.png`, MCP `screen_capture` (asks the user once per session, returns a file, never silent).

### Annotate
- **Pattern:** C, visual editor.
- **Budgets:** standard. The canvas is tiled for huge images and editing holds 120 fps. OS engines: no download, no resident memory for us. The fallback models (PaddleOCR about 15 MB per language, YuNet) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them.
- **Job:** "Mark up a screenshot with arrows, text, blur and numbered steps, fast."
- **Inputs:** an image, a screenshot straight from capture, a clipboard image, or a PDF page.
- **On drop:** detects text regions (OCR boxes) and sensitive patterns (emails, card numbers, API keys, faces) and offers "Blur sensitive info" as a one-click action with the detections previewed. Detects the dominant colour to choose a contrasting default annotation colour.
- **Options:** tools (arrow, rectangle, ellipse, line, text, highlight, pixelate and blur, numbered step, crop, spotlight); style presets (name and save); balloon text with tail; canvas padding and background (for tidy social images, off); beautify frame (browser or device frame, off); output scale.
- **Preview:** the canvas, live; blur is true pixel destruction in the exported file, and the preview marks it "destroyed on export".
- **Output:** "name (annotated).png"; layered project kept in History to reopen editing (a small `.pano` sidecar), flattened PNG exported.
- **Result card:** "Annotated: 3 arrows, 2 text, 1 blur. 188 KB."
- **Edge cases:** blur that can be reversed (a redaction is never a Gaussian blur over a recoverable low-radius text region; the tool uses pixelation with a minimum block size or a solid fill and re-checks that OCR cannot read the area); transparency; very large images (tiled canvas); undo history across a session; text in right-to-left and CJK; copy to clipboard with a transparent background.
- **Quality bar:** after Blur or Pixelate export, OCR run over the redacted region returns no characters across 200 test regions, and a test reversing common blur radii fails to recover text; editing stays at 120 fps on a 5K image; reaching the first arrow takes at most 2 keystrokes after capture. Compared with CleanShot X's annotation set and Shottr's speed on the same tasks (published step counts).
- **Engine:** OS first, own model second. Two ML jobs, one ladder each, both shared with other modules. Text regions and the redaction check use the OCR ladder of Screenshot search: macOS and iOS Apple Vision `VNRecognizeTextRequest`, Android ML Kit Text Recognition v2, Windows `Windows.Media.Ocr` (`OcrEngine`) and the Windows AI Text Recognition API on Copilot+ PCs, Linux the fallback, and a PaddleOCR-class model (Apache-2.0) as fallback on any platform where the OS engine is absent or fails the quality bar test in CI. Face detection for "Blur sensitive info": macOS and iOS Apple Vision `VNDetectFaceRectanglesRequest`, Android ML Kit Face Detection, Windows `Windows.Media.FaceAnalysis` (`FaceDetector`, detection only), Linux the fallback, and YuNet (MIT) as fallback. Non-ML: `tiny-skia` for vector rasterization, `cosmic-text` for text, our blur and pixelate kernels.
- **Surfaces:** opens automatically after Screenshot, panel, right-click on an image "Annotate", phone, CLI `panoply capture annotate in.png --blur-sensitive`, MCP `image_annotate_redact`.

### Copy text from screen
- **Pattern:** D, guided capture.
- **Budgets:** standard, plus OCR and table structure. OS engines: no download, no resident memory for us. The fallback models (PaddleOCR is about 15 MB per language, the small table-structure model targets under 100 MB) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. Barcode decoding with `rxing` is code, not a model.
- **Job:** "Select any text on screen, even in an image or a video, and copy it."
- **Inputs:** the screen, an image, a paused video frame.
- **On start:** a global shortcut shows an area picker. The captured area is classified: plain text, table, code, QR or barcode, math, handwriting; the output default follows: tables copy as tab separated cells that paste into Excel and Sheets correctly, code keeps indentation and monospace characters, QR content is decoded and offered as the answer.
- **Options:** language (device locale plus auto; several allowed); keep line breaks (off for paragraphs, on for code and lists); copy as (text; table; Markdown); translate result (off, opens Translate); auto-detect links and emails (on); keep the screenshot (off).
- **Preview:** the recognised text overlaid in place on the frozen area, selectable, with low-confidence words underlined.
- **Output:** text on the clipboard and a toast "Copied 38 words"; optional file save.
- **Result card:** "Copied 38 words, 94 percent confident, English."
- **Edge cases:** low contrast and tiny text (rescaled before recognition); text on video or paused frames; hyphenated line wraps (rejoined); mixed scripts; vertical Japanese; dark mode screenshots; protected content black frames (permission or DRM message); on phones, selection from the screenshot taken with the OS; macOS and Windows already offer Live Text and OCR in places, so the module wins where the OS tool does not reach (any app, any video frame, tables, code, QR).
- **Quality bar:** character accuracy at least 98 percent on clean UI text and at least 95 percent on a harder set of small, low-contrast and code screenshots (targets on our published 300 image set); table cell accuracy at least 95 percent; hotkey to copied text under 400 ms for a 1080p area with OS OCR on an M-class chip. Benchmark against macOS Live Text and the Windows Snipping Tool text actions on the same set.
- **Engine:** OS first, own model second. OCR: macOS and iOS use Apple Vision `VNRecognizeTextRequest`, and `RecognizeDocumentsRequest` on iOS and macOS 26 (paragraphs and tables). Android uses ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean). Windows uses `Windows.Media.Ocr` (`OcrEngine`), and the Windows AI Text Recognition API on Copilot+ PCs. Linux uses the fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): a PaddleOCR-class model (Apache-2.0) via `ort`, also used for tables where the OS engine returns no table structure. QR and barcodes: macOS and iOS use Vision `VNDetectBarcodesRequest` (and VisionKit `DataScannerViewController` on iOS), Android uses ML Kit Barcode Scanning, Windows has none found, Linux uses the fallback. Fallback: `rxing` (own barcode decoder, permissive). Table structure: OS engine: none beyond `RecognizeDocumentsRequest` on iOS and macOS 26. Own model elsewhere: an on-device small table-structure model.
- **Surfaces:** global shortcut, bar ("copy text from screen"), right-click on an image or video ("Copy text"), phone camera (live text over camera in the app), CLI `panoply capture text --area` or `panoply ocr in.png`, MCP `screen_text`.

### Pin to screen
- **Pattern:** G, always on while pinned.
- **Budgets:** standard. 0% idle CPU and under 10 MB per pin.
- **Job:** "Keep this image floating above everything while I work."
- **Inputs:** a screenshot, clipboard image, image file, or a screen area.
- **On start:** from a capture, a "Pin" button creates a borderless always-on-top window at the original size and position. Detects whether the content has transparency and sets click-through accordingly.
- **Options:** opacity (100 default, scroll to change); click-through (off; shortcut toggles); scale (wheel zoom); lock position; shadow; keep across spaces and full screen apps (on); a pinned item may be a live region that updates (off; a lighter "live preview" mode for a window region).
- **Preview:** the floating window itself.
- **Output:** none; the pinned image can be saved, copied or annotated; unpinning leaves nothing.
- **Result card:** n/a.
- **Edge cases:** full screen apps and other Spaces (an overlay level on macOS, topmost style on Windows, layer-shell where Wayland allows and otherwise a normal window with a "keep on top" request that some compositors ignore, which is stated); high DPI; many pins; screen sharing (a "hide from screen share" toggle marks the pinned window excluded where the OS supports it); the app quits (pins can be saved and restored).
- **Quality bar:** a pin appears at the exact pixel position of the original capture (zero offset in a pixel test on 1x and 2x displays); stays on top over full screen apps on macOS and Windows in a 20-app test matrix; 0 percent idle CPU and under 10 MB per pin. Compared with Shottr and CleanShot X pinning on the same matrix.
- **Engine:** native window APIs (`NSPanel` level, `HWND_TOPMOST`, layer-shell where available), `winit`/platform UI per VISION architecture.
- **Surfaces:** from Screenshot ("Pin"), right-click on an image "Pin to screen", bar ("pin"), no CLI (it is a window), MCP none. Recommended: fold into Screenshot as an action.

### Screen recording
- **Pattern:** D, guided capture.
- **Budgets:** standard for the UI and encoder (target under 10 percent of one core at 1080p30 with hardware encoding). Capture frame rate follows the chosen setting (30 or 60 fps), not 120. Fragmented MP4 is written to disk, so memory is constant for any length.
- **Job:** "Record my screen with sound and my face, and show my clicks."
- **Inputs:** none; the screen, a window or an area; microphone, system audio, webcam.
- **On start:** runs the permission flow for screen, plus microphone and camera only when those sources are on, each asked at the moment of choosing, never up front. Detects the display resolution and refresh rate, available encoders (VideoToolbox, Media Foundation, VA-API, MediaCodec; see VISION) and system audio support (native on macOS 13+ through ScreenCaptureKit, WASAPI loopback on Windows, PipeWire monitor on Linux). Picks resolution and frame rate from the target: 30 fps for a tutorial, 60 fps for a game, and warns when audio loopback is unavailable (older macOS needs a virtual audio driver, which we do not install).
- **Options:** source (area, window, display); audio (system, microphone, both; each with a level meter); webcam bubble (off; position, shape, size); show clicks and keystrokes (on for clicks, off for keys); cursor (highlight on); resolution (native default, or scaled to 1080p); frame rate (30 default); quality (auto bitrate); hide notifications and desktop icons while recording (on, restored after); countdown (3 s); max length and storage guard (stops with a plain message when free space falls under 2 GB).
- **Preview:** a floating control bar and a live timer with the file size growing and a mic level; after stop, a trim bar for start and end.
- **Output:** MP4 (H.264 default, HEVC for smaller where the target supports it), audio on separate tracks when system and mic are both on (so they can be mixed later), in the chosen folder; webcam optionally recorded as its own track.
- **Result card:** "5:12, 1920 x 1080 at 30 fps, 62 MB, system audio and mic on separate tracks."
- **Edge cases:** permission revoked mid-recording (recording finalized up to that frame, never corrupted); a crash or power loss (fragmented MP4 written, so everything up to the last seconds is playable, recovery on next launch); audio and video drift on long takes (timestamps from one clock, drift under 1 frame over 60 minutes); variable refresh rates and HDR (tone-mapped); display changes and sleeping; DRM content (black, reported); battery drain and thermal throttling (hardware encoder, warns when dropping frames); Wayland (portal prompt, PipeWire frames, window selection through the portal; no global click overlay in some compositors, said); microphone devices that disappear.
- **Quality bar:** dropped frames under 0.5 percent at 1080p30 and 1440p60 over a 30-minute recording on reference machines (target, measured); CPU use under 10 percent of one core with hardware encoding at 1080p30 (target); audio and video drift under 40 ms at 60 minutes; the file plays in QuickTime, Windows Media Player, VLC and browsers; a crash-recovery test kills the process at 50 random times and recovers the file every time. Compared head to head with CleanShot X on recording features and CPU, ShareX on Windows, and OBS Studio on frame drops and CPU for the same settings.
- **Engine:** ScreenCaptureKit and the hardware encoder via our media crate, Windows Graphics Capture with Media Foundation, PipeWire with VA-API, or the OpenH264 software encoder (BSD, Cisco binary) when no hardware encoder exists. Never x264 (GPL), `cpal` for audio, `mp4` fragmented muxer.
- **Surfaces:** global shortcut, tray and bar ("record screen"), phone (screen recording via the OS, Panoply trims and shares), CLI `panoply capture record --window "Safari" --audio system --out demo.mp4`, MCP `screen_record_start`, `screen_record_stop` (always asks the user and shows the OS recording indicator).

### Record to GIF
- **Pattern:** B, tuned transform (record, then tune).
- **Budgets:** standard. Capture as Screen recording. GIF encoding streams in two passes and never holds the clip.
- **Job:** "Record a short clip and get a small GIF I can paste into a message or an issue."
- **Inputs:** none; or an existing screen recording to convert.
- **On start:** same as Screen recording for permissions and the area picker. The default is an area recording capped at 30 s. A dropped video is measured (length, size, frame rate, scene change rate) to pick width and frame rate so the GIF lands near a target size.
- **Options:** width (800 default; the chip row 480, 640, 800, 1280); frame rate (15 default; up to 30); length cap (30 s); target size (under 8 MB for chat apps and under 10 MB for GitHub, as presets; default "under 8 MB"); loop (on); palette (adaptive, with dithering chosen by content); alternative output (animated WebP and MP4 offered when smaller, because they are better than GIF on size).
- **Preview:** live preview of the GIF with its estimated size and an in-place trim and speed control.
- **Output:** "name.gif" in the chosen folder and clipboard (as a file).
- **Result card:** "6.2 s, 800 x 450, 15 fps, 3.1 MB. Animated WebP would be 0.9 MB."
- **Edge cases:** large flat color UI (small GIFs; photographic content (large GIFs, warned and an alternative offered)); transparency; cursors and motion blur; very long clips (capped, with an "extend" choice); GIF size limits of a platform (presets); 256-colour banding in gradients (error diffusion option).
- **Quality bar:** hits the target size within 10 percent on 50 test clips; visual quality at equal size judged by SSIM against Gifski and ffmpeg's palette method on the same clips (we match or beat); encode time for 10 s of 800 px under 2 s on an M-class chip. Compared with Kap and ScreenToGif.
- **Engine:** the capture layer, our own NeuQuant-style quantizer with the `gif` crate, and `libwebp` through `webp` for animated WebP. `gifski` is the quality benchmark only (AGPL-3.0, never linked); license-check any quantizer before shipping.
- **Surfaces:** shortcut, bar ("record gif"), right-click on a video "Make GIF" (shared with Video tools), CLI `panoply capture gif --area --max 8mb`, MCP `screen_gif`. Recommended: fold into Screen recording as an output choice.

### Color picker and ruler
- **Pattern:** F, instant answer (and a small tool for measurements).
- **Budgets:** standard. Hotkey to loupe under 50 ms. The loupe samples one screen frame.
- **Job:** "Pick any color on screen and measure pixels."
- **Inputs:** none; the screen.
- **On start:** a global shortcut shows a magnifier loupe at the pointer. Reads the pixel from a screen capture, so it needs the same Screen Recording permission on macOS. Detects display profile and picks colors in the display's profile, converting to sRGB for web values.
- **Options:** formats shown (HEX, RGB, HSL, OKLCH, Display P3, CSS variable; the last used copied on click); palette history (last 50, savable as named palettes); contrast checker (WCAG ratio between two picked colors, on); ruler tools (distance between two points, rectangle, edge-snapped measurement with a detected edge, screen angle); units (pixels and points, with the display scale shown).
- **Preview:** the loupe with the pixel grid, the current value and a swatch; the ruler shows dimensions live and snaps to edges.
- **Output:** the value on the clipboard (Return) and in History; palette export to CSS, JSON or an ASE file.
- **Result card:** "#3A7BD5, rgb(58,123,213), oklch(0.58 0.14 255). Contrast against white 4.2:1, fails AA for body text."
- **Edge cases:** colour-managed displays (value reported with the profile, never claimed as the "true" colour of the content); HDR and night shift (Night Shift and f.lux are not part of the picked value, said once); gradients and video overlays (sampled average over a chosen 1 to 9 pixel area); Wayland without screen read (portal screenshot used for the picker, so it shows the portal prompt, and says so); scaled displays (1 logical pixel equals 2 device pixels reported both ways).
- **Quality bar:** the picked value equals the pixel in the captured image exactly across 1,000 random points on 1x and 2x displays; hotkey to loupe in under 50 ms; ruler measurement exact to 1 device pixel on a calibrated test image. Compared with the macOS Digital Color Meter, PowerToys Color Picker and Pixie on feature set and speed.
- **Engine:** the capture layer, `palette` crate for color spaces, `lcms2` for profiles, our edge detector.
- **Surfaces:** global shortcut, bar inline ("pick color", then Return copies HEX), CLI `panoply capture color --at 400,300`, MCP `screen_color` (asks once per session).

### Capture history
- **Pattern:** H, find (backed by G).
- **Budgets:** exception: bounded background work. OCR of new captures runs when charging or on idle, one at a time. Optional speech text is off by default. OS engines: no download, no resident memory for us. The fallback models (PaddleOCR, whisper.cpp) download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. Items cost under 2 KB each plus OCR text, kept in the Index and not in memory.
- **Job:** "Find any screenshot or recording I made, by the words in it."
- **Inputs:** a query; none for the browse view.
- **On start:** on by default for captures made through Panoply; adopting old screenshots in the OS screenshot folders is an offer, not automatic. Every capture is stored with its time, source app, window title, display, and OCR text (screenshots, run when charging or at idle if on battery) and speech text for recordings (off by default). Nothing is stored for screen recording frames beyond the video. A "Pause capture history" switch and "Exclude windows of these apps" list (password managers, banking) exist; excluded apps' windows are never recorded in history even if captured.
- **Options:** retention (forever default for Panoply-made captures; 30, 90 days; keep starred); storage location (inside the app data, or a folder); OCR (on); thumbnails (on); sync across your devices (off; over the local network when on, using the same share stack and never a server); search text and filters from the Panoply Index parser (app, date, kind).
- **Preview:** a grid of thumbnails with the matched words boxed, a timeline, and filters as chips.
- **Output:** list; actions Copy, Annotate, Pin, Send, Reveal, Delete, Redact then share (the redaction check from Annotate).
- **Result card:** n/a.
- **Edge cases:** screenshots with sensitive content (a pattern flag, hidden in the grid until revealed, and excluded from sync by default); huge histories (thumbnails and vectors stay under 2 KB per item plus the OCR text); deleting an item removes it from the index and Trash'es the file; storage growth (a size meter and a "clean up large recordings" action); imports from CleanShot X, Shottr and ShareX folders (by folder, as plain files).
- **Quality bar:** a text query returns results in under 16 ms on 50,000 captures (target); 95 percent of known-item text queries find the capture in the top 3 on our 500-screenshot set (shared with Screenshot search); zero entries for excluded apps across a test where 100 captures are made from them. Compared with macOS Spotlight image text search for recall on the same set and with the Windows Recall concept on privacy: our version is opt-in, local, per capture, with no continuous screen recording.
- **Engine:** the Panoply Index. OS first, own model second. OCR uses the ladder of Screenshot search: macOS and iOS Apple Vision `VNRecognizeTextRequest`, Android ML Kit Text Recognition v2, Windows `Windows.Media.Ocr` (`OcrEngine`) and the Windows AI Text Recognition API on Copilot+ PCs, Linux the fallback, and a PaddleOCR-class model (Apache-2.0) as fallback where the OS engine is absent or fails the quality bar test in CI. Optional speech text: macOS and iOS Apple `SpeechAnalyzer` with `SpeechTranscriber` (macOS and iOS 26), Android `SpeechRecognizer.createOnDeviceSpeechRecognizer` and ML Kit GenAI Speech Recognition (alpha), Windows AI Speech Recognition (`SpeechRecognitionModel`, experimental, not production yet) and `Windows.Media.SpeechRecognition`, Linux the fallback, and `whisper.cpp` (MIT) as fallback.
- **Surfaces:** bar ("capture history" or "screenshot wifi"), panel, CLI `panoply capture history --query "order id"`, MCP `capture_history_search` (returns metadata and snippets; image bytes only by user approval). Recommended: merge with Screenshot search (one index, one UI).

---

## Cross-cutting recommendations

Counts: section 6 has 16 modules, section 7 has 12, section 8 has 13, section 9
has 10 and section 10 has 8, 59 in total.

**Merge (one module, several modes or facets)**
- Screenshot search + Photo search by meaning + Capture history into one "Search images and captures": one index, one grid, text and meaning modes. Keep Capture history as the setting for Panoply-made captures.
- Instant file search + Search inside files + Metadata search + Recent everything: one find module with operators and an empty-query state. They share one index and one parser, and users do not choose between them.
- Similar images + Duplicate finder into one "Duplicates" with an exact mode and a visual mode; they share the keep rule and cleanup.
- Send to device + Send text and links + Send to many into one send flow (multi-select, text as a message).
- Archives + Split and join (split archives exist). Rename to convert into Universal converter as a toggle. Pin to screen and Record to GIF into Screenshot and Screen recording as actions and output choices. Metadata for any file into the shared strip engine of Images and Privacy.
- Search unplugged drives into the Index as the "catalog this drive" setting plus a results filter.

**Split**
- Office to and from PDF into two modules: PDF to Office (our own layout recovery, the hardest part) and Office to PDF (a LibreOffice wrapper). Different engines, different quality bars, different failure modes.
- Ebook convert into "to EPUB" (stable, pandoc plus our code) and "AZW3 and MOBI output" (our own KF8 writer, a risky component).
- Email and contact files into mail (EML, MBOX, MSG, PST) and calendar and contacts (ICS, VCF); the users and outputs differ.

**Cut or defer**
- Far away transfer: defer to a community module. It costs money to run relays, needs abuse handling, and conflicts with "nothing leaves your device" as a default.
- Shred: keep only with the honest SSD behaviour, or cut. A tool that cannot guarantee deletion on SSDs and copy-on-write file systems risks giving false assurance; a "erase this drive's encryption key" guide may serve users better.
- Search inside videos: ship after the images index, since it needs hours of background processing and it is the heaviest on battery.
- 3D files STEP support: download as an optional OpenCASCADE helper, later; STL repair is the v1 part.
- Translate and Summarize on phones: via Borrow the desktop only.

**Licensing watch list (verify at pin time).** pandoc is GPL-2.0 (run as a
separate process, never linked). `gifski` is AGPL-3.0 (do not link; vision rule
forbids AGPL in the core). `unrar` has its own restrictive license (read only,
downloadable plugin). Opus-MT models are CC-BY-4.0 (attribution). Calibre,
ExifTool, ripgrep, `fclones` and 7-Zip are benchmarks and test oracles only.
