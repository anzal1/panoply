# Panoply PDF module specs

Scope: section "1. PDF" of MODULES.md. Template and shared capabilities are in
FLOWS.md; nothing here repeats batch, presets, chains, result card actions, undo,
history, progress or plain errors.

Conventions used below:

- **pdfium** (BSD): render, text and glyph geometry, form fill, page objects.
  **lopdf** (MIT): object and stream level edits. **own**: Rust code in the core.
- No AGPL engine is used anywhere (no MuPDF, no Ghostscript, no Stirling or
  BentoPDF code). BentoPDF and Stirling-PDF are quality references only.
- "The corpus" means `research/corpus/pdf`, the shared test set of real ugly
  PDFs (scans, signed, encrypted, forms, RTL, huge, broken) that CI runs every
  module against. Counts in quality bars are targets we commit to, not measured
  results. Nothing here claims a measured competitor figure.
- Output name is always "name (suffix).pdf" next to the original unless stated.
- **Memory model.** lopdf is used through a lazy loader: the file is
  memory-mapped, only the cross-reference table is read up front, objects are
  parsed on demand, and untouched streams are copied byte for byte to the
  output. A whole file is never loaded. pdfium renders in bands, never a bitmap
  over 64 MB. Peak memory for any job is the page tree plus the objects of the
  pages in flight, so a 2 GB PDF stays inside the 300 MB cap.
- **Budgets.** Every module has a `Budgets:` line after its Pattern line. It
  says "standard" when the module fits the table in FLOWS.md, otherwise the
  exception and the reason. Models follow the Models budget: load on first use
  in a job, unload after 60 s idle, size shown before the first download.
- Every module that rewrites a file checks first for a signature and for an
  encryption dictionary, and says so on drop. Defaults below assume that check.

---

## A. Assemble and reshape

### 1. Merge (includes Alternate merge)
- **Pattern:** A. **Inputs:** two or more PDFs, plus images and Office files (routed through Convert in).
- **Budgets:** standard.
- **Job:** "Put these files in one PDF." Alternate merge: "Interleave my front scans and back scans."
- **On drop:** reads page counts, sizes, bookmarks, forms, encryption of every file. Sorts by natural filename order (file2 before file10), or by modified time if names are not sequential. If exactly two files have page counts equal (or differing by 1) and both look scanned, offers "Interleave" as the top button (second file reversed if it is a back-side scan in reverse order, detected by page-number OCR or the user flipping a switch).
- **Options:** Order (list, drag): natural name order. Bookmarks: one per source file, on. Mixed page sizes: Keep (default); Fit all to the first page's size. Forms: keep fields, renaming duplicates ("name_2"). Interleave mode: Reverse the second file, off unless detected.
- **Preview:** thumbnail strip of the final page order with source colour bands and total page count.
- **Output:** "first-name (merged).pdf".
- **Result card:** "5 files, 148 pages, 12.3 MB" and how many bookmarks and fields were kept.
- **Edge cases:** encrypted inputs (ask per file, output is unencrypted unless Keep encryption is on); signed inputs (warn, merged copy loses signatures, original untouched); duplicate form field names (rename, merge never silently shares values); mixed page sizes and rotations (kept per page, never stretched); file with 5,000 pages (stream, no full load); broken inputs (run Repair silently, flag in the card); an odd page count in interleave (last page stands alone, say so); shared fonts and images deduplicated so output is not the sum of inputs.
- **Quality bar:** output is never larger than the sum of inputs, and smaller when fonts and images repeat; 100 files, 2,000 total pages merge within 1.5x of qpdf wall time; opens without repair warnings in Acrobat and pdfium on every corpus combination. Must beat iLovePDF and Smallpdf on form and bookmark preservation, scored on the same corpus.
- **Engine:** lopdf object merge with own font and image dedup, own name-tree and outline merging.
- **Surfaces:** bar, panel, right-click "Merge PDFs" on a multi-select, CLI `panoply pdf merge a.pdf b.pdf -o out.pdf` (`--interleave`), MCP `pdf_merge`.

