# Recent PDF, document, OCR, e-sign, form, scanner and office utility launches (Oct 2025 to Oct 2026)

Researched 2026-10-13. About 45 searches plus about 40 page fetches. Read-only, no logins.

Method notes, read these first:
- The search tool did not return a single x.com or twitter.com post, even with site: filters and casual tweet phrasing. Reddit threads and Hacker News Show HN pages were also mostly absent. What surfaced instead: Product Hunt, Hunted.space (PH mirror), Indie Hackers, PeerPush, Peerlist, DEV, GitHub, Chrome Web Store, press (Gigazine, Korben, borncity, It's FOSS). Where a maker's X handle appears on a listing it is noted, but those are handles, not tweets.
- Product Hunt pages only show relative ages ("3mo ago"). I converted these to approximate months relative to today (2026-10-13), marked "~". Treat as plus or minus 1 to 2 months. Exact dates are given only where a page stated one.
- Engagement numbers are as shown on the page at fetch time. PH counts are unlabeled "points" and are tiny almost everywhere.
- "Local" means the maker claims in-browser or on-device processing. None of it is independently verified.

---

## A) Products

Columns: product | URL | date | launch or listing URL | engagement | local or upload | tools | angle

### Big, traction-bearing

| Product | URL | Date | Listing | Engagement | Local/upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| BentoPDF | https://www.bentopdf.com | Web launch mid 2025 (creator says 10k stars in under 3 months); desktop app first release Jul 2026; It's FOSS review 2026-10-10 | https://github.com/alam00000/bentopdf , https://itsfoss.com/bentopdf.md | GitHub 16.0k stars, 1.4k forks (fetched); self-host clones on Railway, Hostinger | Local (WASM, self-hostable) | 118 | The most complete client-side PDF suite. AGPL-3.0 or $79 lifetime commercial. Maker Alam, from India. Started as an internal merge/split tool |
| VERT.sh | https://vert.sh | Gigazine coverage 2025-10-14, ASCII 2025-12 | https://github.com/VERT-sh/VERT , https://gigazine.net/gsc_news/en/20251014-vert-sh | GitHub 15.9k stars, 847 forks | Local (WASM: ImageMagick, Pandoc, FFmpeg); video can hit a server | 250+ formats | Universal file converter, fully local, AGPL-3.0 |
| LuxPDF | https://luxpdf.com | HN item ~2025-10 (svelte mirror "11 months ago"); earlier HN post Aug 2025 is outside the window | https://hn.svelte.dev/item/45582199 , https://hn.svelte.dev/item/44772729 , https://github.com/VSRemoter/LuxPDF | Aug 2025 HN post: 117 points, 39 comments. GitHub 298 stars | Local | 38 implemented (README) | Student-built, no limits, donations only, AGPL. Commenters noted AI coding help |
| Stirling PDF 2.0 | https://github.com/Stirling-Tools/Stirling-PDF | 2025-12 | https://www.opensourceforu.com/2025/12/stirling-pdf-2-0-brings-text-editing-and-enterprise-tools-to-open-source/ | n/a | Self-host or desktop | 50+ | Not indie, but the incumbent moved to open-core licensing and added desktop apps and text editing, which opens room for alternatives |
| SimplePDF Copilot | https://simplepdf.com/copilot | ~2026-05-02 (aggregator, source listed as HN) | https://simplepdf.com/copilot | A secondary source claims 200k monthly users for SimplePDF overall (unverified) | Editor in browser; chat goes to an AI provider you pick | n/a | Chat with a PDF form and it fills fields; human handles signatures. MIT reference implementation |

### Browser PDF toolkits (the crowded "ilovepdf but private" lane)

| Product | URL | Date | Listing | Engagement | Local/upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| ihatepdf.cv | https://www.ihatepdf.cv | 2026-07-01 (also 2025-12-30 earlier launch) | https://www.producthunt.com/products/ihatepdf/launches/ihatepdf | PH shows "5 and 12" and "0 and 4" (unlabeled). Maker X: @pranavthedev_ | Local, offline | 46 to 60+ | Grew from editor to suite with AI, scanning, sharing, collaboration |
| OxygenPDF | https://oxygenpdf.com | 2026 (no date; launch price $29, was $79) | https://oxygenpdf.com | none shown | Local, but the Chat feature indexes on servers | 119 | Solo-built. Widest long-tail set (see section B) |
| PDFMergely | https://pdfmergely.com | 2026 (no date) | https://pdfmergely.com | none shown | Local; OCR engine downloads first run | 68 | Includes on-device Summarize/Ask/Translate, resume, invoice, letterhead makers |
| iCreatePDF | https://www.producthunt.com/p/icreatepdf | ~Jul 2026 | https://www.producthunt.com/p/icreatepdf/i-built-icreatepdf-a-100-local-privacy-first-pdf-utility-suite-with-46-tools | 0/2 unlabeled, no replies. Maker @barath0508 | Local, open source | 36 or 46 (listing says both) | "Turn off wifi, it still works" |
| Plume | https://www.producthunt.com/posts/plume-private-pdf-tools | ~Jul 2026 | same | 1 point, 7 comments (2 maker replies). Maker Wasif Abdullah | Local | 47+ | Adds invoice and barcode generator. Commenter asked for a CLI/batch mode |
| PixoPDF | https://www.producthunt.com/products/pixopdf | ~Jul 2026 | same | 9 points, ~14 comments. Maker P Ganesh Krishna Reddy | Local | 34 | A commenter asked if redaction truly removes text |
| CoolPDF | https://hunted.space/product/coolpdf | 2026 (no date) | same | 0 votes, 1 comment, #41 on its day | Local, AGPL-3.0 | 20 | HEIC to PDF, PDF to Markdown |
| PDFTasker | https://www.producthunt.com/products/pdftasker | ~Jun 2026 | same | 2 points | Local (WASM) | 16 | Maker notes big files get slow via IndexedDB |
| Only PDF Editor | https://www.producthunt.com/products/only-pdf-editor | ~Jul 2026 | same | 9 points, 13 comments. Maker Vikrant Kumar | Local (WASM) | ~16 (6 Pro: password, redact, OCR, batch, watermark, chat) | Freemium split: core free, privacy and AI tools Pro |
| SafePDF | https://www.producthunt.com/p/safepdf-3/safepdf-4 | ~Apr 2026 | same | 1/0. Maker Sujay Raj | Local | 4 | Edit, merge, compress, convert |
| itinypdf | https://www.producthunt.com/products/itinypdf | ~Mar 2026 | same | 2 points. Maker Ankit kumar singh (@unkeet) | Local | 35+ | WASM, works offline after load |
| PDFYogi | https://www.producthunt.com/products/pdfyogi-private-pdf-tools-in-browser | ~Oct 2025 (PH "12mo ago") | same | 24 points, 34 followers | Local | 20+ | Best PH score in this set |
| Privatools | https://www.hunted.space/product/privatools | 2026 (no date) | same | 4 votes, 1 comment, #120 | Local | 51 to 60+ | Mixed PDF, image, dev tools |
| EasyPDFNex | https://www.hunted.space/product/easypdfnex | 2026 (no date) | same | 0 votes, 1 comment | Local-first | 131+ | Biggest claimed count |
| NeedAnyTool | (name from article) | 2026-08-21 | https://borncity.com/news/pdf-tools-im-browser-needanytool-verarbeitet-dateien-lokal/ | press coverage | Local, Next.js | merge, split, rotate, compress, convert | Press coverage of an indie launch |
| ClientPDF | https://peerpush.com/p/localpdf (listing under @clientpdf) | 2026-04-18 | PeerPush | 2 upvotes | Local WASM | 4 | "Your PDFs. Your browser. Zero server uploads." |
| OffPDF | https://offpdf.com | 2026 (listed ~mid Sep per search metadata) | https://offpdf.com | none shown | Local desktop app (Win, Mac) plus browser tools, MIT | 23 (v0.4.0) | Open-source desktop for air-gapped use, OCR and Office conversion in the desktop build |
| OfflinePDF | https://offlinepdf.com | 2026 (no date) | https://offlinepdf.com | none shown | Local | 19 | Includes Scan ID to PDF and booklet |
| Aservus | https://www.indiehackers.com/product/aservus | updates 2026-03 and 04 | same | listed on SourceForge, SAAShub, PH, Capterra (no numbers) | Local | 23 | Distribution-by-directory playbook |
| ImagePDF.Tools | https://www.indiehackers.com/product/imagepdf-tools | 2026-08-15 | same | none shown | Local where possible | n/a | PDF plus image utilities |
| LocalPDF (extension) | https://addons.mozilla.org/addon/localpdf/ | Chrome release 2025-10-10 | https://www.producthunt.com/@localpdf | PH 0/1 and 0/6 unlabeled | Local | 12+ | Browser extension, open source, no limits |
| Offline PDF Tools (extension) | https://chromewebstore.google.com/detail/lkokanmnglecjkgabbhincgaiceedolc | v1.1.0 2026-06-05 | https://hunted.space/product/offline-pdf-tools | none | Local | merge, split, compress, protect, repair, metadata, watermark | Chrome extension |
| EzyPDF and PDF Total Privacy (extensions) | https://chromeboard.com/extension/ezypdf-edit-sign-convert-nllakednhpmnopbibfjjddcecgncddnj | 2026-03-14 and 2026-03-02 | chromeboard.com | EzyPDF 49 users | Local | EzyPDF: sign, OCR, edit. Total Privacy: 30 features | Extension long tail |
| PDF-Vert, PDFCrate, PDFQuokka, Docexp, Techsinghe, PDFCraft, HamaraPDF, MoguanPDF, PDF0, PDFTara, ZeroCloudPDF, Pyrelo, PDFOnlineLovePDF | PH / Hunted / Peerlist (see search results) | 2026, no exact dates | e.g. https://www.hunted.space/product/pdfcrate , https://peerlist.io/srbhoi2012/project/hamarapdf | 0 to 4 votes where shown | Mostly local. PDF-Vert uploads (files deleted in 2h). GoToolsOnline uploads (FastAPI on one VPS) | 9 to 163+ (PDFCrate 35, PDFQuokka 17, Docexp 113, HamaraPDF 54+, MoguanPDF 24, PDFTara 99+, Pyrelo 40+, PDFOnlineLovePDF 31, GoToolsOnline 163+) | PDFCrate sells an AI layer from $4.99/month on a free core. India-built: HamaraPDF, PDFTara, ZeroCloudPDF, Pyrelo, PDFOnlineLovePDF, GoToolsOnline |

### Redaction, privacy

| Product | URL | Date | Listing | Engagement | Local/upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| RedactVault | https://www.producthunt.com/products/redactvault | ~Jul 2026 | same | 3 points, 10 comments | Local | redaction with PII auto-detect | Built with Hugging Face model and Claude Code |
| KeptPDF | https://www.producthunt.com/products/keptpdf | ~Jun 2026 | same | 4 points. Maker John Whitworth | Local | redact + 24 more (sign, OCR, compare, Bates, fill, extract tables) | "Privacy X-ray" finds live text under failed redactions; SHA-256 redaction certificate; AI anonymizer |
| RedactLocal | (PH profile) | 2026-08 | https://www.producthunt.com/@daniyal14 | none seen | Zero network | redact | Rasterizes every page and self-checks 0 selectable characters and 0 fonts |
| Offline PDF Redactor | https://peerpush.com/p/offline-pdf-redactor | 2026-06-17 | PeerPush | n/a | Local | redact | Review each redaction before export |
| PlainBytes PDF Redactor, OfflinePDF redactor | https://hunted.space/product/plainbytes-pdf-redactor | 2026 | Hunted | 1 and 2 votes | Local | redact | AI plus rule matching |

### E-sign, forms, compare, OCR, convert, scan, office

| Product | URL | Date | Listing | Engagement | Local/upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| Firmalo | https://www.producthunt.com/p/firmalo/firmalo | ~Jul 2026 | same | 2 and 6 unlabeled. Maker Kirill Sirenko | Local | sign | Spanish and Portuguese, no signup, says it is not a DocuSign replacement |
| Serverless PDF editing + digital signature certificates | https://www.producthunt.com/p/self-promotion/i-have-created-a-100-free-tool-serverless-for-pdf-editing-and-digital-signatures-certificates | ~Jul 2026 | same | 0/1. Maker Mirza Munawar | Local (JS + WASM) | sign (draw, type, upload), merge, compress, rotate, protect | Claims ESIGN and eIDAS validity (marketing claim) |
| SignifyPDF | https://www.indiehackers.com/product/signifypdf | 2026 (no date) | same | commenter noted local-only cannot serve automated signing | Local, open source | sign | Pure signing, no accounts or watermark |
| DesiDocs.in | https://www.producthunt.com/products/desidocs | ~Jun 2026 | same | 2 points, 5 followers. Maker X: @desi_docs | Local | edit, organize, merge, sign, attest, secure | Aadhaar and Form 16 password removal, "attest" copies |
| Pdf2Fill | https://www.indiehackers.com/product/pdf2fill | 2026-07 | same | none | Not stated | detect fields on flat PDFs | Geometry-based field detection (lines, checkboxes, character cells) |
| PDF Diff (James Montemagno) | https://montemagno.com/introducing-pdf-diff-compare-pdfs-privately-in-your-browser/ | 2025-12-31 | same | none (asks for GitHub stars) | Local, offline after load | compare | 5 view modes, exports a PDF report |
| PdfCompare.app | https://www.indiehackers.com/post/compare-pdfs-in-seconds-without-uploading-your-files-5275804665 | 2026-09-06 | same | none | Local WASM | compare | "Saves $239/yr vs Acrobat Pro" claim |
| CompareDiff | https://peerpush.com/p/comparediff | 2026-08-19 | PeerPush | n/a | browser | compare text, JSON, images, PDFs | |
| pdfmarkdown.app | https://pdfmarkdown.app | ~Jul 2026 | https://www.producthunt.com/posts/pdfmarkdown-app | 0 points, 3 comments, 9 followers. Maker @jerome2000 | Local incl. OCR | PDF to Markdown | Tables, formulas, reading order, synced side-by-side, Obsidian bundle. Batch requested |
| ocr4you | https://www.producthunt.com/products/ocr4you/launches | 2026-06-14 and 2026-06-20 | same | 1 point, 2 comments | Not stated | OCR | Hindi and Indian-language OCR, invoices |
| imgRead | https://www.hunted.space/product/imgread | 2026 (no date) | same | few votes | Local claim | handwriting to text, table extractor to Excel, receipt reader | |
| Extrako | https://www.indiehackers.com/product/extrako | 2026-03 | same | none | Hosted (upload) | invoices, receipts, handwriting to Excel/Word/PDF | Multi-stage AI extraction; 99% accuracy is the founder's claim |
| EditDocx | https://www.indiehackers.com/product/editdocx | 2026-08 | same | none | Local (IndexedDB), built on eigenpal/docx-editor | DOCX, XLSX, PPTX edit, PDF view and fill | Browser office editor without cloud |
| docling.rs WASM | https://docs.rs/crate/docling | 2026 (no date) | https://docs.rs/crate/docling | none | Local, 3.4 MB gzip | DOCX, HTML, XLSX, PPTX, EPUB to Markdown, JSON, DocLang | Rust port of docling in a browser |
| Clearcan | https://www.producthunt.com/products/clearcan/makers | ~Jul 2026 | same | 14 points. Maker Ion Hortolomei | Local (iPhone) | scanner: edge detect, on-device text search (EN, RU, RO), copy text, PDF export | No ads, subs, cloud, accounts, analytics |
| ProScan, OpenScan, FairScan | https://f-droid.org (FairScan) | date not confirmed | https://discuss.privacyguides.net/t/fairscan-a-simple-and-respectful-document-scanner-for-android/31915 | n/a | Local | scanners | FairScan is GPLv3 on F-Droid and Play |
| EPUB Creator, Epub Reader Pro, ReadHere | https://addons.mozilla.org/addon/epub-creator/ | v1.3 2026-06-23 | https://www.indiehackers.com/product/epub-reader-pro | n/a | Local | web page to EPUB, EPUB reader, EPUB to PDF | Small ebook long tail |
| Resume Builder by Shiva Kar, MyPDFCV, Visual Typst Resume Builder | https://www.producthunt.com/products/resume-builder-by-shiva-kar | 2026 | same | few | Local | resume to PDF (Typst, ATS-friendly) | |
| GSTInvoiceFree, Billaze | https://hunted.space/product/gstinvoicefree | Billaze 2026-06-15 | PeerPush / Hunted | few | Local (GSTInvoiceFree) | invoices with CGST, SGST, IGST | India GST |

---

## B) Tasks offered that are NOT in MODULES.md

Sources are the BentoPDF, OxygenPDF and PDFMergely tool pages plus individual launches. One line each, deduplicated.

PDF
- Digital (certificate-based) signing, signature validation, and trusted timestamping (BentoPDF, OxygenPDF "Cert Cryptor"). MODULES has visual Sign only.
- Remove an existing digital signature (PDFMergely).
- Request signatures from other people with an audit trail (DocuSeal, Documenso, OpenSign class; hosted by nature).
- Overlay one PDF on another (letterhead or template underlay).
- Reverse page order.
- Rotate by any angle (not just 90 degrees).
- Page labels (roman numerals, custom prefixes) as distinct from page numbers.
- Generate a printed table of contents page.
- Stamps (Approved, Draft, Confidential) as a library.
- Remove a watermark.
- Adjust brightness, contrast and colors on a PDF; scanner effect (make a clean PDF look scanned).
- Enhance handwriting ink; e-ink optimizer; PDF reflow for small screens.
- Page size inspector and fix oversized or mixed page sizes.
- PDF health check and dead link debugger; citation linker.
- Smart split a scanned batch into separate documents by detected boundaries.
- Export annotations and comments as a list or file.
- Barcode stamp on pages and bulk barcode generator.
- Form logic and calculated fields; convert a PDF to a web form; signature anchor helper.
- Detect text hiding under fake redactions; redaction certificate with a hash (KeptPDF).
- Clean or "prepare for AI" a PDF (strip headers, noise) (BentoPDF, PDFMergely).
- Presenter mode, PDF as slides; PDF slides to video (Slide2Video Pro).
- PDF to SVG, to CBZ, to TIFF and BMP.
- Self-attest stamp on copies (DesiDocs); unlock Aadhaar or Form 16 PDFs using their known password pattern (DesiDocs).
- Speed reader (RSVP) and built-in PDF reader (OxygenPDF).

Documents and ebooks
- Legacy formats to PDF: WordPerfect (WPD), Works (WPS), Publisher (PUB), Visio (VSD), Apple Pages.
- Web article to EPUB (EPUB Creator extension).
- EPUB and PDF reader inside the app (Epub Reader Pro, ReadHere).
- Structured extraction from DOCX, XLSX, PPTX, EPUB to Markdown, JSON or DocLang (docling.rs).
- Browser DOCX/XLSX/PPTX editing, not just conversion (EditDocx).
- Handwriting to text (imgRead); Indian-language OCR presets such as Hindi (ocr4you).
- Invoice, receipt and bank statement to Excel as structured extraction (Extrako, imgRead, PDFCrate AI).

Business and printables
- Resume, CV and biodata builder with ATS checker (PDFMergely, Shiva Kar, MyPDFCV, PDF&Resume).
- Letterhead maker, business card maker, signature generator.
- Printable templates: graph paper, lined paper (OxygenPDF).
- Chat-to-fill for PDF forms (SimplePDF Copilot).

---

## C) Standout traction and why

Numbers below are the only ones I could verify on a page. Everything else I saw is 0 to 24 on Product Hunt.

- BentoPDF: 16.0k GitHub stars and 1.4k forks on the repo page (https://github.com/alam00000/bentopdf). The creator's own post says 10k stars in under 3 months. Why: a single "everything, offline, self-hostable" pitch with a 118-tool surface, AGPL plus a $79 lifetime license, and a Reddit r/webdev launch. It is also being re-reviewed in the press (It's FOSS, 2026-10-10) and has a desktop app (Jul 2026).
- VERT.sh: 15.9k stars and 847 forks (https://github.com/VERT-sh/VERT). Why: it generalizes the same "nothing leaves the device" idea to 250+ formats via WASM builds of ImageMagick, Pandoc and FFmpeg, and got Gigazine coverage in Oct 2025.
- LuxPDF: HN post "Got tired of bad PDF WebApp so we made a Free, Open-Sourced, Private Alternative" at 117 points and 39 comments (https://hn.svelte.dev/item/44772729). That post is Aug 2025, just before the window, but the same author posted again around Oct 2025 (https://hn.svelte.dev/item/45582199), and Korben covered it. GitHub stars are only 298, so HN attention did not convert to stars. Why it worked on HN: student founders, no limits, donation funded.
- PDFYogi: 24 points and 34 followers on Product Hunt, the top PH score I saw in this category. Others are at 1 to 14 (Clearcan 14, PixoPDF 9, Only PDF Editor 9).
- The pattern: the two things that moved numbers were open source with a self-host story (BentoPDF, VERT) and a distribution event on HN or Reddit. Dozens of identical "no upload" browser suites on PH sit at 0 to 4 votes. Tool count is not the differentiator (49 to 163+ claimed counts, same zero traction). Differentiated wedges that got attention or comments: true redaction proofs (KeptPDF, RedactVault, RedactLocal), Indian-specific tasks (DesiDocs, ocr4you, GST invoices), PDF to Markdown for notes/AI (pdfmarkdown.app), and AI form fill (SimplePDF Copilot).
- Gap I did not close: I could not read X itself, so launch tweets and their like/repost counts are not captured. Maker X handles seen on listings: @pranavthedev_ (ihatepdf.cv), @barath0508 (iCreatePDF), @unkeet (itinypdf), @jerome2000 (pdfmarkdown.app), @desi_docs (DesiDocs), @redactvault, @machox (a PH maker of a merge/split/compress tool). Searching these directly on X is the next step.
