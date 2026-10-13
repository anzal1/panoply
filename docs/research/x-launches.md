# Indie file converter and utility launches on X, 2025 to 2026

Harvested 2026-10-13. Method: web search (X is poorly indexed), X syndication endpoint (public, read-only, gives likes, replies and, for parents, reposts), X post and profile pages in the built-in browser without logging in (these show view counts), and each product's own site. Nothing was posted, liked, followed or clicked.

Honesty notes:
- X search needs a login and the nitter mirrors are dead or blocked (xcancel returns 451 or is suspended, nitter.net is offline), so I could not enumerate launches by X search. The list below is what search engines, profile pages and product directories surfaced. It is a sample, not a census.
- View counts are only reported where the X page showed them. Where a number came from a search snippet it is marked "(snippet)".
- Counts of likes or reposts are as of 2026-10-13 unless a column says otherwise. Product claims (no upload, tool counts) are the maker's own unless I noted a contradiction.

## A) Table

| product | URL | launch date | launch post URL | engagement | local or upload | tools count | notable angle |
|---|---|---|---|---|---|---|---|
| ihatefiles | https://ihatefiles.com | 2026-09-06 | https://x.com/pranvtwt/status/2096600379131933121 | 80.2k views, 1.1k likes, 75 reposts, 121 replies, 529 bookmarks (X page) | local (ffmpeg.wasm and browser codecs, "no upload endpoint", works offline) | 53 tools, 63 formats, 969 conversions (site; the post says "64+ formats") | Sequel from an existing hit ("the next product from the ihate club"), same pain across images, video, audio, free, 100% client-side. Creator already had a PDF audience. |
| ihatepdf | https://ihatepdf.cv | first tryout post 2025-12-24, DevHunt launch 2026-01-13 | https://x.com/pranvtwt/status/2003838935823507803 (early "Tryout" post) | 20 likes, 2 replies (syndication). Product Hunt: 12 upvotes, 5 comments, #26 of the day. Creator's own claim in the pinned 2026-10-06 post: 300K+ users, $500+ revenue (https://x.com/pranvtwt/status/2107474001862025407, 56k views, 436 likes, 44 replies) | local (JS and WASM, offline after first load). Caveat: its Chat with PDF sends text to Google Gemini with the user's key | 46 listed (site header says 60+) | Started tiny (20 likes), grew by search and word of mouth. The slow-burn then fed the ihatefiles launch. "No watermark, no upload, free forever". Also India business tools (GST invoice, POS billing). |
| PDFduck | https://pdfduck.app | 2026-04-21 | https://x.com/pdfduck/status/2046689548890001857 | 1 like, 0 replies (syndication). Views not read | mixed. Images in browser. Word, Excel and PowerPoint are converted on its server with LibreOffice, then deleted (site's own text). The launch post says "everything runs in your browser" | 8 | Plain checklist post (no uploads, no sign-up, no watermark, mobile and desktop). Near zero engagement. Contradiction between post and site is a trust risk. |
| Haven PDF | https://havenpdf.com | 2026-07-21 | https://x.com/havenpdf/status/2079521493747138753 | 0 likes, 0 replies (syndication). Product Hunt: 3 upvotes, 3 comments, #155 | local, PWA, works offline, OCR local (maker's claim) | 24 per the launch post | "I built X" solo post. Follow-up posts with a pain hook ("Attachment must be under 200 KB", target-size compress). No traction visible. |
| PDFLocal | http://pdflocal.site | 2026-01-18 | https://x.com/Rock4754/status/2012866301077901624 | 16 likes, 2 replies (syndication) | local (maker's claim) | not retrievable (site did not render for fetch) | "Complete PDF toolkit, no cloud processing", free built-in OCR as headline feature. |
| VidShift | https://vidshift.io | 2026-02-23 | https://x.com/jasongillyon/status/2025983554409157094 | 1 like, 1 reply (syndication); 213 views (snippet) | local (WASM, built on MediaBunny) | not retrievable (domain fetch blocked) | "My first project with Claude Code, I did not write a line". Video toolkit. Low reach. |
| VERT (vert.sh) | https://vert.sh | posts from 2025-07; GitHub amplification 2025-11-26 | https://x.com/github/status/1993662391276441681 (third-party post by @github) | 3,740 likes, 45 replies on the @github post. Other posts: https://x.com/ErickSky/status/2057201436815954128 (194 likes, 2026-05-20), https://x.com/sivalabs/status/1992871515222950148 (61 likes). Repo about 15.9k stars (GitHub) | local for images, audio, documents (WASM). Video can use the maker's server unless you self-host the vertd daemon | 250+ formats across image, audio, document, video | "Stop uploading your sensitive files to random websites." Fear-of-upload framing posted by a big account, then open source, no limits, no ads. Biggest reach in the set. |
| PaperKnife | search surfaced a third-party post only | 2026-04-24 (post) | https://x.com/Himanshu_tweaks/status/2047571350555185190 | 732 likes, 10 replies (syndication; a discovery post, not the maker's launch) | local, "100% offline", on F-Droid as com.paperknifeplus.app | not retrieved | "Just found an alternative to ILovePDF." Discovery post with a named incumbent and a bank-statement, ID, contract hook. |
| PDFCraft | https://github.com/PDFCraftTool/pdfcraft | repo posts 2026-01 to 2026-07 | https://x.com/simplifyinAI/status/2077948353401524551 (third-party, 2026-07-17) | 169 likes, 3 replies. https://x.com/tom_doerr/status/2011069281371308220 142 likes (2026-01-13). Repo 9.1k stars | local (WASM), AGPL-3.0 | 93 listed in README (headings say 95, text says 90+) | Open source, 90+ tools, "without uploading them anywhere". Spread through curator accounts, not the maker. |
| Convry Tools | https://www.ahhhhfs.com/78548/ (write-up) | 2025-12-22 | https://x.com/abskoop/status/2003122932202258805 | 453 likes, 10 replies (syndication, Chinese curator post) | not verified | PDF merge, image edit, audio to text (headline only) | "One page covers 90% of daily tool needs". Chinese curation account. |
| Generic PDF privacy explainer | not a launch | 2026-04-21 | https://x.com/heynavtoor/status/2046509090084626941 | 2,464 likes, 85 replies (syndication) | n/a | n/a | Second-person story ("you uploaded your tax return to a site you never heard of"). The tool linked is truncated in the data I saw, so I did not attribute it. Shows how well the privacy-fear hook performs. |
| AlternativeTo feature | AlternativeTo post | 2026-04-08 | https://x.com/AlternativeTo/status/2041917517103567256 | 202 likes, 1 reply (syndication); 7.5K views (snippet) | local, open source, no account | video and image compressor, SVG conversion, batch, GIF export | The product name was not in the post text I could read. |
| Small tail from directories (no X launch post found) | Pixore (Indie Hackers 2026-06-28: 9 converters, 33 languages), FormatDog (32 tools), PDFQuokka (17), ConvertLocal, Fast-Vid, FormatDrop (80+ formats), Konvertus, Convertie.org, Private Convert (30+ pairs), LocalConvert (Show HN 2026-03-09), Tools Root (70+), ProBatchResizer (130+ presets), EasyFreePDF (25), iLoveFastPDF | 2026 | none located | Product Hunt boards show 0 to 12 upvotes for these; no X numbers | all claim local | as listed | Crowded long tail, almost no reach. |