### 2. Split
- **Pattern:** A, with a short mode switch. **Inputs:** one PDF.
- **Budgets:** standard.
- **Job:** "Cut this PDF into parts."
- **On drop:** reads page count, outline depth, size, and finds separator candidates (blank pages, barcode or QR sheets, repeated header text). Picks the default mode: if the file has top-level bookmarks, "By bookmark"; else if over 20 MB, "By size, 10 MB"; else "Every page".
- **Options:** Mode: Ranges ("1-3, 7, 9-"), Every N pages, By bookmark (level 1 default), By size (target MB), At blank pages. Name pattern: "{name} {n}" or bookmark title, sanitized for filesystems. Keep bookmarks inside each part: on.
- **Preview:** list of resulting files with page ranges and estimated sizes. Page ranges accept "last", "odd", "even", reverse ranges.
- **Output:** a folder "name (split)" with one file per part, optionally a ZIP.
- **Result card:** "12 files, 6.1 MB largest, 0 empty."
- **Edge cases:** one page larger than the size target (it stays alone, flagged); out-of-range pages (clamped, listed); shared resources (fonts and images written only into the parts that use them, so parts sum to about the original); bookmarks that point to the same page (merged into one part, said); form fields that span parts (kept where the widget lives); signed PDFs (warn, signature cannot survive); encrypted (password once, output keeps the same permissions).
- **Quality bar:** every part opens clean in pdfium and Acrobat across the corpus; by-size split lands every part at or under target except unsplittable pages; within 1.5x of qpdf speed. Must match iLovePDF and Smallpdf on ranges, and add by-bookmark naming and by-size parts in one flow (check each competitor's current feature set before launch).
- **Engine:** lopdf with own resource pruning.
- **Surfaces:** bar, panel, right-click "Split PDF", CLI `panoply pdf split in.pdf --every 10` (also `--ranges`, `--bookmarks`, `--max-size 10mb`), MCP `pdf_split`.

### 3. Smart split
- **Pattern:** B. **Inputs:** one scanned PDF, typically a batch scanned in one go.
- **Budgets:** standard, plus a small page-embedding model (OS engine: none, so it is used on every platform; target under 50 MB, size shown before the first download), loaded on first use and unloaded after 60 s idle. Thumbnails are rendered in batches of 32 pages, never all at once. OS engines: no download, no resident memory for us. The OCR fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Job:** "I scanned a pile of documents as one file; separate them."
- **On drop:** renders thumbnails in batches of 32 pages, runs a small page-classifier model (layout embedding) and looks for separator sheets, barcodes, blank pages, page-number resets ("Page 1 of N"), repeated letterheads. Chooses the strongest signal found and shows which one it used.
- **Options:** Boundary signal: Auto (default), Separator sheets, Blank pages, Barcode or QR, "Page 1 of N" resets, Letterhead change. Sensitivity: slider, default from how clean the signal is. Drop the separator pages: on.
- **Preview:** the scan as a strip with proposed cut lines the user can drag, add or delete, and a title guess for each document from its first-page text.
- **Output:** one PDF per detected document in a folder, named by guessed title or "Document 1".
- **Result card:** "47 pages into 9 documents, 6 cut lines from page-number resets, 3 from blank pages."
- **Edge cases:** letters with a continuation page (page-number signal overrides letterhead); double-sided scans with blank backs (blank pages that are backs of real pages are kept, detected by pairing); mixed A4 and receipts (size change is a weak signal only); handwritten pages (no text signal, falls back to visual similarity and says so).
- **Quality bar:** on a labelled test batch of at least 30 scanned bundles, boundary F1 of at least 0.9 for separator and barcode modes, at least 0.8 for Auto on unmarked stacks. Adobe Acrobat's split by top-level bookmarks is the nearest comparison and needs bookmarks the scan does not have. Wrong cuts must be one drag to fix.
- **Engine:** OS first, own model second. Page embedding: OS engine: none. Own model on every platform. Small page-embedding model, downloaded once. OCR for page-number text: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback. Barcodes on separator sheets: macOS and iOS: Apple Vision VNDetectBarcodesRequest. Android: ML Kit Barcode Scanning. Windows: none found. Linux: fallback. Fallback (any platform where the OS engine is absent or fails the quality bar test in CI): own barcode decoder. Also pdfium render, own boundary scoring.
- **Surfaces:** panel with the cut-line editor, right-click "Split scanned batch", CLI `panoply pdf smart-split in.pdf --signal auto`, MCP `pdf_smart_split`.

### 4. Organize pages (includes Reverse, Extract pages, Rotate)
- **Pattern:** C, visual grid. Reverse, Extract and Rotate are one-click actions on the same grid and also exist as direct commands (A, instant).
- **Budgets:** standard. The grid is virtualized and the thumbnail cache is capped at 64 MB.
- **Job:** "Reorder, delete, rotate or pull out pages."
- **On drop:** renders thumbnails lazily, detects pages whose rotation flag disagrees with their text direction (sideways scans) and pre-marks them as "needs rotation" with a one-click fix all; detects blank and duplicate pages and badges them.
- **Options:** none up front. In the grid: drag, multi-select, delete, duplicate, insert blank (size from neighbours), insert pages from another PDF, rotate 90, 180, reverse selection. Rotate command: Angle 90 clockwise by default, Auto (uses text orientation detection) on scans. Extract: ranges as in Split.
- **Preview:** the live grid is the preview; zoom a page to 200%; page count and estimated size shown.
- **Output:** "name (edited).pdf", "name (rotated).pdf", "name (reversed).pdf", "name (pages 3-7).pdf".
- **Result card:** "12 pages to 9. 2 deleted, 1 rotated, 3 moved."
- **Edge cases:** pages with inherited rotation and a non-zero /Rotate plus a content-stream rotation (we compute the effective angle, never double rotate); 3,000-page files (virtualized grid, thumbnails on demand); mixed page sizes (shown to scale); rotating a page that carries annotations or form widgets (they rotate with it); bookmarks of deleted pages (removed, or retargeted to the next page, chosen in a small prompt).
- **Quality bar:** grid scrolls at 120 fps on a 1,000-page file; first thumbnails under 300 ms; rotation correct on 100% of corpus pages with known orientation; no text or annotation drift after a rotate and rotate-back round trip (byte-for-byte content equality on the content stream). Must match iLovePDF and Smallpdf organize on every action, with no page cap.
- **Engine:** pdfium thumbnails, lopdf page tree edits, own orientation detector (text-line angle).
- **Surfaces:** panel and bar, right-click "Rotate", "Extract pages", "Reverse order", CLI `panoply pdf pages in.pdf --keep 1-5,9 --rotate 90 --reverse`, MCP `pdf_organize` (also `pdf_rotate`, `pdf_extract_pages`, `pdf_reverse`).

### 5. Overlay
- **Pattern:** B. **Inputs:** a base PDF and an overlay PDF (letterhead, template, background, form).
- **Budgets:** standard.
- **Job:** "Put this letterhead behind my document" or "Stamp this page over every page."
- **On drop:** with two files dropped, decides which is the base: the longer one is the base; a one-page file is the overlay. Reads both page sizes and recommends scale mode.
- **Options:** Position: Behind (default when the overlay has a white background, detected by rendering) or In front (default for transparent overlays). Pages: all (default), first only, odd or even. Overlay pages: repeat the single page (default for one-page overlays) or pair page by page. Size: Fit to base page (default), original, scale. Opacity 100%.
- **Preview:** page 1 of the result at 200%, with a toggle between behind and in front.
- **Output:** "base (overlay).pdf".
- **Result card:** "Overlaid on 24 pages, letterhead scaled 97% to fit."
- **Edge cases:** different page sizes or orientations (scale to fit, centre, never crop); base page that is scanned and opaque (warn that Behind will be hidden, switch to In front with Multiply blend); overlay carrying fonts that clash with the base (resources renamed, never merged by name); a base with a signature (warn); overlay with its own form fields (flattened by default, option to keep).
- **Quality bar:** result renders the same as a pdfium composite of the two pages within SSIM 0.99 on the corpus; no font substitution in either layer. Must match PDF24 and Stirling-PDF overlay output, with the mode picked automatically.
- **Engine:** lopdf form XObject wrap of the overlay page, own content-stream ordering, pdfium for the preview.
- **Surfaces:** bar, panel, right-click on two selected files, CLI `panoply pdf overlay base.pdf letterhead.pdf --behind`, MCP `pdf_overlay`.

### 6. Divide pages
- **Pattern:** B. **Inputs:** one PDF, usually scanned book spreads or two-up pages.
- **Budgets:** standard.
- **Job:** "Each page holds two pages side by side; cut them apart."
- **On drop:** detects spreads: pages much wider than tall, a central gutter (vertical low-ink band) and two text blocks. Proposes the cut position at the gutter, per page, and the direction (vertical cut by default; horizontal if pages are tall with a central gap).
- **Options:** Direction: Vertical, Horizontal (auto). Cut at: Detected gutter (default) or a fixed percent, 50% fallback. Reading order: left then right (default), or right then left (set automatically if the text is right-to-left). Trim gutter shadow: off.
- **Preview:** first spread with the cut line, draggable, and a page 2 and last page check since gutters drift.
- **Output:** "name (divided).pdf" with twice the pages; pages share the original images, cropped by a clip, so size barely grows.
- **Result card:** "60 spreads into 120 pages, gutter detected on 58, fixed at 50% on 2."
- **Edge cases:** gutter drifts across pages (per-page detection, smoothed); the first page being a single cover (not split, detected by width); right-to-left books such as Arabic and Urdu (order reversed); text in the gutter (cut at the least-ink column); crop boxes already set (respected, rewritten).
- **Quality bar:** cut lands within 2 mm of the gutter on at least 95% of corpus spreads; output size within 5% of the input (no re-encode). Must beat a fixed 50% cut (the naive baseline) on gutter accuracy.
- **Engine:** pdfium render for gutter profile, own crop-box and clip writer in lopdf.
- **Surfaces:** panel, right-click "Divide pages", CLI `panoply pdf divide in.pdf --direction auto`, MCP `pdf_divide_pages`.

### 7. Page layouts (Combine pages, N-up, Booklet, Poster)
- **Pattern:** B. **Inputs:** one PDF.
- **Budgets:** standard.
- **Job:** "Fit several pages on a sheet", "make a booklet", "print this big page across several sheets".
- **On drop:** reads page size and count, the device's default paper (A4 or Letter by region) and whether the printer is duplex-capable. Default layout: 2-up if text is large, 4-up if the pages are mostly small-type. Booklet is offered first when the page count is over 8 and the source looks like a document.
- **Options:** Layout: 2-up, 4-up, 6-up, 9-up, custom grid, Booklet (saddle stitch), Poster (tile count). Sheet size: region default. Margins and gap: 5 mm. Order: across or down. Borders: off. Booklet: pad with blanks to a multiple of 4 (automatic), binding side (left, right for RTL text).
- **Preview:** the first sheet and the sheet count; for booklet, the imposed sheet order front and back.
- **Output:** "name (4-up).pdf", "name (booklet).pdf", "name (poster).pdf".
- **Result card:** "40 pages onto 10 sheets, scaled to 70%."
- **Edge cases:** mixed page sizes (each scaled to its cell, centred); right-to-left booklets (binding flips); poster with overlap for gluing (overlap 10 mm default) and trim marks; pages with annotations (kept as live annotations); scale falling below a readable size (warn under 6 pt effective).
- **Quality bar:** booklet imposition prints in correct order on a 20-page and 22-page test (verified by page-number OCR on the imposed output); vector content stays vector (no rasterising). Must match PDF24 booklet and Adobe Acrobat booklet printing output order.
- **Engine:** lopdf form XObjects with transforms, own imposition code.
- **Surfaces:** bar, panel, right-click "Print layout", CLI `panoply pdf layout in.pdf --nup 4` (also `--booklet`, `--poster 2x2`), MCP `pdf_layout`.

## B. Clean up, shrink, repair, read

### 8. Clean scans (includes Straighten by any angle)
- **Pattern:** B. **Inputs:** one or many scanned or photographed PDFs.
- **Budgets:** standard, except that peak memory scales with scan size, capped at 300 MB by processing each page in bands. The stamp-versus-blank classifier is a tiny model (target under 5 MB). It has no OS engine, so it loads on first use on every platform and unloads after 60 s idle.
- **Job:** "Remove blank pages and straighten my crooked scans."
- **On drop:** classifies each page as born-digital or scanned (image covers the page, no real text), measures skew angle per page, finds blank pages (ink coverage below threshold, ignoring scanner edge noise and punch holes), and finds dark borders. If the file is not a scan, says so and offers nothing destructive.
- **Options:** Remove blank pages: on. Blank threshold: from the file's noise floor, not a fixed number. Keep pages that have only a stamp or a page number: off (listed in the preview so the user can rescue). Deskew: on for skew over 0.3 degrees, applies only to scanned pages. Manual angle: a dial from minus 15 to plus 15 degrees for one page or all (this is "Straighten by any angle"). Crop dark borders: on.
- **Preview:** pages flagged blank as a strip with a checkbox each, and one skewed page before and after at 200% with the detected angle.
- **Output:** "name (cleaned).pdf". Deskew resamples the page image once, never twice.
- **Result card:** "4 blank pages removed, 17 pages straightened (largest 2.1 degrees), 3.2 MB to 3.0 MB."
- **Edge cases:** intentionally blank pages ("this page left blank" has text, kept); near-blank pages with a faint pencil mark (kept, flagged); back sides of double-sided scans (blank backs removed only if the user chooses, since removing them breaks alternation); photographed pages with perspective (deskew handles rotation only, offers perspective fix as a second step); text-only PDFs with a slight rotated content stream (rotates the page, not the pixels); rotation of image with a text layer (the OCR layer rotates with it).
- **Quality bar:** at least 99% recall and at least 98% precision of blank pages on a labelled corpus of 50 scanned files; skew error under 0.2 degrees on at least 95% of pages; zero resampling when skew is under the threshold. Must match Adobe Acrobat's scan clean-up on blank and skew accuracy, with one drop instead of a project.
- **Engine:** OS first, own model second. Stamp versus blank classifier (the "🧠" part, to tell stamp or page-number-only pages from blank): OS engine: none. Own model on every platform. Everything else: pdfium render, own projection-profile and Hough deskew, own blank detector.
- **Surfaces:** bar, panel, right-click "Clean scan", CLI `panoply pdf clean in.pdf --blank --deskew` (also `--angle -1.5`), MCP `pdf_clean_scans`.

### 9. Crop and resize pages
- **Pattern:** B. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Trim the margins" or "make this A4 instead of Letter."
- **On drop:** reads every page's size, crop box and media box, and measures the content bounding box per page. If page sizes are mixed, says so and recommends Resize to one size. Recommends Auto-crop when margins exceed 15% of the page.
- **Options:** Mode: Crop (default) or Resize. Crop: Auto (content box plus 5 mm), Manual margins, Draw a box on the preview. Apply to: all pages, odd and even separately (default Auto computes odd and even separately for books). Resize: target size (A4, Letter, Legal, A5, custom), region default first; Scale content: fit (default) or no scale; Alignment centre. Real crop (remove content outside) vs hide (set crop box only): hide by default.
- **Preview:** a page with the new box, and a mixed-size warning if applicable.
- **Output:** "name (cropped).pdf" or "name (A4).pdf".
- **Result card:** "A4 to Letter, 148 pages, content scaled 94%." For crop: "Margins trimmed 18 mm average."
- **Edge cases:** hide-only crops leave hidden content recoverable (the privacy scan flags it, and the "Remove hidden area" option really deletes it); pages with annotations outside the box; mixed orientations; scanned pages with uneven margins (per-page boxes); signed files (warn).
- **Quality bar:** resize keeps all vector content vector, with no rasterising; text stays selectable (checked by text extraction equality); A4 to Letter on 100 corpus pages loses no content (bounding box fits). Must match iLovePDF Crop and add per-page auto boxes.
- **Engine:** lopdf boxes and content transforms, pdfium bounding box measurement.
- **Surfaces:** bar, panel, right-click "Crop" and "Resize pages", CLI `panoply pdf crop in.pdf --auto` and `panoply pdf resize in.pdf --size a4`, MCP `pdf_crop`, `pdf_resize_pages`.

### 10. Compress
Full worked example is in FLOWS.md (Worked example 1). Additions to it only:
- **Pattern:** B. **Job:** "Make this PDF small enough to email or upload."
- **Budgets:** standard. Under-a-size mode samples images to find the setting, then writes the file in one streamed pass.
- **On drop (additions):** a byte breakdown bar (images, fonts, content, scans, attachments) so the preview says where the savings come from. Scanned pages are re-encoded as JBIG2-class monochrome (lossy off, generic region only) when pure black and white, JPEG when grey or colour, and flagged for OCR-aware compression if a text layer exists.
- **Options (additions):** Image resolution cap: from the dominant image DPI (150 DPI for screen, 300 kept for print-looking files). Under-a-size mode searches over quality on a sample of the images (at most 5% of the image bytes) to find the setting, then writes the file in one streamed pass, and tells the user when the size cannot be reached.
- **Edge cases (additions):** target below the floor (say "cannot go below 340 KB without removing images", offer grayscale or lower DPI); pure-vector PDFs (nothing to shrink, stream and font recompression only); already compressed (say so); JBIG2 pattern-matching substitution errors (never used, generic region only, so no character substitution); images with soft masks and ICC profiles (colour kept); signed (warn); encrypted (keep encryption).
- **Quality bar (addition):** size at equal SSIM (at least 0.97) is compared on the 100-PDF corpus against iLovePDF "recommended", Smallpdf, Adobe Acrobat "Reduce file size" and PDF24, and ours must be no bigger; target-size mode hits within 5% under the target when reachable; one reachable-target run takes under 3 seconds for 10 MB input on an M-series laptop (target, not measured).
- **Surfaces:** CLI `panoply pdf compress in.pdf --under 2mb`, MCP `pdf_compress`.

### 11. Repair
- **Pattern:** A. **Inputs:** one or many broken PDFs.
- **Budgets:** standard.
- **Job:** "This PDF will not open or is missing pages."
- **On drop:** attempts a normal parse; if it fails (missing xref, bad trailer, truncated, junk before the header, wrong stream lengths, broken page tree), switches to a full object scan of the file bytes and reports what it found: "2 of 41 pages unrecoverable".
- **Options:** none by default. Hidden: Keep only recovered text (for very damaged files), Password if encrypted.
- **Preview:** a thumbnail strip of recovered pages, with unreadable pages shown as grey placeholders marked missing.
- **Output:** "name (repaired).pdf", rebuilt xref, valid trailer, pruned dangling objects.
- **Result card:** "Rebuilt cross-reference table. 39 of 41 pages recovered. 2 pages lost: truncated at byte 1,204,112."
- **Edge cases:** download truncated mid-file (recover everything before the cut); incremental updates with a corrupt last revision (fall back to the previous revision and say which); wrong Length values (ignore and scan for endstream); fonts missing (page kept, shows substitution); circular page trees; encrypted files (needs password first, never guessed); a file that is not a PDF at all (HTML error page saved as .pdf, ZIP, image with the wrong extension: say what it is and offer the right module).
- **Quality bar:** on a corpus of at least 100 deliberately broken files (truncated, xref-corrupt, bit-flipped, junk-prefixed), recovers every page that pdfium itself can render from the damaged original plus more. Must recover at least as many pages as iLovePDF Repair and PDF24 on the same broken files; never produce a file that opens with a warning in Acrobat.
- **Engine:** own tolerant object scanner over lopdf, pdfium to verify each recovered page renders.
- **Surfaces:** bar (auto-offered when any module fails to parse), right-click "Repair", CLI `panoply pdf repair in.pdf`, MCP `pdf_repair`.

### 12. OCR
- **Pattern:** B. **Inputs:** one or many scanned PDFs or images.
- **Budgets:** standard for CPU and memory (one page image at a time, runs in the worker pool). OS engines: no download, no resident memory for us. The fallback PaddleOCR-class model (about 15 MB per language) downloads per language on first use with the size shown first, and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Job:** "Make this scan searchable and selectable."
- **On drop:** classifies each page as scanned, mixed or already text (skips pages that already have a good text layer). Detects script and language per page from a quick sample (Latin, Devanagari, Arabic, CJK, and so on), and picks the language model set. Detects page orientation and rotates before recognition.
- **Options:** Languages: auto-detected, shown as chips, multiple allowed (Hindi plus English is common). Mode: Add text layer, keeping the original image (default); Replace the page image with a cleaned one (smaller, off); Redo pages that already have text: off. Output also as text file: off.
- **Preview:** one page with word boxes overlaid and the recognised text beside it for a quick check, and a confidence indicator per page.
- **Output:** "name (searchable).pdf" with an invisible text layer positioned per word; original page images untouched byte for byte in the default mode.
- **Result card:** "34 pages, Hindi and English, 12,480 words, 6 low-confidence pages flagged."
- **Edge cases:** right-to-left text (Arabic, Urdu, Hebrew: logical order in the text layer, right-to-left selection); mixed language on one page (per-region language); tables and columns (reading order by layout analysis, not left to right line by line); handwriting (flagged as unsupported or low confidence, no false claim); very large scans (page at a time, bounded memory); rotated pages (auto); already-OCRed with a poor layer (offer redo); signed (warn: adds content after signing); skewed pages (run Clean scans first and suggest it).
- **Quality bar:** character error rate on a labelled corpus per language we list (target under 2% on clean printed Latin and Devanagari at 300 DPI); the text layer aligns within one line height of the printed words; time under 2 s per page on a laptop for printed Latin. Must match iLovePDF OCR and Smallpdf OCR on accuracy per language, cover 100+ languages, and match Adobe Acrobat on Indian and Arabic scripts, measured on the same corpus.
- **Engine:** OS first, own model second. macOS and iOS: Apple Vision VNRecognizeTextRequest, and RecognizeDocumentsRequest on iOS and macOS 26 (paragraphs and tables). Android: ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean). Windows: Windows.Media.Ocr (OcrEngine), and Windows AI Text Recognition on Copilot+ PCs. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): PaddleOCR-class model (Apache-2.0), downloaded per language. An OS engine is used only when it covers the language and gives word boxes. Own layer writer (render mode 3 text with the correct font and widths).
- **Surfaces:** bar, panel, right-click "Make searchable", phone share sheet, CLI `panoply pdf ocr in.pdf --lang hi,en`, MCP `pdf_ocr`.