Same-name collisions to avoid mixing up: ihatepdf.world, ihatepdf.site (Threads account @ihate_pdf), github.com/Burthcer/IHatePDF, AnkitNayak-dev/ihatepdf (a joke "PDF destroyer"), cu-sanjay/iHatePDF. These are not Pranav Mailarpawar's product.

## B) Union of tools and tasks offered, deduplicated

Sources fully enumerated: ihatefiles (53), ihatepdf.cv (46), PDFCraft (93), PDFduck (8), VERT (format families only). Haven PDF, PaperKnife, PDFLocal, VidShift lists were not retrievable.

**PDF: organize and pages**
Merge, split, extract pages, organize, delete pages, rotate (also by custom degrees), reverse pages, add blank page, divide pages (cut each page in two), N-up, combine to single page, alternate merge (interleave odd and even scans), grid combine, booklet, posterize, crop and resize, fix page size, page dimensions viewer, remove blank pages, deskew, PDF multi tool, PDF reader.

**PDF: edit and annotate**
Edit text, edit PDF, sign, annotate, stamps, watermark, page numbers, headers and footers, table of contents, edit bookmarks, form filler, form creator, background color, change text color, invert colors, remove annotations, OCG (layer) manager, add attachments, extract attachments, edit attachments.

**PDF: security and cleanup**
Encrypt, decrypt or remove password, remove restrictions, change permissions, redact, flatten, sanitize (strip scripts and embedded content), remove metadata, view and edit metadata, privacy risk scanner, fingerprint PDF (per-recipient tracing mark).