### 13. Edit text
- **Pattern:** C. **Inputs:** one PDF.
- **Budgets:** standard. The editor holds the objects of the current page and one neighbour only.
- **Job:** "Change a word, a number or a date in this PDF."
- **On drop:** opens the editor on page 1 with text blocks outlined on hover; detects whether each text run uses an embedded subset font, a standard font or is part of a scanned image (not editable, offers OCR first). Tells the user per block whether the font matches fully.
- **Options:** none up front. In the editor: click a block, type; font, size, colour (taken from the block); Add text box; Delete block; Find and replace across the file ("2025" to "2026") with a preview of each hit.
- **Preview:** the live page. A marker on any glyph that needed a substitute ("Edited text uses Arial instead of the embedded subset of Gotham").
- **Output:** "name (edited).pdf".
- **Result card:** "7 edits across 3 pages, 1 font substituted (Gotham to Arial)."
- **Edge cases:** embedded subset fonts missing the new letter (substitute from the installed system font with matching metrics, or re-embed the full font if present on the device, and warn); text reflow (blocks reflow within their own box only, never move neighbours); right-to-left and complex scripts (edit in logical order with shaping, Arabic and Devanagari work, ligatures kept; if shaping is unsafe the block is locked and says why); tables (cell by cell, no reflow); forms (edit field values in Fill forms); signed documents (editing breaks signature, warning); scanned pages (route to OCR); text converted to curves (not editable, say so); hidden OCR layers (edit changes both visible and layer only if the user asks).
- **Quality bar:** edited page renders pixel-identical outside the edited box on all corpus files (SSIM 1.0 outside the box); an edit of a same-length word in an embedded-subset font keeps the font (no substitution) when the glyphs exist; round trip text extraction returns the new text exactly. Must match Adobe Acrobat on in-place editing for Latin text, and beat any tool that only adds an overlay text box over the old text.
- **Engine:** pdfium page-object model for text objects plus own font matching and glyph-metric code, HarfBuzz-class shaping (own Rust shaping with rustybuzz) for complex scripts.
- **Surfaces:** panel only (editor), right-click "Edit PDF", CLI for scripted replace `panoply pdf replace in.pdf --find "2025" --with "2026"`, MCP `pdf_replace_text` (the interactive editor has no MCP form).

## C. Mark up, sign, fill, protect

### 14. Annotate (includes Remove annotations)
- **Pattern:** C. **Inputs:** one PDF.
- **Budgets:** standard.
- **Job:** "Highlight, comment and draw on this PDF", and "strip all the comments."
- **On drop:** lists existing annotations by type and author with counts, and says if the file has comments from others (shown as read-only unless the user takes ownership). Opens with the Highlight tool armed when the file has a text layer, Freehand when it is a scan.
- **Options:** Tools: highlight, underline, strikeout, note, text box, arrow, rectangle, ellipse, freehand, image. Colour: the last used per tool. Remove annotations: by type, by author, by page range, or all (a one-click button in the report).
- **Preview:** the live page; the sidebar lists every annotation with jump-to.
- **Output:** "name (annotated).pdf", standard annotation objects, not burned in; "Flatten annotations" lives in Flatten. Remove: "name (clean).pdf".
- **Result card:** "12 annotations added." Remove: "38 comments, 4 highlights and 2 stamps removed."
- **Edge cases:** highlights over scanned pages with no text (rectangle highlight with multiply blend, not text-based); highlights on right-to-left text (quad points follow glyph boxes in logical order); annotations that other viewers render differently (we write appearance streams for every annotation so Preview, Chrome, Acrobat agree); popup and reply threads (kept on remove-by-type only if asked); link annotations (never removed by "all comments", only by explicit choice); signed (adding annotations is allowed after signing by the spec, we use an incremental save so the signature stays valid).
- **Quality bar:** the file shows the same annotation in pdfium, Chrome, Preview and Acrobat on the corpus; incremental save leaves existing signatures valid; open to first mark in under 500 ms. Must match Smallpdf and iLovePDF on tool coverage and Adobe Acrobat on highlight geometry.
- **Engine:** pdfium annotation API, own appearance-stream writer, lopdf for incremental save.
- **Surfaces:** panel, right-click "Annotate", CLI `panoply pdf annotations in.pdf --remove all`, MCP `pdf_remove_annotations` and `pdf_annotate` (highlight text by search string).

### 15. Sign
- **Pattern:** C. **Inputs:** one PDF and a saved or new signature.
- **Budgets:** standard.
- **Job:** "Put my signature on this PDF."
- **On drop:** finds the likely signature place: a line near "Signature", "Sign here", "Date", a form signature field, or the last page bottom area. Arms the saved signature at that spot. Detects whether the file already has signature fields.
- **Options:** Signature: saved default (draw with trackpad, touch or pen, type with one of a few script fonts, or image with background removed by the Signature cleaner). Date and initials as separate stamps. Size: from the detected line height.
- **Preview:** the live page, signature draggable, scalable, with snap to the line.
- **Output:** "name (signed).pdf". The mark is an ordinary stamp or appearance, not cryptographic (the Digital signatures module does that).
- **Result card:** "Signed on page 4, date added." Plus the reminder "visual signature, not a certificate signature."
- **Edge cases:** form signature fields (fills the field, so a later digital signature can still sign it); several places to sign (a "next sign here" jump); rotated pages (signature rotates upright); scanned pages (placed as an image); the saved signature stored only on this device, in the system keychain, never synced unless the user syncs presets; a file with a certificate signature already (adding a visual mark is an incremental save, so the old signature stays valid).
- **Quality bar:** signature image placed at the true selected size within 0.5 mm; on 50 corpus forms the detected sign spot is the right one for at least 85%; hand-drawn signatures are smoothed without losing shape. Must match Smallpdf and iLovePDF sign on speed to a placed signature, with no account and no daily cap.
- **Engine:** pdfium and own stamp writer, saved signature in the OS secure store.
- **Surfaces:** panel, right-click "Sign", phone (finger or Apple Pencil), CLI `panoply pdf sign in.pdf --signature default --page last --at 120,80`, MCP `pdf_sign_visual`.

### 16. Stamps (includes Self-attest stamp)
- **Pattern:** A for self-attest and stock stamps, with a placement step when the position is not obvious. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Stamp Approved, Paid or Draft", and "put 'Self attested' with my signature and date on every page."
- **On drop:** for a self-attest request, reads the page margins to find free space at the bottom right and keeps the stamp out of the content. For stock stamps, defaults to the page-1 top right.
- **Options:** Stamp: Approved, Paid, Draft, Confidential, Received, custom text or image. Self-attest text: "Self attested" plus the saved name and today's date, plus the saved signature (off if no signature saved). Pages: all for self-attest, page 1 for stock stamps. Colour and rotation: per stamp style. Opacity 90%. Date format from the region.
- **Preview:** page 1 with the stamp, and a check that it does not cover text on any page (red marker on pages where it would).
- **Output:** "name (self attested).pdf", "name (stamped).pdf".
- **Result card:** "Self-attested on 14 pages, placed bottom right, no content covered."
- **Edge cases:** pages with a full-bleed scan (stamp placed in the corner over the image); mixed page sizes (stamp scaled per page); page rotations (stamp upright); pages where margin space is not free (moves up, or places over the least-ink corner and tells the user); right-to-left documents (stamp on the left side if the user prefers).
- **Quality bar:** zero text overlap on at least 95% of 200 corpus pages with default placement; render equal across pdfium and Acrobat. The one-step self-attest on every page is the whole point; the comparison is doing it by hand in a generic PDF editor.
- **Engine:** lopdf content append with own appearance stream, pdfium free-space measurement.
- **Surfaces:** bar ("self attest"), panel, right-click "Self attest", CLI `panoply pdf stamp in.pdf --text "Self attested" --signature default --all-pages`, MCP `pdf_stamp`.

### 17. Digital signatures
- **Pattern:** I for validation, A for signing. **Inputs:** one PDF and optionally a certificate.
- **Budgets:** standard. The only network use is the optional timestamp server, named before it is contacted.
- **Job:** "Sign this with my certificate", "is this signature valid?", "remove the signature."
- **On drop:** if the file has signatures, shows an inspect report at once: signer, certificate chain, valid or tampered, signed at, whether the document changed after signing, timestamp status. If not, offers Sign.
- **Options:** Certificate: PKCS#12 file or a keychain, Windows store or smart-card identity (default: the last used). Appearance: visible with the saved visual signature, or invisible. Timestamp: off by default, choose a trusted timestamp authority (names the server before contacting it, since this uses the network). Level: PAdES baseline B (default), with LT and LTA as options. Certify (no changes allowed): off.
- **Preview:** the signature box on the page, and for validation a plain verdict ("Valid. Signed by Acme Ltd on 3 Mar 2026. Not changed since.").
- **Output:** "name (digitally signed).pdf" by incremental save. Remove signature: "name (unsigned).pdf" with a warning that the document no longer proves who signed.
- **Result card:** the signer, algorithm (SHA-256 with RSA or ECDSA), timestamp, and "valid".
- **Edge cases:** multiple signatures (each checked, covering range shown); signed then edited (reported as modified after signing, with the page changes where visible); expired or untrusted certificates (reported as such, not as "invalid"); self-signed (valid mathematically, untrusted chain, said plainly); Indian DSC tokens and Aadhaar eSign (supported through the OS smart-card API where available, otherwise stated unsupported); encrypted PDFs; broken byte ranges.
- **Quality bar:** validation verdicts agree with Adobe Acrobat on a corpus of at least 100 signed files (valid, tampered, expired, chain-untrusted, modified-after-signing); signed files pass Acrobat's validation and the EU DSS validator for PAdES-B. Verdicts must match Adobe Acrobat; the key never leaves the OS store, which is the difference from browser-based signing such as BentoPDF's.
- **Engine:** own CMS and PAdES code on RustCrypto (cms, x509-cert, rsa, p256), lopdf incremental save, OS keystore bindings. No OpenSSL needed.
- **Surfaces:** panel, right-click "Check signatures", CLI `panoply pdf signature verify in.pdf` and `panoply pdf signature sign in.pdf --cert me.p12`, MCP `pdf_verify_signatures` (signing needs a user-held certificate, so `pdf_sign_digital` asks the user to approve).