**PDF: optimize and repair**
Compress (also to an exact target size), linearize, repair, rasterize, font to outline, PDF/A, greyscale.

**PDF: convert in**
Image, JPG, PNG, WebP, SVG, BMP, HEIC, TIFF, PSD to PDF; TXT, JSON, CSV, Markdown, HTML, RTF, XPS, Email to PDF; Word, Excel, PowerPoint to PDF; EPUB, MOBI, eBook, CBZ, DjVu to PDF; audio to PDF.

**PDF: convert out**
JPG, PNG, WebP, BMP, TIFF, greyscale, JSON, DOCX, PowerPoint, Excel, HTML, EPUB, audio, ZIP; extract images, extract text, extract tables, OCR (searchable).

**PDF: AI and compare**
Chat with PDF, summarizer, compare PDFs.

**Images**
Convert (many formats), HEIC to JPG, compress, resize, crop, rotate and flip, adjust brightness and contrast, remove background, image to text (OCR), watermark, remove metadata, favicon and icon set, images to GIF, find the smallest format ("format shootout"), auto-optimize for the web, batch resize with 130+ social and store presets (ProBatchResizer).

**Video**
Convert, compress, resize, crop, trim, split, join, change speed, reverse, boomerang, mute, replace or add audio, extract audio, video to GIF, GIF to video, extract frames, contact sheet, watermark, burn in subtitles, generate subtitles, extract subtitles from MKV, convert and sync subtitles, unpack MKV or MP4 streams, repair broken video, restore old video, tidy a screen recording.

**Audio**
Convert, trim (MP3 cutter), split, join, fade in and out, change volume, normalize, remove silence, stereo to mono, remove background noise (clean voice), remove vocals, split into stems.

**Cross-cutting and business**
Offline mode, P2P share, collaborative whiteboard, scan to PDF, GST invoice generator, POS billing, workflow builder, platform size-limit reference pages (e.g. "compress video to 10 MB"), format explainers and per-pair conversion landing pages (969 on ihatefiles).

## C) Tasks from B that are NOT in MODULES.md

Checked against /Users/anzalhussainabidi/personal/panoply/MODULES.md (read first). Items already present, such as merge, split, OCR, redact, compress to size, subtitles, vocal remover, stems, noise removal, boomerang, favicon, GIF tools, extract frames, contact sheet and Ask your PDF, are excluded.

Clear gaps (no equivalent line in the catalogue):
1. Stereo to mono (audio).
2. Subtitle file convert and sync (SRT, VTT, ASS conversion and time shift). The catalogue only has generate, translate and burn in or export SRT.
3. Extract subtitle tracks from MKV, and more generally unpack or demux streams from MKV and MP4 into separate files.
4. Repair a broken or truncated video (the catalogue has PDF Repair only).
5. Restore old video (denoise, deinterlace, colorize, upscale). The catalogue has Restore old photo only.
6. Tidy a screen recording (cut dead time, idle sections, pauses). Remove silence exists for audio only.
7. Image format shootout (encode one image in every format and show the smallest at equal quality).
8. One-click "optimize for web" batch preset for images (pick format, size and quality for a site).
9. Per-platform preset library for batch image resize at scale (130+ presets). The catalogue has social presets, so treat this as a depth gap.
10. PDF: alternate merge (interleave odd and even scan batches), divide pages (split each page into two), combine to single page, grid combine, reverse page order (Organize may cover reverse implicitly, not stated).
11. PDF: remove blank pages automatically, deskew scanned pages.
12. PDF: add or extract embedded file attachments.
13. PDF: sanitize (strip JavaScript, embedded files, hidden layers), OCG layer manager, linearize for fast web view, rasterize, font to outline, remove annotations only, remove restrictions, change permissions in isolation, background and text color change, invert colors, stamps (approved, draft), table of contents generation (Bookmarks says auto-generate from headings, so partial).
14. PDF: per-recipient fingerprint or canary marking to trace leaks.
15. PDF: privacy risk scanner that reports hidden data before sharing (Clean before sharing strips it but there is no scan-and-report step).
16. PDF to ZIP, PDF to JSON, JSON to PDF, CSV to PDF, audio to PDF (transcript as a document), PDF to audio is covered by Read aloud only if it exports a file; the catalogue does not say it does.
17. Niche to-PDF inputs: PSD, XPS, RTF, MOBI, CBZ, DjVu, TIFF multi-page, BMP.
18. Collaborative whiteboard (peer to peer).
19. Business templates that Indian users search for: GST invoice generator, POS billing. The catalogue has a GST calculator under Tax but no invoice generator.
20. Reference pages as a feature: platform file size limits ("Discord 10 MB, WhatsApp 16 MB") and per-pair conversion explainers. Not a task but a discovery surface; the catalogue has size-target compress with named platforms, not a limits reference.