### 18. Fill forms
- **Pattern:** C. **Inputs:** one PDF form.
- **Budgets:** standard.
- **Job:** "Type into this form and save it."
- **On drop:** detects AcroForm fields, XFA forms (flagged), and "flat" forms with no fields (offers Create form or free text placement). Detects dates, name, address, email and phone fields by name and nearby label, and suggests values from the user's saved profile (always shown before filling, never silent).
- **Options:** Autofill from profile: suggest, not apply (default), apply. Save as: Filled and still editable (default) or Flattened. Field highlight: on.
- **Preview:** the live form, tab order, required fields marked, a "fields left" count.
- **Output:** "name (filled).pdf".
- **Result card:** "26 of 28 fields filled, 2 required fields empty."
- **Edge cases:** XFA forms (fill the static fallback if present, otherwise explain they need a viewer that supports XFA); JavaScript calculated fields (run a safe subset: sums, products, date formats; never run arbitrary script); comb fields and fixed-width character cells; right-to-left fields; checkbox groups and radio buttons; signed forms (filling after signing invalidates, warn; unless the field is unlocked by the signature's field lock rules); fonts missing the characters typed (embed a full font, e.g. for Devanagari); very long text in fixed boxes (shrink to fit, said).
- **Quality bar:** filled values display the same in pdfium, Chrome, Preview and Acrobat on 100 corpus forms; calculations match Acrobat's output on the corpus's calculated fields; no field loses its appearance on flatten. Must match Adobe Acrobat on calculated and comb fields in typical government forms, and be no worse than Smallpdf and iLovePDF fill.
- **Engine:** pdfium form fill (FPDF_FFL) with own appearance regeneration and a restricted expression evaluator for calculations.
- **Surfaces:** panel, right-click "Fill form", CLI `panoply pdf fill form.pdf --data answers.json` (also `--dump-fields`), MCP `pdf_fill_form` and `pdf_list_fields`.

### 19. Create form
- **Pattern:** B. **Inputs:** one flat PDF.
- **Budgets:** standard, plus a document-layout detection model (OS engine: none, so it is used on every platform; target under 50 MB, size shown before the first download), loaded on first use and unloaded after 60 s idle.
- **Job:** "Make this flat form fillable."
- **On drop:** a layout model finds label and blank pairs: underlines, boxes, comb cells, checkboxes, tables of fields. Classifies each (text, date, checkbox, radio group, signature) and names them from the nearby label.
- **Options:** Sensitivity: default from how many lines and boxes the page has. Field naming: from the label (default). Add a signature field: on if "Signature" text is found.
- **Preview:** the page with proposed fields overlaid; click to change type, drag to resize, delete or add.
- **Output:** "name (fillable).pdf" with real AcroForm fields, tab order following reading order.
- **Result card:** "31 fields created: 19 text, 6 date, 5 checkboxes, 1 signature."
- **Edge cases:** scanned forms (fields placed over the image after the layout model reads the lines; OCR assists); comb fields (cells recognised and made into a comb field with the right max length); tables with repeating rows (one field per cell with indexed names); radio groups (grouped by proximity); forms that already have fields (kept, only missing ones proposed); right-to-left forms.
- **Quality bar:** on a labelled set of at least 50 forms, at least 90% of real blanks get a correctly typed and placed field, with false fields under 5%. Adobe Acrobat's "Prepare Form" is the bar; match it on recall and beat it on time to a finished form (one click versus review). Must also beat a manual field-by-field setup in time.
- **Engine:** OS first, own model second. Layout detection of label and blank pairs: OS engine: none. Own model on every platform. On-device document-layout detection model. Also own line and box detection, lopdf AcroForm writer.
- **Surfaces:** panel, right-click "Make fillable", CLI `panoply pdf create-form in.pdf`, MCP `pdf_create_form`.

### 20. Flatten
- **Pattern:** A. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Lock the filled form and comments into the page so nobody can edit them."
- **On drop:** counts form fields, annotations, and signature fields; recommends what to flatten (forms and annotations by default).
- **Options:** What: Forms (on), Annotations (on), Signature fields (off), Layers (off, they are in the Layers module). Keep links: on. Keep text selectable: on.
- **Preview:** counts and a before and after of one page.
- **Output:** "name (flattened).pdf".
- **Result card:** "28 fields and 6 annotations flattened. Text still selectable."
- **Edge cases:** fields with no appearance stream (we generate one first, otherwise the value would vanish); hidden fields (dropped, not printed); JavaScript actions (removed); signed PDFs (warn, flattening breaks the signature); transparency blending of highlight annotations (multiply kept); file with Optional Content layers (flatten visible state only if chosen).
- **Quality bar:** the flattened render equals the live-form render within SSIM 0.99 on the corpus; text extraction of the flattened page still returns the form values. Must match Smallpdf and iLovePDF flatten on look, and keep text selectable.
- **Engine:** pdfium (FPDFPage_Flatten) plus own appearance regeneration, lopdf cleanup.
- **Surfaces:** bar, right-click "Flatten", CLI `panoply pdf flatten in.pdf`, MCP `pdf_flatten`.

### 21. Protect and unlock
- **Pattern:** A. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Put a password on this", and "remove the password, I know it."
- **On drop:** reads the encryption state: none, open-password, owner-password only (restrictions), AES-128 or AES-256. If encrypted with an open password, asks for it once and tries it on all files in a batch. If only restrictions are set (opens without a password), unlocks it immediately and says "this file only had editing restrictions".
- **Options:** Protect: open password; permissions password (separate, optional); allowed actions (print, copy, edit, fill, annotate, all on by default). Algorithm: AES-256 (default; AES-128 for old readers, a labelled choice). Password strength meter and a generator offered. Unlock: password, or "no password needed" detected.
- **Preview:** a summary line with what will be locked or removed. Passwords are never saved to history.
- **Output:** "name (protected).pdf", "name (unlocked).pdf".
- **Result card:** "AES-256, opens with a password, printing allowed, copying blocked."
- **Edge cases:** lost password (we do not crack or brute-force, and say so; owner-only restrictions are removable because no password is needed to open); wrong password (plain error, no lockout); signed documents (encryption after signing is allowed for new copy, said); unicode passwords (normalised per the PDF 2.0 SASLprep rule); old RC4 files (read and upgraded to AES on request); batch with different passwords (asks per file only on failure); metadata encryption (on, so titles do not leak).
- **Quality bar:** protected files open with the given password in Acrobat, Preview, Chrome and pdfium (AES-256 R6) on 100 percent of test passwords including unicode; unlock of corpus files with owner-only restrictions succeeds on all. Must match iLovePDF and Smallpdf on reader compatibility, with the password never leaving the device.
- **Engine:** own encryption handler (AES, SHA-2 key derivation per ISO 32000-2) over lopdf, pdfium to verify open.
- **Surfaces:** bar, right-click "Protect" and "Unlock", CLI `panoply pdf protect in.pdf --password` (prompts, no password on the command line by default; `--password-file`), `panoply pdf unlock in.pdf`, MCP `pdf_protect`, `pdf_unlock` (password supplied by the user via a prompt, not a tool argument stored in logs).

## D. Redact, watermark, number, compare, inspect

### 22. Redact
- **Pattern:** C. **Inputs:** one PDF.
- **Budgets:** standard, plus the shared names model and OCR models for scans. OS engines: no download, no resident memory for us. The fallback names model (under 150 MB peak, as stated for Redact pasted text) and the fallback OCR model for scans download on first use and unload after 60 s idle, only on platforms where the quality bar test picks them. The 150 MB peak applies to the fallback path only. Pages are processed one at a time so a 500-page file stays under 300 MB. Regex and checksum detection need no model.
- **Job:** "Black out private information for real, so it cannot be recovered."
- **On drop:** finds sensitive items in the text layer (and in OCR text for scans, running OCR first if needed): emails, phone numbers, card numbers (Luhn checked), IBAN, Aadhaar, PAN, SSN, passport numbers, dates of birth, addresses, names (via a small on-device NER model). Shows them as pending redactions the user accepts or rejects. Region defaults (Aadhaar and PAN for India, SSN for the US) follow the device region.
- **Options:** Find: categories as chips (all PII on, names off by default because of false positives), custom text or regex, an area drawn by hand. Fill: black (default), white, or a label ("REDACTED"). Remove metadata too: on. Redact in images: off (drawn areas only).
- **Preview:** every match highlighted in context with a count per category; the burned-in result before saving.
- **Output:** "name (redacted).pdf". The text under the box is deleted from the content stream, images under the box are cropped and re-encoded, and the original is untouched. The file then runs the Redaction check automatically and shows the verdict.
- **Result card:** "47 items redacted: 12 emails, 9 phones, 3 Aadhaar, 23 names. Check passed: no text found under any box."
- **Edge cases:** text split across lines or across glyph runs (matched on reassembled text); ligatures and hyphenation; scanned pages (OCR box, then paint over the pixels, not just a layer); text in form fields and annotations (removed or flattened); text in bookmarks, metadata, XMP, attachments and the document outline (swept); right-to-left numerals (Arabic-Indic digits matched); redaction across a signature; text drawn as outlines (found by OCR only, said); invisible OCR layers (deleted for the redacted area).
- **Quality bar:** after redaction, zero recoverable characters under any box on the corpus when tested by pdfium, pdftotext-class and a raw stream search of the file (hard pass or fail, zero tolerance); PII recall of at least 95% on emails, phones and card numbers, at least 90% on Aadhaar and PAN on a labelled set. Must match Adobe Acrobat's Redact on removal, beat it on auto-detect coverage for Indian IDs, and match iLovePDF and Smallpdf on removal (verified by the check, not by claim).
- **Engine:** OS first, own model second. Names and addresses (NER): macOS and iOS: Apple NaturalLanguage NLTagger. Android: ML Kit Entity Extraction (structured entities such as dates and addresses, not person names, so names on Android use the fallback). Windows: none. Linux: fallback. Fallback (any platform where the OS engine is absent or fails the quality bar test in CI): own regex and checksum validators plus a small on-device NER model. OCR for scans: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback. Also pdfium for geometry, own content-stream rewriter that removes glyphs and clips images.
- **Surfaces:** panel (editor), bar, right-click "Redact", CLI `panoply pdf redact in.pdf --pii emails,phones,aadhaar` (also `--text "Acme"`), MCP `pdf_redact`.

### 23. Redaction check
- **Pattern:** I. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Is the blacked-out text in this PDF really gone?"
- **On drop:** instantly reads every page for text under black or white rectangles, painted-over regions, annotations with black fill, cropped-out content, images with text beneath, and hidden text. Shows findings with the recovered text in a safe view.
- **Options:** Check also: metadata, attachments, comments, hidden layers (all on). Report: a PDF or JSON redaction report listing what was checked and the time (off, a button on the result).
- **Preview:** the report itself, with a thumbnail and the recovered string for each finding, so the user sees the leak in context.
- **Output:** the report (PDF or JSON) next to the file. A fix button: "Redact these for real", which hands the findings to Redact.
- **Result card:** "3 fake redactions found on 2 pages: 2 names, 1 account number still readable."
- **Edge cases:** boxes placed by a black rectangle annotation (found); text covered by a white image (found); text rendered in white on white (found, labelled hidden); text clipped but still in the stream; redactions already done properly (reports "no text under 14 boxes"); scanned redactions (images with black boxes: nothing under them, passes); large file (streams page by page).
- **Quality bar:** zero false "clean" verdicts on the corpus's fake-redaction set (at least 40 files made with rectangles, annotations, white-on-white, clip masks); false alarms under 5% on properly redacted files. Adobe Acrobat's hidden-information tool is the nearest comparison; the per-box recovered-text report is ours.
- **Engine:** own geometry intersection of text runs and filled shapes over pdfium text boxes and lopdf stream parsing.
- **Surfaces:** bar, drop on window, CLI `panoply pdf redaction-check in.pdf --report`, MCP `pdf_redaction_check`.

### 24. Watermark
- **Pattern:** B. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Stamp text or a logo across every page."
- **On drop:** reads page sizes and colours; picks text colour with contrast against the page background and picks diagonal position for text, corner for logos.
- **Options:** Type: Text (default, last text used) or Image (logo, PNG with transparency). Position: centre diagonal (text), tiled option, or a grid of nine. Opacity: 25% for text, 100% for logos. Size: percent of page width. Layer: over content (default) or under. Pages: all, range, odd and even. Font: system fonts, with a fallback for non-Latin scripts.
- **Preview:** page 1 and the last page at 200%, since page size can differ.
- **Output:** "name (watermarked).pdf". The watermark is marked as an Artifact so it is easy to remove again by Remove watermark.
- **Result card:** "Watermark on 36 pages, 30% opacity."
- **Edge cases:** mixed page sizes and rotations (scaled and placed per page); non-Latin text (Arabic joins, Devanagari conjuncts shaped correctly); very dark scans (outline plus fill for contrast); text watermark being hard to read (suggests a stroke); huge files (stream); images with transparency (kept); signed (warn).
- **Quality bar:** the watermark is at the same relative position on every page of a mixed-size file (within 1 mm of intended); non-Latin shaping matches the platform renderer on 20 sample strings; adds under 5% to file size for text. Must match Adobe Acrobat for control and iLovePDF and Smallpdf for speed, with correct non-Latin shaping.
- **Engine:** lopdf content append with Optional Content or Artifact tagging, own text shaping (rustybuzz), pdfium for preview.
- **Surfaces:** bar, panel, right-click "Watermark", CLI `panoply pdf watermark in.pdf --text "CONFIDENTIAL" --opacity 30`, MCP `pdf_watermark`.

### 25. Remove watermark
- **Pattern:** B. **Inputs:** one PDF.
- **Budgets:** standard for exact removal (no model). The flattened-watermark path adds a small inpainting model (OS engine: none, so it is used on every platform; size shown before the first download), loaded only on that path, on first use, and unloaded after 60 s idle.
- **Job:** "Take the watermark off this PDF."
- **On drop:** classifies each repeated element across pages (keeping a fingerprint per element and page, never whole content streams): tagged Artifact watermarks, Optional Content layers named like "Watermark", repeated text or image XObjects at the same coordinates, stamp annotations, and, last, flattened watermarks (baked into the page image). The first four are removed exactly; the last is repainted by a model.
- **Options:** Which item (a chip per detected watermark, with a thumbnail, default: all detected watermark-type items). Repaint flattened: on only when the previous methods found nothing. Strength for repainting: automatic from the pattern.
- **Preview:** before and after of one page, with the removed element named ("Removed: image XObject, 14 pages").
- **Output:** "name (no watermark).pdf".
- **Result card:** "Watermark removed on 14 pages by deleting a layer. 0 pixels repainted." Or "Flattened watermark repainted on 9 pages: check these pages."
- **Edge cases:** the user's own watermark versus someone else's rights (we remove watermarks from your own files; a one-line note about copyright is shown before repainting flattened marks from third parties); watermark over text (exact removal leaves text intact, repainting may blur it; warns); semi-transparent logos on photos; different watermark per page (handled per page).
- **Quality bar:** exact removal is lossless (text extraction and the rest of the page unchanged) on the corpus's layered and artifact watermarks; for flattened marks, the repainted area has SSIM at least 0.9 against the clean original on a synthetic test set where we apply a watermark to known clean pages. Must match Stirling-PDF and PDF24 on the exact-removal cases, and the flattened-mark repaint is the extra.
- **Engine:** lopdf object and content-stream analysis for exact removal (no model). Inpainting of baked-in marks: OS first, own model second. OS engine: none. Own model on every platform. Small on-device inpainting model, downloaded once.
- **Surfaces:** panel, right-click "Remove watermark", CLI `panoply pdf unwatermark in.pdf`, MCP `pdf_remove_watermark`.

### 26. Page numbers, headers, footers (includes Bates numbering)
- **Pattern:** B. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Number the pages" and "add a header with the case name and a Bates number."
- **On drop:** measures free space at the top and bottom of every page and avoids content. Picks bottom centre for portrait documents and bottom right for landscape; picks the starting number 1, or continues from existing page labels if present.
- **Options:** Format: "3", "Page 3", "Page 3 of 12", "3 / 12" (default "Page 3 of 12" for documents, plain "3" for slides). Header and footer text with tokens {page}, {pages}, {date}, {file}, {title}. Bates: prefix, start, digits, suffix, and continue across multiple files in a batch (default off). Start at page, skip first page: off. Font: matches the document's body font and size minus 2 pt. Position and margin.
- **Preview:** pages 1, middle and last with the text in place, plus the collision check.
- **Output:** "name (numbered).pdf".
- **Result card:** "Pages 1 to 148 numbered. Bates ACME000001 to ACME000148."
- **Edge cases:** footers that already exist (detects an existing page number and offers to replace it); pages with different sizes (position per page); rotated pages; Arabic-Indic or Devanagari digits (offered by language); Bates across a batch (single counter in file order, recorded in a log file); roman numerals for front matter (via Page labels); scanned pages (margin from the ink map).
- **Quality bar:** text never overlaps page content on at least 98% of 200 corpus pages with default margin; Bates sequences have zero gaps or repeats across a 1,000-page, 20-file batch. Must match Adobe Acrobat's Bates numbering and iLovePDF and Smallpdf page numbers; check whether they do multi-file continuous numbering before we say anything about it.
- **Engine:** lopdf content append, pdfium font and free-space measurement.
- **Surfaces:** bar, panel, right-click "Add page numbers", CLI `panoply pdf number in.pdf --format "Page {page} of {pages}"` (also `--bates ACME --start 1`), MCP `pdf_page_numbers`.

### 27. Compare
- **Pattern:** I. **Inputs:** two PDFs (old and new).
- **Budgets:** standard. Two pages are rendered at a time and text is diffed one aligned section at a time, so memory stays flat on long files.
- **Job:** "What changed between these two versions?"
- **On drop:** with two files, assigns old and new by modified time (swappable). Aligns pages (insertions and deletions detected, not just page-by-page), extracts text with positions, and renders both.
- **Options:** View: Side by side, Overlay (red and green), Text diff (default when both have text), Visual diff (default when scanned). Ignore: whitespace and line breaks (on), page numbers and dates (off, a chip when detected), fonts and style changes (off). Sensitivity for visual diff.
- **Preview:** the report itself: change count, a list of changes jumping to the page, and changes marked on the pages.
- **Output:** an optional report PDF with highlighted changes, and an optional "changes only" PDF.
- **Result card:** "27 changes on 9 pages: 14 text edits, 6 added paragraphs, 4 removed, 3 image or layout changes."
- **Edge cases:** reflowed text (word-level diff, not line-level, so a moved paragraph is one move not two edits); pages inserted in the middle (alignment by similarity, not by number); scanned versions (OCR then diff, flagged as approximate); right-to-left text (diff in logical order); tables (cell aware); different page sizes (normalised before visual diff); font substitutions (not reported as text changes).
- **Quality bar:** on 50 pairs of real contract-style revisions with known edits, finds at least 98% of text edits and reports false changes on under 2% of unchanged paragraphs; page-insertion alignment right on every corpus pair with inserted pages. Adobe Acrobat Compare is the benchmark; must match iLovePDF and PDF24 compare and beat them on moved-text handling and RTL, scored on the same pairs.
- **Engine:** pdfium text and geometry, own diff (Myers on words with paragraph alignment, run per aligned section so memory stays flat), pixel diff for images.
- **Surfaces:** panel, right-click on two selected files "Compare PDFs", CLI `panoply pdf compare old.pdf new.pdf --report out.pdf`, MCP `pdf_compare`.

### 28. Edit metadata
- **Pattern:** A (apply a clean) and I (shows the report). **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Change the title and author", "strip all the metadata."
- **On drop:** shows the Info dictionary, XMP, producer, creation and modified dates, custom properties, and, if present, tools and file paths leaked into the metadata (for example "C:\Users\priya\Documents\...").
- **Options:** Fields: title, author, subject, keywords, creator, producer, dates; Strip all: one button. Title default: filled from the first heading if empty. Keep XMP in sync with Info: on. Remove: edit history and thumbnails.
- **Preview:** a side-by-side list of before and after values.
- **Output:** "name (metadata).pdf", written as an incremental update only if a signature exists, otherwise rewritten cleanly so old values do not remain in the file.
- **Result card:** "Removed author, producer, 4 XMP blocks and a Windows file path. Title set."
- **Edge cases:** old values remaining in earlier revisions of an incrementally saved file (rewritten to a single revision so they are really gone, except for signed files where it warns); XMP and Info disagreement (synced); encrypted metadata; PDF/A files (must keep XMP valid; warns that stripping breaks conformance); signed (warns).
- **Quality bar:** after Strip all, a raw byte search of the output finds none of the removed values (hard pass or fail on the corpus); round trip leaves rendering unchanged. Must also handle the leftover-revision case that simple field editors miss.
- **Engine:** lopdf for Info, own XMP read and write, full rewrite.
- **Surfaces:** bar, panel, right-click "Edit PDF properties", CLI `panoply pdf meta in.pdf --title "..."` (also `--strip`), MCP `pdf_metadata`.

### 29. Privacy scan and Sanitize
- **Pattern:** I with fix buttons (Privacy scan), A (Sanitize). **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "What am I about to leak?" and "make this untrusted PDF safe to open."
- **On drop:** reads and lists: author and software metadata, comments and who wrote them, tracked layers (hidden Optional Content), hidden text, embedded files, JavaScript, launch and URI actions, form data, GPS in images (EXIF), file paths, document history, and links to external servers. Each item has a risk level and a "Remove" button; one "Fix all" for sharing, and a stricter "Sanitize" preset for untrusted files.
- **Options:** Preset: Share safely (removes metadata, comments, hidden layers, history), Sanitize untrusted (adds: removes JavaScript, embedded files, launch actions, external links to non-http, XFA, and rewrites the file from clean objects). Keep links: on. Keep form fields: on for sharing, off for untrusted.
- **Preview:** the report with counts by category and what each fix changes.
- **Output:** "name (safe).pdf".
- **Result card:** "Removed 2 authors, 14 comments, 1 hidden layer, 3 images with GPS, 1 script. Opens with no active content."
- **Edge cases:** content that looks like a hidden layer but is visible (shown, left alone); JavaScript that is needed for calculations in forms (the user is asked); embedded files that are the whole point (an invoice's XML in ZUGFeRD: detected and kept on Share safely, flagged); signed (warn); malformed objects crafted to crash readers (parser fuzzed; the module never executes anything).
- **Quality bar:** the scanner finds every item in a corpus of at least 60 crafted files (one per hazard category); sanitized output contains no active content when checked by a second independent parser (pdfium) and by raw search; parser has zero crashes on the fuzz corpus run in CI. Must match Adobe Acrobat's Sanitize and Stirling-PDF's sanitise on what is removed, and report every change in plain words.
- **Engine:** lopdf object-graph walk, own EXIF reader for images, rewrite to a clean file.
- **Surfaces:** bar ("what is in this pdf"), drop on window, right-click "Privacy scan" and "Sanitize", CLI `panoply pdf privacy in.pdf` and `panoply pdf sanitize in.pdf`, MCP `pdf_privacy_scan`, `pdf_sanitize`.

## E. Structure, output and print preparation

### 30. Attachments
- **Pattern:** A. **Inputs:** one PDF, plus files to embed.
- **Budgets:** standard.
- **Job:** "Add files inside this PDF" or "get the files out of this PDF."
- **On drop:** lists embedded files (document-level and file-attachment annotations) with name, size, type and date. Dropping a PDF together with other files means "attach"; dropping one PDF alone and pressing Return means "extract all".
- **Options:** Add: files, an optional description, and relationship type (Source, Data, Alternative). Extract: all (default) or chosen. Remove: chosen. Output of extract: a folder next to the file.
- **Preview:** the list with a safe-type indicator (executables flagged).
- **Output:** "name (with attachments).pdf" or a folder "name (attachments)".
- **Result card:** "3 files embedded, 412 KB added." or "5 files extracted."
- **Edge cases:** invoices with embedded XML (ZUGFeRD or Factur-X: shown as an invoice and preserved); PDF portfolios (collections: extract as files); duplicate names (numbered); executables and macros inside (never run, extract warns); names with path tricks like "../" (sanitised, never written outside the folder); encrypted attachments follow the document password; PDF/A-3 requirement for attachments (offer to keep conformance).
- **Quality bar:** extracted files are byte-identical to the originals (hash check on the corpus); no write outside the target folder on the path-trick fuzz set. Match Stirling-PDF's attachment handling and add the safety checks.
- **Engine:** lopdf EmbeddedFiles name tree and annotation handling.
- **Surfaces:** bar, right-click "Extract attachments", CLI `panoply pdf attach in.pdf data.xlsx` and `panoply pdf attachments in.pdf --extract`, MCP `pdf_attachments`.

### 31. Leak tracing
- **Pattern:** A, one file in, one copy per recipient out. **Inputs:** one PDF and a list of recipients.
- **Budgets:** standard.
- **Job:** "Give each person their own copy so I know who leaked it."
- **On drop:** reads the file for spots where invisible marks survive (text, fonts, spacing) and whether it is signed or encrypted. Recipients come from a pasted list or CSV of names or emails.
- **Options:** Marks: invisible (default) as micro-variations in word spacing and an ID in metadata and a hidden watermark; plus a faint visible footer with a short code (off by default). Strength: from how long the file is. Recipient list.
- **Preview:** count of copies and which mark types apply; a test "identify this file" box.
- **Output:** a folder with "name (recipient).pdf" per recipient, and a private mapping file (code to recipient) kept only on this device.
- **Result card:** "40 copies. 3 mark types each. Keep leak-map.csv private."
- **Edge cases:** the file printed and rescanned (only the visual marks and spacing survive, said honestly, not guaranteed); someone stripping metadata (spacing marks survive); copy-paste of text (a text-level mark is not in the pasted text, said); two recipients colluding and comparing files (can detect, flagged as a limit); scanned documents (spacing marks do not apply; visual mark only); PDF/A (marks do not break conformance).
- **Quality bar:** identify the right recipient from the leaked file in 100% of tests where only metadata is stripped and in at least 95% where the file is re-saved or compressed (a number we must measure before we ship; the claim is not made until measured); no visible change at normal zoom (SSIM at least 0.999). We found no mainstream competitor doing per-recipient copies this way in our research (re-check before launch).
- **Engine:** own code: per-recipient spacing encoding in text positioning operators, hidden Optional Content stamp, metadata ID.
- **Surfaces:** panel, CLI `panoply pdf trace in.pdf --recipients list.csv` and `panoply pdf trace --identify leaked.pdf`, MCP `pdf_leak_trace` and `pdf_leak_identify`.

### 32. Layers
- **Pattern:** I with direct toggles. **Inputs:** one PDF.
- **Budgets:** standard.
- **Job:** "Show, hide or delete the layers in this drawing or map."
- **On drop:** lists Optional Content groups with initial state, nesting, and which pages use them. Says "no layers" if none. Flags layers named like watermark, comments, hidden or draft.
- **Options:** per layer: Show, Hide, Remove. Presets: "Print state" and "View state". Remove hidden layers: one button (really deletes content, not just hides).
- **Preview:** the live page re-rendered as toggles change.
- **Output:** "name (layers).pdf".
- **Result card:** "5 layers: 2 hidden layers removed, 3 visible."
- **Edge cases:** content in a hidden layer that is still recoverable (removal deletes it; hiding does not, a privacy scan item); nested layers and radio-button groups (OCG intent respected); layers that depend on usage (print versus view) application settings; CAD and map PDFs with hundreds of layers (searchable list); layer state controlled by JavaScript (state set statically, script removed).
- **Quality bar:** rendered result matches Acrobat for the same layer state on at least 30 corpus drawings; removed layers leave no recoverable content (raw search). Must match Adobe Acrobat and BentoPDF on layer handling.
- **Engine:** lopdf OCProperties, pdfium rendering with layer state.
- **Surfaces:** panel, right-click "Layers", CLI `panoply pdf layers in.pdf --hide "Notes" --remove-hidden`, MCP `pdf_layers`.

### 33. Web optimize
- **Pattern:** A. **Inputs:** one or many PDFs.
- **Budgets:** standard. Holds the object table, not object content.
- **Job:** "Make this PDF open fast in a browser while it is still downloading."
- **On drop:** checks whether the file is already linearized, its size, and whether it is encrypted or signed. Recommends only if the file is over 1 MB, else says "small enough that it makes no difference".
- **Options:** none. A hidden one: also compress object streams (on, safe).
- **Preview:** "first page needs 80 KB instead of the full file" estimate.
- **Output:** "name (web).pdf".
- **Result card:** "Linearized. First page loads from the first 118 KB. Size 5.2 MB to 5.0 MB."
- **Edge cases:** already linearized (say so and do nothing); signed (linearizing rewrites the file, breaks signatures, warn); incrementally updated files (flattened to one revision first); huge files (stream); encrypted (keeps encryption).
- **Quality bar:** output passes `qpdf --check-linearization` equivalent and is correctly linearized on the whole corpus; first-page byte offset matches or beats qpdf on every file. Match PDF24 and beat any tool that does not verify its output.
- **Engine:** lopdf with own linearization writer (hint tables), pdfium to verify.
- **Surfaces:** bar, right-click "Web optimize", CLI `panoply pdf linearize in.pdf`, MCP `pdf_web_optimize`.

### 34. Rasterize and outline fonts
- **Pattern:** B. **Inputs:** one or many PDFs.
- **Budgets:** standard, except that peak memory scales with page size, capped at 300 MB by rendering in 64 MB tiles.
- **Job:** "Flatten pages to pictures" or "turn text to shapes so the printer does not need my fonts."
- **On drop:** lists fonts (embedded or not, subset or full, licence flags where present) and counts pages. Recommends Outline text when any font is not embedded, and Rasterize when the user is sending to a viewer that strips selection.
- **Options:** Mode: Outline fonts (vector, keeps look, loses selectable text) or Rasterize pages (images). Raster: DPI 150 (screen) or 300 (print), chosen from page size and content; format JPEG or PNG (PNG when mostly black and white); keep an invisible text layer: off. Pages: all.
- **Preview:** a page and file size estimate, plus a warning that text will no longer be selectable.
- **Output:** "name (outlined).pdf" or "name (rasterized).pdf".
- **Result card:** "4 fonts outlined, 36 pages. Text is no longer selectable. 2.1 MB to 2.4 MB."
- **Edge cases:** Type 3 and CID fonts and fonts without glyph outlines (outlines taken from the embedded program; non-embedded fonts are substituted first, said); right-to-left shaped text (positions kept, no re-shaping); huge pages (poster size) at 300 DPI (tile and limit memory); form fields and annotations (flattened first); transparency (flattened into the output); rasterising an already scanned file (no gain, said).
- **Quality bar:** outlined page renders within SSIM 0.995 of the original at 300 DPI on the corpus; raster pages at 300 DPI match pdfium's render exactly. Must match Adobe Acrobat's convert-to-outlines in preflight.
- **Engine:** pdfium render and glyph path access, own path writer in lopdf.
- **Surfaces:** panel, right-click "Outline fonts", CLI `panoply pdf outline in.pdf` and `panoply pdf rasterize in.pdf --dpi 300`, MCP `pdf_outline_fonts`, `pdf_rasterize`.

### 35. Recolor and Grayscale
- **Pattern:** B. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Black and white for printing", "dark mode for night reading", "change the background colour."
- **On drop:** measures the page: mostly white background with dark text (document), coloured background, photographs, and how many pages use colour at all. Recommends Grayscale for print, Dark for a document when the system is in dark mode.
- **Options:** Mode: Grayscale (print) with Keep photos in colour: off; Dark mode (text light, background dark, photos unchanged by default); Custom (background colour, text colour); Sepia. Contrast: from the page. Output as vector recolour or image recolour: vector when text is real, image when a scan.
- **Preview:** a page before and after at 200%, with a toggle for photos.
- **Output:** "name (grayscale).pdf", "name (dark).pdf".
- **Result card:** "Recoloured 148 pages, vector text kept, 6 photos left in colour."
- **Edge cases:** scanned pages (recolour as an image, with an invert and threshold option); photos in dark mode (kept as is, because inverting ruins them); highlighted and annotated text (colours remapped consistently); transparency groups and blend modes; spot colours and ICC profiles (converted correctly, no hue shifts in grayscale); text on images (contrast guard); forms (fields re-coloured as appearance).
- **Quality bar:** grayscale luminance matches the ITU BT.709 conversion within one level on the corpus; text stays selectable on every non-scanned file; grayscale cuts size by at least what dropping colour channels achieves in lossless images (we measure, no claim yet). Must match Adobe Acrobat's grayscale conversion output, and add saving a dark PDF as a file.
- **Engine:** lopdf content-stream colour operator rewrite (own), pdfium render for scans, image channel conversion.
- **Surfaces:** bar ("grayscale pdf", "dark mode pdf"), right-click "Grayscale", CLI `panoply pdf recolor in.pdf --mode dark` and `panoply pdf grayscale in.pdf`, MCP `pdf_recolor`, `pdf_grayscale`.

### 36. Navigation: Bookmarks, Contents page, Page labels
- **Pattern:** C (outline editor), with A one-click auto generation. **Inputs:** one PDF.
- **Budgets:** standard. Rules run first; the optional ambiguity model is tiny (target under 10 MB). It has no OS engine, so it loads on first use on every platform and unloads after 60 s idle.
- **Job:** "Add bookmarks", "make a clickable contents page", "number the front pages i, ii, iii."
- **On drop:** reads the outline, page labels and the font size and weight of text to infer headings. If no outline exists, proposes one from heading levels (font size and bold, numbered "1.2" patterns, known "Chapter" words, right-to-left headings included). If a printed contents page exists, parses its entries and page numbers and links them.
- **Options:** Bookmarks: Auto-generate (default when none), keep existing, depth (levels 1 to 3). Contents page: placement (front, after cover), style (dots and numbers), levels. Page labels: ranges with style (decimal, roman, letters) and prefix; defaults detect a roman front matter by the printed numbering.
- **Preview:** the outline tree beside the page; contents page rendered; the page label mapping ("cover i, ii, iii... 1, 2, 3").
- **Output:** "name (bookmarked).pdf". The contents page is inserted as real pages with link annotations, and later page numbers shift correctly.
- **Result card:** "46 bookmarks generated (3 levels). Contents page added on page 2."
- **Edge cases:** headings in scanned files (OCR required, offered); headings drawn as images (skipped, said); right-to-left outlines (title text stored as proper Unicode); outlines pointing at pages that are later moved (re-linked by destination); Unicode titles (UTF-16BE correctly encoded); very long outlines (collapsed by default).
- **Quality bar:** on 30 corpus books and reports with real outlines hidden, auto-generation recovers at least 85% of the original level 1 and level 2 entries with correct page targets; every link lands on the right page. Must match Adobe Acrobat's bookmark editing and add auto generation.
- **Engine:** pdfium text style extraction, own heading classifier (rules first). Heading ambiguity: OS first, own model second. OS engine: none. Own model on every platform. Small on-device model, optional. Also lopdf outline and page-label writer.
- **Surfaces:** panel, right-click "Bookmarks", CLI `panoply pdf bookmarks in.pdf --auto` (also `panoply pdf toc`, `panoply pdf labels --roman 1-4`), MCP `pdf_bookmarks`, `pdf_toc_page`, `pdf_page_labels`.

### 37. Extract images
- **Pattern:** A. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Save every picture in this PDF at full quality."
- **On drop:** lists the embedded images with size, format, colour space and where used, and merges duplicates (same image object used on 20 pages is one). Warns when a "page image" is a full-page scan.
- **Options:** Format: Original (default, so JPEGs come out byte-identical), PNG, WebP. Minimum size: 64 x 64 px (skips icons and spacer pixels). Include masks and layered composites: on. Also page renders: off.
- **Preview:** thumbnail grid with checkboxes and dimensions.
- **Output:** a folder "name (images)" with names like "p03-img1.jpg".
- **Result card:** "58 images, 41 unique, 38 MB, JPEGs extracted without re-encoding."
- **Edge cases:** images split into strips; images with soft masks (alpha preserved as PNG); CMYK JPEGs (kept, or converted with ICC when saving to PNG); JPEG 2000 and JBIG2 (converted to PNG or lossless); inline images; image drawn with an applied clip (offered as "as seen on the page" too); huge images (stream); scan pages (extract returns the page image, said).
- **Quality bar:** original-format extraction is byte-identical to the embedded stream on every corpus JPEG; non-original output is lossless relative to the decoded image. Must match PDF24 on stream fidelity, and beat any tool that renders pages instead of extracting the embedded image.
- **Engine:** lopdf stream access, own JBIG2 and JPEG 2000 decode via pure Rust crates, pdfium for masks and clipped views.
- **Surfaces:** bar, right-click "Extract images", CLI `panoply pdf images in.pdf -o folder`, MCP `pdf_extract_images`.

### 38. Extract text (includes Prepare for AI)
- **Pattern:** A. **Inputs:** one or many PDFs.
- **Budgets:** standard. The optional layout model for tables and formulas (OS engine: none, so it is used on every platform) is loaded on first use (size shown before the first download) and unloaded after 60 s idle. Scans use the OCR route and its budgets: OS OCR engines: no download, no resident memory for us. The fallback OCR model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Job:** "Get the text out", and "clean this PDF into text I can paste into an AI tool."
- **On drop:** detects whether there is a real text layer, whether it is garbled (missing Unicode mapping, ligature and private-use-area characters), whether the file is a scan (offers OCR first), columns, tables, headers and footers repeated on every page. Chooses Markdown for documents with headings and tables, plain text for simple files.
- **Options:** Format: Plain text, Markdown (default for structured docs), HTML, JSON with positions. Prepare for AI preset (default for the "AI" entry): remove repeated headers, footers and page numbers; rejoin hyphenated words; reorder columns correctly; headings as Markdown; tables as Markdown tables; footnotes at the end; token count estimate. Page range.
- **Preview:** the first page of the text beside the page with reading-order numbers.
- **Output:** "name.txt" or "name.md" next to the file, plus copy to clipboard.
- **Result card:** "12,480 words, about 17,000 tokens, 2 columns detected, headers removed on 36 pages."
- **Edge cases:** garbled text from fonts with no ToUnicode map (falls back to OCR for those pages, says so); right-to-left and Indic text (logical order, shaped ligatures decoded to real characters); math and formulas (kept as Unicode or LaTeX only if the model is on, otherwise passed through); multi-column and sidebars (reading order by layout analysis); hyphenation; very large files (page by page output); scans (OCR route).
- **Quality bar:** on a labelled set of 50 mixed documents, reading-order edit distance beats pdfium's raw extraction and matches or beats the best open tools (Docling-class) we test; zero garbled output on files that have a valid ToUnicode; Indic and Arabic text extract to correct Unicode on the corpus's samples. Must beat the plain-text extraction of PDF24 and iLovePDF, and match BentoPDF, on structure.
- **Engine:** pdfium text, own layout analysis (columns, headers, tables). OS first, own model second. Optional layout model for tables and formulas: OS engine: none. Own model on every platform. On-device layout model, optional. OCR for scans: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback.
- **Surfaces:** bar ("pdf to text", "prepare pdf for ai"), right-click "Extract text", CLI `panoply pdf text in.pdf --format md --ai` (also `--pages 1-10`), MCP `pdf_extract_text` (and `pdf_prepare_for_ai`).

### 39. Extract tables (and Statements and invoices to Excel)
- **Pattern:** B. **Inputs:** one or many PDFs. Statements mode accepts bank statements, invoices and receipts.
- **Budgets:** standard, plus a table-structure model (OS engine: none, so it is used on every platform; target under 100 MB, size shown before the first download), used only for unruled and scanned tables, loaded on first use and unloaded after 60 s idle. OS OCR engines: no download, no resident memory for us. The fallback OCR model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Job:** "Get the table into Excel" and "turn this bank statement into rows I can import."
- **On drop:** finds tables on each page (ruled and unruled), counts and previews them. Detects a statement or invoice by keywords, a balance column and dates, and switches to Statements mode with a recognised template (bank layout, currency, date format, debit and credit columns).
- **Options:** Tables: all (default), chosen. Output: XLSX, CSV, or for statements: CSV, QBO, OFX, XLSX (default XLSX). Merge tables across pages: on when headers repeat. Header row: detected. Statements: date format, decimal separator and currency from the document and region; running balance check: on.
- **Preview:** the extracted grid above the page with the table outlined; for statements, a reconciliation line "opening 12,400.00 plus credits minus debits equals closing 9,812.45: matches".
- **Output:** "name (tables).xlsx" with one sheet per table, or "name (statement).csv".
- **Result card:** "4 tables, 212 rows. Statement balances reconcile, 0 rows need review."
- **Edge cases:** tables spanning pages with repeated headers (one table); merged cells; rotated tables; unruled tables (aligned by column gaps); scanned statements (OCR first, row confidence flagged); password-protected bank statements (ask once; Indian bank PDFs often use customer ID plus date of birth, so the field accepts it and never stores it); numbers with lakh and crore commas or European separators (parsed correctly, kept as numbers not text); multi-line descriptions (joined to the right row); debit and credit shown as Dr and Cr or minus signs.
- **Quality bar:** cell-level F1 of at least 0.95 on ruled tables and 0.85 on unruled tables over a labelled set of 100 real tables; on 50 bank statements, the balance reconciles on at least 95% of files, and any row that fails is flagged rather than guessed. Must beat iLovePDF and Smallpdf PDF to Excel on unruled tables and numeric parsing, and match Adobe Acrobat export, scored on the same tables.
- **Engine:** pdfium text and ruling lines, own table detector, template rules for statements. OS first, own model second. Table structure for unruled and scanned tables: OS engine: none. Own model on every platform. On-device table-structure model. OCR for scanned tables and statements: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback. Spreadsheet writing via rust_xlsxwriter.
- **Surfaces:** bar, panel, right-click "Tables to Excel", CLI `panoply pdf tables in.pdf -o out.xlsx` and `panoply pdf statement in.pdf --format qbo`, MCP `pdf_extract_tables`, `pdf_statement_to_rows`.

### 40. PDF/A
- **Pattern:** A, with a validation report. **Inputs:** one or many PDFs.
- **Budgets:** standard.
- **Job:** "Make this an archive-grade PDF/A for a court or a government portal."
- **On drop:** runs the PDF/A checks to see which parts are non-conforming: fonts not embedded, transparency (not allowed in A-1), encryption, JavaScript, external references, colour spaces without a profile, missing XMP. Chooses the target level from the user's saved portal choice; defaults to PDF/A-2b, and to A-1b if a portal demands it, A-3 when files are attached.
- **Options:** Level: PDF/A-1b, 2b (default), 2u, 3b. Colour profile: sRGB for RGB, a grey profile for grey, from the content. Fix: on (embed fonts, convert colour, flatten transparency, remove scripts). Also remove encryption: asks first.
- **Preview:** the compliance list: "Will fix 6 problems. Cannot fix 1: font Gotham has no embedding permission."
- **Output:** "name (PDF-A).pdf" with a validation report available ("passed 130 of 130 checks").
- **Result card:** "PDF/A-2b, validated, 5.1 MB, 6 problems fixed."
- **Edge cases:** fonts that cannot legally be embedded (the file cannot be conformant; says so and offers outline fonts); transparent content for A-1 (flattened, with a raster fallback); encrypted (decrypts, with a prompt); signed (conversion breaks the signature; warns); scanned pages with no text (conformant, but offers OCR); embedded attachments (only legal in A-3); large files (stream).
- **Quality bar:** every output passes veraPDF (the reference validator, which we run in CI, not ship) on at least 95% of the corpus and the portal-specific validators we name in the rule files for the courts and e-filing systems we support; unfixable cases are explained, not output as conformant. Must match iLovePDF and Smallpdf PDF/A and Adobe Acrobat preflight, with conformance verified by the oracle rather than claimed.
- **Engine:** own PDF/A fixer on lopdf and pdfium (fonts, colour, XMP), own rule list; veraPDF is used only as a test oracle in CI.
- **Surfaces:** bar ("pdf to pdf/a"), right-click "Convert to PDF/A", CLI `panoply pdf pdfa in.pdf --level 2b`, MCP `pdf_to_pdfa`.

## F. Convert, translate, ask, listen

### 41. Convert out (PDF to other formats)
- **Pattern:** A. **Inputs:** one or many PDFs. This is the PDF side of the Universal converter (CONVERSIONS.md); the routes live there, the PDF-specific behaviour is here.
- **Budgets:** standard for text and image targets. Scanned and layout-heavy routes use the OCR and optional layout models. OS engines: no download, no resident memory for us. The fallback OCR model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. The optional layout model (OS engine: none, so it is used on every platform) loads on first use and unloads after 60 s idle. DOCX, PPTX and XLSX are written as streamed zips, one page range at a time.
- **Job:** "Turn this PDF into Word, Excel, PowerPoint, images, text or something else."
- **On drop:** classifies each page as text, scanned, table-heavy or slide-like and picks the route. A scanned PDF goes through OCR first and says so. Ranks targets by the file: slide-like pages put PowerPoint first, table-heavy pages put Excel first, plain documents put Word first.

| Target | Default behaviour | Engine |
|---|---|---|
| Word (DOCX) | Rebuild flow: paragraphs, headings, lists, tables, columns, images in place; fallback to text boxes for hard layouts | own layout reconstruction, native DOCX writer |
| Excel (XLSX) | Same as Extract tables, one sheet per table, or per page if no tables | table module, rust_xlsxwriter |
| PowerPoint (PPTX) | One slide per page: real text boxes and images, background kept | own PPTX writer |
| Images (JPG, PNG, WebP, TIFF) | 150 DPI for screen-looking pages, 300 DPI for print-looking; one file per page or a ZIP | pdfium |
| Text, Markdown, HTML, JSON | As in Extract text | text module |
| EPUB | Reflowable from detected headings and paragraphs, cover from page 1 | own EPUB writer |
| ZIP of pages | One single-page PDF per page | lopdf |

- **Options:** target, then folded options per target: DPI and format for images, "layout fidelity" (Flowing text or Exact position) for Word, with the default chosen by whether the PDF is a designed layout or a text document; OCR language when scanned.
- **Preview:** the route ("PDF to OCR to DOCX, 34 pages") and a first-page render of the result for Word, PowerPoint and images.
- **Output:** one file per input in the target format, "name.docx" (never overwriting), pages as a folder or ZIP.
- **Result card:** what converted and what was approximated ("Table on page 6 kept as table. Fonts substituted: Gotham to Arial. 2 text boxes used for a sidebar.").
- **Edge cases:** scanned PDFs (OCR route, flagged as approximate); right-to-left documents (paragraph direction and alignment set, shaping kept); forms (fields become content controls in Word where possible, else text); math (Unicode text or image fallback, not wrong text); multi-column magazines (columns, not one interleaved stream); encrypted (password); 1,000-page files (stream, one section per page range); text on curved paths.
- **Quality bar:** PDF to Word is compared page by page against Adobe Acrobat's export and iLovePDF and Smallpdf on a 50-document corpus, scored on text accuracy, paragraph count and table cells (target: parity with Adobe on text accuracy, and at least as good as Smallpdf and iLovePDF on tables and columns); PDF to image is pixel-equal to pdfium's render at the chosen DPI; PPTX opens without a repair prompt in PowerPoint on the whole corpus.
- **Engine:** pdfium, own layout reconstruction and DOCX and PPTX writers, rust_xlsxwriter, own EPUB writer. No LibreOffice dependency on this side. OS first, own model second. Scans: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback. Optional layout model for scans: OS engine: none. Own model on every platform. On-device layout model, optional.
- **Surfaces:** bar ("pdf to word"), right-click "Convert to", phone share sheet, CLI `panoply convert in.pdf out.docx` (also `panoply pdf to docx`), MCP `convert` and `pdf_to_images`.

### 42. Convert in (other formats to PDF)
- **Pattern:** A. **Inputs:** one or many files, mixed types allowed, optionally merged into one PDF.
- **Budgets:** exception on peak memory and first preview, for Office, Pages and similar sources only. LibreOffice core (size shown before the first download) runs as a separate process started per job with an OS memory limit (target under 500 MB) and killed when the job ends, never kept warm. Cold start means the first-page preview appears when the first page is ready, not in 100 ms, and it never blocks the UI. Image, text and HTML sources are standard. The system web view used for HTML is created per job and destroyed after it.
- **Job:** "Make a PDF from this."
- **On drop:** identifies the true type from the bytes and picks the engine. For images, reads EXIF orientation and DPI; for Office files, reads page setup and fonts and checks which fonts are missing on this device.

| Source | Engine | Notes |
|---|---|---|
| JPG, PNG, WebP, HEIC, TIFF, BMP, GIF | own, native image decoders | Page size from image DPI or the region paper size; JPEG embedded as-is when no change, so no re-encode |
| Word, Excel, PowerPoint, ODT, ODS, ODP, RTF | LibreOffice core (MPL), downloaded on first use | Missing fonts mapped to metric-compatible ones, listed |
| HTML, Markdown, TXT, CSV, JSON | own HTML pipeline over the system web view's print path; Markdown via pulldown-cmark | Page size A4 or Letter by region, print stylesheet respected |
| EPUB, XPS, CBZ and CBR comics | own parsers, pdfium for XPS-like rendering | Comic pages kept as full-page images |
| Emails (EML, MSG) | mail-parser, own HTML render | Attachments embedded, or appended as pages (a choice) |
| PSD, DjVu | Rust PSD flatten, DjVu decoder | Flattened composite; DjVu text layer kept |
| Pages, Publisher, Visio, WordPerfect, Works | LibreOffice import filters (libetonyek, libmspub, libvisio, libwpd, libwps; all MPL), and Pages' embedded preview where the filter fails | Best effort, stated in the result |

- **Options:** Merge into one PDF: off for files of different kinds, on when dropping many images (default by detected type). Page size: from region; Fit: image fills page with margins (default), or exact-size page. Orientation: auto per image. Compress images: no (on request only).
- **Preview:** the route and page count, and a first-page render for Office files.
- **Output:** "name.pdf" per input, or "first-name (combined).pdf".
- **Result card:** "DOCX to PDF, 12 pages. Fonts substituted: Calibri to Carlito. Page count same as Word: 12."
- **Edge cases:** HEIC with rotation and depth data (rotation applied); huge images and mixed DPI (pages sized per image); images of documents with a side-on photo (offers the document scan module); Word with tracked changes or comments (final view by default, option to show markup); Excel with wide sheets (fit to one page wide, said); password-protected Office files (ask); macros (never run); emails with remote images (not fetched, local only); fonts missing (listed, never silent).
- **Quality bar:** images to PDF keeps JPEG bytes untouched (output size equals the sum of image bytes plus under 2% overhead); Office conversions are compared page by page (SSIM) against Microsoft Office output on a 100-document corpus, with page count equal on at least 95%; faster than uploading to iLovePDF or Smallpdf. Must match them on output and beat them on privacy; compare image-to-PDF size against theirs, since we embed JPEGs untouched.
- **Engine:** see the table. Never a server, never a cloud. LibreOffice is a downloaded separate process under MPL, not linked. It is started per job with an OS memory limit and killed when the job ends, never kept warm. The system web view used for HTML is created per job and destroyed after it.
- **Surfaces:** bar ("jpg to pdf", "word to pdf"), drop on window, right-click "Convert to PDF", phone share sheet, CLI `panoply convert in.docx out.pdf` and `panoply pdf from img1.jpg img2.jpg -o out.pdf`, MCP `convert` and `pdf_from_images`.

### 43. Translate PDF
- **Pattern:** B. **Inputs:** one PDF.
- **Budgets:** shares the engine and the exception of Translate text. OS engines: no download, no resident memory for us. ML Kit Translation manages its own per-language downloads. The fallback translation model is per language pair, about 100 MB class (size shown before the first download), peak under 600 MB including the runtime, downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. The 600 MB peak applies to the fallback path only.
- **Job:** "Translate this PDF and keep the layout."
- **On drop:** detects the source language per page from the text (or via OCR for scans), and chooses the target from the device language. Measures how much the layout will expand (German and Hindi run longer than English) and checks that the target-language font covers the script.
- **Options:** From: detected. To: device language. Layout: Keep layout (shrinks and wraps text to fit, default) or Text only. Glossary: optional terms to leave untranslated (names, brands, product codes, on by default for words in all caps and codes). Tone: formal or neutral (default neutral).
- **Preview:** a page before and after, and a list of the language pair model needed and its download size.
- **Output:** "name (hi).pdf" (language code in the name).
- **Result card:** "English to Hindi, 24 pages, 6,200 words, 14 text blocks shrunk to fit."
- **Edge cases:** right-to-left targets (Arabic, Urdu: block mirrored, alignment flipped, numerals per user choice); text in images (OCR and replace only if the module is on; otherwise left, said); tables (cell by cell); text that grows too long (shrink to a floor of 70% font size, then flagged); technical terms and code (kept); scanned pages (OCR, then rebuild); fonts for target scripts (embedded full font, e.g. Noto); quality of a rare language pair (shows the model quality label).
- **Quality bar:** measured on a labelled test set for each shipped pair (chrF and COMET scores reported in the model card, and a floor we set before shipping a pair); layout overflow on under 5% of blocks; original design visible outside text blocks (SSIM 1.0 outside text boxes). Must match iLovePDF and Smallpdf translate on layout fidelity, with on-device privacy as the difference; Google Translate document upload is the reference for quality.
- **Engine:** OS first, own model second. Translation: macOS and iOS: Apple Translation framework (TranslationSession). Android: ML Kit Translation. Windows: none found. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): small permissively licensed MT model, per-language-pair downloads. Source language detection: macOS and iOS: Apple NaturalLanguage NLLanguageRecognizer. Android: ML Kit Language Identification. Windows: none. Linux: fallback. Fallback: own language identifier. OCR for scanned pages: the OCR module's ladder (module 12): OS OCR first on macOS, iOS, Android and Windows, PaddleOCR-class fallback model (Apache-2.0) where the OS engine is absent or fails the quality bar test in CI, Linux: fallback. Own layout-fit code over the Edit text engine.
- **Surfaces:** panel, right-click "Translate PDF", CLI `panoply pdf translate in.pdf --to hi`, MCP `pdf_translate`.

### 44. Ask your PDF
- **Pattern:** B in shape: a chat panel next to the document, Return sends. **Inputs:** one or several PDFs.
- **Budgets:** standard for memory in our process, because we ship no language model: it runs in the OS engine or in the user's own service, not in ours. The former exception sized by device RAM is gone. OS engines: no download, no resident memory for us. The small embedding model (OS engine: none, so it is used on every platform; size shown before the first download) loads on first use and unloads after 60 s idle. The chunk index lives on disk, memory-mapped, and is deleted with the session.
- **Job:** "Summarise this, and answer my questions about it."
- **On drop:** extracts text (OCR if scanned), builds a local index of chunks with page references, and reads the first page to guess the document type (contract, paper, invoice, manual), which sets the starting prompts ("Key terms and dates", "What do I owe and when").
- **Options:** Task: Summary (default), Ask a question, Extract fields (a short list the user names, answered as a table). Length: short, medium. Model: the downloaded model, with a smaller and a larger choice by RAM (default chosen from device memory); it unloads 60 s after the last answer. Cite pages: always on.
- **Preview:** the summary with page citations that jump to the page; the model name and "ran on this device" line.
- **Output:** an answer in the panel, copyable as text, Markdown or a note saved next to the file ("name (summary).md"), on request.
- **Result card:** "Summary of 34 pages in 9 seconds, 5 citations."
- **Edge cases:** documents longer than the model's context (map-reduce over sections, said); scans (OCR); answers not in the document (says "not found in this PDF" rather than guessing); tables and numbers (shown with the page reference so the user checks); non-English documents (answers in the document's language unless asked); hostile instructions hidden in the PDF text ("ignore previous instructions": the PDF is data only, the model has no tools and no network); legal or medical documents (a short "not advice" line).
- **Quality bar:** on a labelled set of 100 questions across 20 documents, at least 90% of answers cite the right page, and the refusal rate on unanswerable questions is at least 90% (measured before launch); summary time for a 50-page document under 30 seconds on a laptop with the default model (target). The comparison is Smallpdf's and iLovePDF's AI features on answer quality, with on-device privacy as the difference.
- **Engine:** OS first, own model second. Answers, summaries and field extraction: macOS and iOS: Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices). Android: Gemini Nano through ML Kit GenAI APIs (Summarization, Proofreading, Rewriting, Prompt; beta, listed devices). Windows: Phi Silica (Copilot+ PCs only, being replaced by Aion Instruct). Linux and any device without these: the user's own Ollama, LM Studio or API key. Otherwise the feature is hidden. We ship no language model (DECISIONS.md), so there is no fallback model for answers. Retrieval embeddings: OS engine: none. Own model on every platform. Local embedding model for retrieval. Text comes from the Extract text module. The chunk index is a memory-mapped file on disk, not held in the heap.
- **Surfaces:** panel, bar ("summarise pdf"), right-click "Ask this PDF", CLI `panoply pdf ask in.pdf "What is the notice period?"` (also `panoply pdf summarize in.pdf`), MCP `pdf_ask` and `pdf_summarize`.