Partial overlaps worth a second look: "Privacy Risk Scanner" vs Clean before sharing; "tidy recording" vs Screen recording; "restore old video" vs Restore old photo.

## D) Launch angles that drew the most engagement

Ranked by what the numbers showed (all counts above):

1. Fear of uploading private files, stated as a second-person story or imperative. @heynavtoor post, 2,464 likes (2026-04-21), and "Stop uploading your sensitive files to random websites" from @github on VERT, 3,740 likes. The same hook on tool-maker accounts with no audience (PDFduck, Haven PDF) drew 0 to 1 likes, so the hook amplifies an audience, it does not create one.
2. A sequel from a maker with an existing audience. ihatefiles drew 80.2k views, 1.1k likes, 75 reposts and 529 bookmarks in one post, with replies like "investing in this asapp" and "this guy is always on fire". The ihatepdf tryout post had 20 likes nine months earlier. Distribution came from the maker's track record and numbers posted in public ("300K+ users"), not from the first launch.
3. A named incumbent plus "alternative". "Just found an alternative to ILovePDF" (PaperKnife) 732 likes; "Todos amamos ILovePDF, pero esto es OTRA COSA" (VERT) 194 likes.
4. Curators and lists beating the maker. PDFCraft's reach came from @simplifyinAI (169) and @tom_doerr (142), not the maker. Open source plus a star count is shareable by third parties.
5. Breadth stated as numbers: "64+ formats", "90+ tools", "250+ formats". Present in every post that worked.
6. Weak performers: plain feature checklists from new accounts (PDFduck 1 like), "I built X, 24 tools" from a zero-follower account (Haven PDF 0 likes), and "my first project with Claude Code" (VidShift 1 like).
7. Builds-in-public numbers keep working after launch. The 2026-10-06 monthly update got 56k views and 436 likes and states ihatefiles at 5K+ users one month in, which is the honest scale of a post that got 80k views.

Implication for Panoply: the privacy hook and breadth numbers are table stakes; the 80k-view result came from an existing audience plus a named category sequel, so a launch needs an audience-first plan, not just the claim.

## Sources
- https://x.com/pranvtwt/status/2096600379131933121 (ihatefiles launch, views and counts read in browser)
- https://x.com/pranvtwt/status/2107474001862025407 (monthly update)
- https://x.com/pranvtwt/status/2003838935823507803 (ihatepdf tryout)
- https://x.com/VazeKshitij/status/2096603497571721619 and https://x.com/imYadav31/status/2096637047020912935 (replies)
- https://ihatefiles.com , https://ihatepdf.cv , https://pdfduck.app , https://github.com/PDFCraftTool/pdfcraft , https://github.com/VERT-sh/VERT
- https://hunted.space/product/ihatepdf/launches/ihatepdf , https://devhunt.org/tool/ihatepdf
- https://x.com/pdfduck , https://x.com/havenpdf , https://x.com/Rock4754/status/2012866301077901624 , https://x.com/jasongillyon/status/2025983554409157094
- https://x.com/github/status/1993662391276441681 , https://x.com/ErickSky/status/2057201436815954128 , https://x.com/Himanshu_tweaks/status/2047571350555185190 , https://x.com/simplifyinAI/status/2077948353401524551 , https://x.com/tom_doerr/status/2011069281371308220 , https://x.com/abskoop/status/2003122932202258805 , https://x.com/heynavtoor/status/2046509090084626941 , https://x.com/AlternativeTo/status/2041917517103567256