### 45. Read aloud
- **Pattern:** B in shape, a player. **Inputs:** one PDF.
- **Budgets:** standard. OS engines (system voices): no download, no resident memory for us. The fallback neural voice model (Kokoro class, quantized, under 300 MB peak) downloads on first use, loads when playback starts and unloads after 60 s idle, only on platforms where the quality bar test picks it. The 300 MB peak applies to the fallback path only. Audio is rendered in chunks.
- **Job:** "Read this PDF to me", or "turn it into an audio file."
- **On drop:** extracts text in reading order using the Extract text engine, drops headers, footers, page numbers and references lists, detects language per section, and picks a voice for that language. Scans go through OCR first.
- **Options:** Voice: by language, defaults to the best installed on-device voice (a system voice or the downloaded neural voice). Speed: 1.0x. Pages: all, range, from the page the user is on. Skip: headers, footers, references (on), tables (read as "table, skipping" by default). Export to audio: MP3 or M4A, off (play only).
- **Preview:** the page with the current sentence highlighted as it is read, and the time estimate for the whole file.
- **Output:** playback, or "name.mp3" next to the file with chapter marks at headings.
- **Result card:** "Audio file 1 h 12 min, 41 MB, 8 chapters."
- **Edge cases:** mixed language (voice switches by sentence, with a fallback voice); abbreviations, numbers, dates and currency (read correctly in the document's locale, e.g. lakh and crore); formulas (read as "formula" unless simple); hyphenation and column order (fixed by the layout engine); very long files (render in chunks, resumable); scanned pages (OCR route); right-to-left text; names and acronyms (pronunciation dictionary the user can add to).
- **Quality bar:** time to first audio under 2 seconds; a listening test on at least 20 documents with fewer than 1 mispronounced non-name word per 500 words (measured by listeners, target); export time under 0.2x real time on a laptop (target). Adobe's Read Out Loud is the comparison on voice quality and extraction order, with offline use as the difference.
- **Engine:** OS first, own model second. Speech output: macOS and iOS: AVSpeechSynthesizer. Android: Android TTS. Windows: SAPI and OneCore. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): open on-device neural TTS model (for example Kokoro, Apache-2.0), downloaded once. Extract text engine for the script.
- **Surfaces:** panel, bar ("read pdf aloud"), right-click "Read aloud", phone share sheet, CLI `panoply pdf speak in.pdf --voice en-1 -o out.mp3`, MCP `pdf_read_aloud`.
