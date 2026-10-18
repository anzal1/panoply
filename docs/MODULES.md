# Panoply modules

The promise: whatever you need to do to a file, a photo, some text or your own
device, it is one click away in Panoply, and nothing leaves your device.

How "one click" works everywhere:

- **Drop anything.** Drag a file onto the bar, the app, or right-click it in
  Finder or Explorer. Panoply knows the type and shows the tools that fit, best
  first, learned from what you use.
- **Say what you want.** "passport photo", "make this under 50 kb", "find the
  screenshot with the wifi password". Plain words map to a module and its
  options.
- **Chain it.** Any sequence of modules saves as one action you can bind to a
  shortcut, a right-click item or a watched folder.
- **Batch it.** Every module takes one file or a thousand.
- **Gesture it.** Hold Shift while dragging a file anywhere to drop it into a
  converter, or hover the notch (Dynamic Island area on Macs with a notch) to
  drop files into Panoply's tray. Converters that launched on one gesture
  alone (Tangerine, convt) drew 300K to 400K views on X in October 2026.

Legend: ⚡ pure Rust, instant · 🧠 needs a local model, downloaded once on first
use, then fully offline · 🌐 needs the internet by nature, opt-in, clearly
labelled · 📱 mobile-first

---

## Ships first: the top 30 by demand

Ranked by search volume and usage evidence (research/tool-harvest.md,
Semrush and Ahrefs-style snapshots, Sep 2026). Search figures are monthly.
iLovePDF's biggest markets are Bangladesh and India, so the South Asian form
tasks are in the first wave.

| # | Module | Evidence |
|---|---|---|
| 1 | JPG and images to PDF | about 4.8M global, 246K Bangladesh |
| 2 | PDF to Word | about 4.0M global |
| 3 | PDF to JPG | about 3.4M global |
| 4 | Word to PDF | about 2.6M global |
| 5 | Merge PDF | about 1.7M global |
| 6 | Compress PDF | about 1.6M global, #2 in browser-tool usage logs |
| 7 | Resize image | 1.22M India |
| 8 | Compress image | 823K India, #1 in usage logs |
| 9 | Remove background | 823K US, 450K Bangladesh, remove.bg site closing 1 Dec |
| 10 | Convert image (WebP, HEIC, PNG, JPG) | 246K HEIC US, 246K WebP India |
| 11 | Resize to exact KB with exam presets | 90K, 60K, 50K India for 20, 50, 100 KB |
| 12 | Passport and visa photo | strong, numbers inconsistent |
| 13 | Split PDF | about 336K global |
| 14 | Convert and compress video | 673K US |
| 15 | MP4 to MP3, audio convert | 550K US, 165K MP4 to MP3 |
| 16 | QR codes | 673K to 823K US |
| 17 | Word counter | 1.22M US |
| 18 | Calculators (BMI, percent, age, units, EMI) | BMI 2.7M, percent 673K, age 550K US |
| 19 | Currency converter | 673K US |
| 20 | Text to speech | 301K US |
| 21 | Image to text (OCR) | 90K US, 74K Bangladesh |
| 22 | GIF maker | 201K US |
| 23 | Password generator | 201K US |
| 24 | Signature maker and cleaner | 74K US |
| 25 | Upscale image | moderate |
| 26 | Object eraser | #7 in usage logs |
| 27 | Photo collage | 60K US |
| 28 | Send to device | our differentiator, LocalSend has 94k stars |
| 29 | Find in screenshots and photos | our differentiator |
| 30 | Clipboard history and snippets | Raycast and Maccy territory |

---

## 0. Universal converter

Any file to any sensible format, in one drop. 214 formats in 20 families,
9,214 format-to-format conversions, every one on your device. Panoply routes
each request through the shortest chain of converters (Word to JPG goes
through PDF automatically). The full matrix, engines and read and write
support are in [CONVERSIONS.md](CONVERSIONS.md), generated from
`conversions/formats.py`.

| Family | Formats |
|---|---|
| Image | 33, including HEIC, AVIF, JPEG XL, WebP, PSD and 10 camera raw formats |
| Video | 21, including MP4, MOV, MKV, WebM, AVI, MXF |
| Audio | 23, including MP3, FLAC, ALAC, Opus, ringtones, MIDI |
| Documents | 21, including Word, Pages, LaTeX, Markdown, Typst, Publisher, Visio |
| Spreadsheets and data | 22, including Excel, Numbers, CSV, JSON, YAML, Parquet, SQLite |
| Slides | 5, including PowerPoint, Keynote, OpenDocument |
| PDF and ebooks | 14, including PDF/A, XPS, DjVu, EPUB, Kindle, comics |
| Everything else | Subtitles, archives, fonts, 3D, CAD, maps, mail and contacts, notebooks, DICOM, LUTs, Lottie |

## 1. PDF

| Module | What it does | |
|---|---|---|
| Merge | Combine files and pages into one PDF | ⚡ |
| Split | By range, every N pages, by bookmark, or by size | ⚡ |
| Organize pages | Reorder, delete, duplicate, insert blank pages, visual grid | ⚡ |
| Reverse order | Back to front in one click | ⚡ |
| Overlay | Lay one PDF over another (letterhead, forms) | ⚡ |
| Smart split | Split a scanned batch into separate documents automatically | 🧠 |
| Alternate merge | Interleave front and back scans into the right order | ⚡ |
| Divide pages | Split each page in two (scanned book spreads) | ⚡ |
| Combine pages | Several pages onto one page, or a grid | ⚡ |
| Clean scans | Remove blank pages, deskew tilted scans | ⚡🧠 |
| Extract pages | Pull chosen pages into a new file | ⚡ |
| Rotate | Single pages or all | ⚡ |
| Straighten by any angle | Fine rotation for skewed pages | ⚡ |
| Compress | Target a size ("under 2 MB") or a quality level, live preview | ⚡ |
| Repair | Fix broken or truncated PDFs | ⚡ |
| OCR | Make scans searchable and selectable, 100+ languages | 🧠 |
| Edit text | Change existing words in place, matching the font | ⚡ |
| Annotate | Highlight, underline, comments, shapes, freehand | ⚡ |
| Sign | Draw, type or use a saved signature, place anywhere | ⚡ |
| Digital signatures | Certificate signing, validate signatures, trusted timestamps, remove a signature | ⚡ |
| Self-attest stamp | "Self attested" with signature and date on every page | ⚡ |
| Fill forms | Type into fields, save filled or flattened | ⚡ |
| Create form | Turn a flat PDF into a fillable one, auto-detect fields | 🧠 |
| Flatten | Burn forms and annotations into the page | ⚡ |
| Protect and unlock | Add a password, remove a known one, set permissions | ⚡ |
| Redact | Black out text for real, auto-find emails, phones, ID and card numbers | 🧠 |
| Redaction check | Find fake redactions with text still underneath, issue a redaction report | ⚡ |
| Watermark | Text or image, position, opacity, tiled | ⚡ |
| Remove watermark | Delete watermark layers and stamp objects, paint out flattened ones | ⚡🧠 |
| Page numbers, headers, footers | Formats like "Page 3 of 12", Bates numbering | ⚡ |
| Crop and resize pages | Trim margins, change paper size A4 to Letter | ⚡ |
| Compare | Show what changed between two versions, text and visual | ⚡ |
| Edit metadata | Title, author, subject, keywords, or strip all | ⚡ |
| Attachments | Add or extract files embedded in a PDF | ⚡ |
| Sanitize | Strip scripts, embedded files and hidden content from untrusted PDFs | ⚡ |
| Privacy scan | Report hidden data (authors, comments, layers, metadata) before sharing | ⚡ |
| Leak tracing | Invisible per-recipient marks, so you know who shared it | ⚡ |
| Layers | Show, hide or remove PDF layers | ⚡ |
| Web optimize | Linearize for fast opening online | ⚡ |
| Rasterize and outline fonts | Flatten pages to images, or text to shapes for print | ⚡ |
| Remove annotations | Strip comments, stamps, highlights | ⚡ |
| Recolor | Change page background or text color, dark mode PDFs | ⚡ |
| Bookmarks and outline | Add, edit, auto-generate from headings | ⚡ |
| Table of contents page | Generate a clickable contents page | ⚡ |
| Page labels | i, ii, iii then 1, 2, 3 | ⚡ |
| Stamps | Approved, Paid, Draft, Confidential, custom stamp library | ⚡ |
| Extract images | Save every embedded image at full quality | ⚡ |
| Extract text | Plain text, Markdown or HTML | ⚡ |
| Extract tables | Tables to Excel or CSV | 🧠 |
| Statements and invoices to Excel | Bank statements, invoices and receipts to clean rows, QBO or CSV | 🧠 |
| Prepare for AI | Clean text, headings and tables for pasting into AI tools | ⚡ |
| Grayscale | Print-friendly black and white | ⚡ |
| PDF/A | Archive-grade output for courts and governments | ⚡ |
| Print layouts | N-up (2 or 4 per sheet), booklet, poster tiling | ⚡ |
| Convert out | PDF to Word, Excel, PowerPoint, images, text, Markdown, HTML, EPUB, JSON, ZIP of pages | ⚡🧠 |
| Convert in | Word, Excel, PowerPoint, images, HTML, Markdown, EPUB, emails, RTF, XPS, CSV, JSON, PSD, DjVu, comics (CBZ), Apple Pages, Publisher, Visio, WordPerfect, Works to PDF | ⚡ |
| Translate PDF | Keep layout, translate text on-device | 🧠 |
| Ask your PDF | Summaries and questions answered by a local model | 🧠 |
| Read aloud | Natural voice, offline | 🧠 |

## 2. ID, forms and document photos

The tasks people search for every day for government and job forms.

| Module | What it does | |
|---|---|---|
| Passport and visa photo | Pick a country or form, auto-detect the face, crop to the exact head size and position, white or blue background, compliance check | 🧠📱 |
| Print sheet | Tile 4, 6 or 8 photos on a 4x6 or A4 sheet for the print shop | ⚡ |
| Resize to exact KB | "Photo 20 to 50 KB, 200 x 230 px, JPG", the classic government form rule, presets for common exams and portals | ⚡ |
| Exam and portal presets | SSC, UPSC, RRB, Passport Seva, PAN, Bangladesh Teletalk and NID, editable and linked to the official notice. Handles portals that count 1 KB as 1000 bytes | ⚡ |
| Name and date strip | Add name and photo date under an exam photo | ⚡ |
| Thumb impression and declaration | Clean a scanned thumb print or handwritten declaration to spec | 🧠 |
| Documents under a size | "JPG to PDF under 100 KB" for uploads | ⚡ |
| Signature cleaner | Photo of a signature on paper to a clean transparent PNG, resize to form specs | 🧠 |
| Document scanner | Camera or photo, edge detect, flatten perspective, enhance, multi-page PDF | 🧠📱 |
| ID card scan | Front and back on one page, ready to print or upload | 🧠📱 |
| Mask ID numbers | Hide all but the last 4 digits on ID copies before sharing | 🧠 |
| DPI and print size | Set DPI, print at a real-world size | ⚡ |

## 3. Images

| Module | What it does | |
|---|---|---|
| Convert | HEIC, JPG, PNG, WebP, AVIF, JXL, TIFF, BMP, GIF, ICO, RAW, SVG | ⚡ |
| Compress | Smallest file at the same look, or a target size | ⚡ |
| Format shootout | Try every format, keep the smallest that looks the same | ⚡ |
| Optimize for web | One click: resize, modern format, strip data, batch | ⚡ |
| Resize | Exact pixels, percent, longest side, batch, 130+ presets for every platform | ⚡ |
| Crop | Free, ratios, social presets, smart crop around the subject | ⚡🧠 |
| Rotate and flip | Including auto-rotate from camera data | ⚡ |
| Remove background | Clean cutout, hair-level edges | 🧠 |
| Keep only what I say | Background removal by prompt ("keep the dog") | 🧠 |
| Change background | Solid color, blur, gradient, another image | 🧠 |
| Upscale | 2x and 4x, sharper detail | 🧠 |
| Enhance | Denoise, sharpen, fix exposure and white balance | 🧠 |
| Restore old photo | Scratches, fading, faces | 🧠 |
| Object eraser | Paint over something, it disappears | 🧠 |
| Remove watermark | Text, logos, date stamps and AI badges painted out cleanly, batch | 🧠 |
| Blur faces and plates | Automatic, for sharing safely | 🧠 |
| Metadata | View, edit or strip location, camera, dates | ⚡ |
| Add location | Write GPS into photos that lack it, batch by date | ⚡ |
| Content credentials | View, keep or remove C2PA provenance data | ⚡ |
| Was this edited? | Forensic checks for edits and AI generation | 🧠 |
| Cull photos | Flag blurry, closed-eye and duplicate shots, pick the best of a burst | 🧠 |
| Watermark | Text or logo, batch | ⚡ |
| Add text | Captions, memes, labels | ⚡ |
| Collage | Grids and layouts | ⚡ |
| Screenshot beautifier | Background, padding, shadow, device frame | ⚡ |
| Color tools | Palette from image, picker, converter, contrast check | ⚡ |
| Compare images | Side by side, slider, pixel diff | ⚡ |
| Favicon and app icons | Every size for web, iOS, Android, desktop from one image | ⚡ |
| SVG tools | Optimize, convert to PNG, trace a PNG to SVG | ⚡ |
| Straighten | Fix tilted horizons and skewed photos of paper | 🧠 |
| GIF maker | From images or video, optimize size | ⚡ |
| Sprite sheet | Pack and unpack | ⚡ |
| Image to text | Copy text out of any picture | 🧠 |
| Image to Markdown, LaTeX, code | Structured OCR for notes, math and screenshots of code | 🧠 |
| What font is this? | Identify fonts in an image | 🧠 |
| HDR images | Make and view HDR gain-map photos | ⚡ |
| Film looks | Film grain, date stamp, retro looks | ⚡ |
| E-reader optimize | Comics and manga pages tuned for e-ink screens | ⚡ |
| Describe image | Alt text and captions, offline | 🧠 |
| QR and barcodes | Make, style, read from image or camera | ⚡📱 |

## 4. Video

| Module | What it does | |
|---|---|---|
| Trim and cut | Remove start, end or middle parts | ⚡ |
| Compress | To a size or quality, with presets for every app limit (WhatsApp, Discord, Gmail, Slack, forms) | ⚡ |
| Convert | MP4, MOV, MKV, WebM, AVI, HEVC, AV1 | ⚡ |
| Resize and reframe | 9:16, 1:1, 16:9 for every platform, follow the subject | ⚡🧠 |
| Crop, rotate, flip | Including fixing sideways phone videos | ⚡ |
| Merge | Join clips | ⚡ |
| Split | By time, size or scene | ⚡🧠 |
| Speed | Faster, slower, smooth slow motion | ⚡ |
| Reverse and loop | Boomerang style | ⚡ |
| Audio | Mute, replace, extract, adjust volume | ⚡ |
| Subtitles | Auto-generate offline, edit, burn in or export SRT | 🧠 |
| Edit by transcript | Delete words in the text, the video follows, remove filler words | 🧠 |
| Bitmap subtitles to SRT | OCR image-based subtitle tracks | 🧠 |
| Smooth motion | Frame interpolation for slow motion | 🧠 |
| Translate subtitles | On-device | 🧠 |
| Subtitle files | Convert SRT, VTT, ASS, shift timing to sync | ⚡ |
| Streams | Extract or swap subtitle and audio tracks, unpack MKV and MP4 | ⚡ |
| Repair video | Fix broken or unplayable files | ⚡ |
| Restore old video | Denoise, deinterlace, upscale | 🧠 |
| Tidy screen recording | Cut dead time and pauses automatically | ⚡🧠 |
| Text and watermark | Titles, logos | ⚡ |
| Remove watermark | Logos, timestamps and burned-in text, tracked across frames | 🧠 |
| Blur faces or regions | Track and blur | 🧠 |
| Stabilize | Smooth shaky footage | ⚡ |
| Frames | Grab stills, contact sheet, best thumbnail | ⚡🧠 |
| Video to GIF and back | | ⚡ |
| Frame rate | 24, 30, 60 | ⚡ |
| HDR to SDR | Fix washed-out iPhone videos on other screens | ⚡ |
| Picture in picture | Overlay one video on another | ⚡ |
| Audio plus image to video | Podcast or song to MP4 with cover art and waveform | ⚡ |
| Video to Live Photo | And Live Photo to video | ⚡ |
| Teleprompter | Scrolls with your voice | 🧠 |
| Info | Codec, bitrate, resolution, everything | ⚡ |

## 5. Audio

| Module | What it does | |
|---|---|---|
| Convert | MP3, AAC, M4A, WAV, FLAC, OGG, OPUS | ⚡ |
| Trim, merge, fade | | ⚡ |
| Compress | Smaller voice notes and podcasts | ⚡ |
| Normalize | Even loudness, broadcast standards | ⚡ |
| Remove noise | Hum, hiss, background chatter | 🧠 |
| Vocal remover | Split vocals, drums, bass, instruments | 🧠 |
| Transcribe | Speech to text with timestamps and speakers, offline | 🧠 |
| Cut by transcript | Select words to delete them from the audio | 🧠 |
| Meeting recorder | Mic plus system audio, no bot joins, notes and key moments offline | 🧠 |
| Dictation | Talk into any text field, system-wide | 🧠 |
| Dictation cleanup | Removes ums, "scratch that", learns your words, edit selected text by voice | 🧠 |
| Text to speech | Reads text or a TXT, MD or PDF aloud into an audio file with the system's own voices, offline (WAV first; macOS and Windows) | ⚡ |
| Speed and pitch | Change one without the other | ⚡ |
| Remove silence | Tighten recordings | ⚡ |
| Stereo to mono | And channel swap, split, merge | ⚡ |
| Ringtone maker | Clip and export for iPhone and Android | ⚡ |
| Tag editor | Title, artist, album art, batch | ⚡ |
| BPM and key | Detect tempo and musical key | ⚡ |
| Audio to MIDI | Melody and notes from a recording | 🧠 |
| Loop maker | Loops with no visible jump, from any clip | ⚡ |
| Recorder | Voice memos | ⚡📱 |

## 6. Documents and ebooks

| Module | What it does | |
|---|---|---|
| Office to and from PDF | Word, Excel, PowerPoint, OpenDocument | ⚡ |
| Any file to Markdown | Clean text for notes and AI tools | ⚡ |
| Markdown to DOCX, HTML, PDF | Styled output | ⚡ |
| Spreadsheet convert | CSV, XLSX, JSON, TSV | ⚡ |
| Ebook convert | EPUB, MOBI, AZW3, PDF, FB2 | ⚡ |
| Merge documents | | ⚡ |
| Compare documents | Tracked-changes style diff | ⚡ |
| Word count and reading time | | ⚡ |
| Grammar and spelling | Offline, private | 🧠 |
| Translate document | Keep formatting | 🧠 |
| Summarize | Local model | 🧠 |
| Email and contact files | Open and convert MSG, EML, MBOX, ICS, vCard | ⚡ |
| Query data files | Run SQL on local CSV, JSON and Excel files | ⚡ |
| 3D files | View STL, OBJ, STEP, repair STL for printing | ⚡ |
| Resume builder | Templates, ATS check, PDF out | ⚡ |
| Printables | Letterhead, business cards, graph and lined paper | ⚡ |

## 7. Find anything on your device

The search Spotlight and Explorer never got right, all indexed locally.

| Module | What it does | |
|---|---|---|
| Instant file search | Every file by name in milliseconds, whole disk | ⚡ |
| Search inside files | Words inside PDFs, Office docs, ebooks, code, emails | ⚡ |
| Screenshot search | Find screenshots by the text in them ("wifi password", "order id") | 🧠 |
| Photo search by meaning | "red car at night", "my passport", "receipts" | 🧠 |
| Metadata search | "photos taken in Goa in December", "PDFs over 50 pages", "videos longer than 10 min", "files from this camera", "documents by this author" | ⚡ |
| Similar images | Find near-duplicates and edits of the same photo | 🧠 |
| Duplicate finder | Exact duplicates across drives, safe cleanup | ⚡ |
| Big file finder | Disk map, what is eating your space | ⚡ |
| Recent everything | What you downloaded, saved or screenshotted lately | ⚡ |
| What is this file? | Identify unknown files and open them | ⚡ |
| Search unplugged drives | Catalog external drives, search them while disconnected | ⚡ |
| Search inside videos | Find spoken words and scenes in your videos | 🧠 |

## 8. Files

| Module | What it does | |
|---|---|---|
| Batch rename | Patterns, numbering, dates from photo data, find and replace | ⚡ |
| Auto-organize | Rules and watched folders: sort Downloads by type, date, source | ⚡ |
| Tidy my Desktop | Sort a messy folder by what files are, not just their type (invoices, IDs, screenshots, receipts) | 🧠 |
| Archives | Zip, unzip, 7z, RAR, TAR, password archives, split archives | ⚡ |
| Encrypt | Lock a file or folder with a password | ⚡ |
| Shred | Delete beyond recovery | ⚡ |
| Checksums | SHA-256, MD5, verify downloads | ⚡ |
| Compare folders | What is different, sync one way | ⚡ |
| Split and join | Big files into parts and back | ⚡ |
| Metadata for any file | View, edit, strip | ⚡ |
| Quick preview | Open odd formats without installing anything | ⚡ |
| Markdown viewer | Open .md files rendered, with outline and search | ⚡ |
| Fonts | Preview, convert TTF, OTF, WOFF2, install | ⚡ |
| Rename to convert | Rename photo.heic to photo.jpg in Finder or Explorer and it converts | ⚡ |
| File shelf | Park files while you drag between windows | ⚡ |

## 9. Share between your devices

| Module | What it does | |
|---|---|---|
| Send to device | Phone, laptop, desktop on your network, full Wi-Fi speed | ⚡📱 |
| Receive | Accept files, auto-save to a chosen folder | ⚡📱 |
| Share by QR | Device without Panoply scans and downloads in its browser | ⚡ |
| Clipboard sync | Copy on phone, paste on laptop | ⚡📱 |
| Phone scanner to desktop | Scan with the phone camera, the PDF lands on your computer | 🧠📱 |
| Send text and links | | ⚡📱 |
| Borrow the desktop | Phone hands heavy jobs to your own computer, gets the result back | ⚡📱 |
| Temporary folder share | Share a folder on the network until you stop it | ⚡ |
| Send to many | One file to several devices at once | ⚡ |
| Far away transfer | Send over the internet through an encrypted relay when not on the same network | 🌐 |

## 10. Capture

| Module | What it does | |
|---|---|---|
| Screenshot | Area, window, full screen, scrolling, timed | ⚡ |
| Annotate | Arrows, text, blur, highlight, numbered steps | ⚡ |
| Copy text from screen | Select any text, even in images and videos | 🧠 |
| Pin to screen | Float a screenshot above everything | ⚡ |
| Screen recording | With audio, webcam bubble, clicks shown | ⚡ |
| Record to GIF | | ⚡ |
| Color picker and ruler | Pick colors, measure pixels | ⚡ |
| Capture history | Every capture, searchable by text | 🧠 |

## 11. Text and clipboard

| Module | What it does | |
|---|---|---|
| Clipboard history | Searchable, pinned items, skips passwords | ⚡ |
| Clean links | Strip tracking parameters from copied links automatically | ⚡ |
| Redact pasted text | Hide emails, phones and IDs before you paste somewhere | 🧠 |
| Snippets | Type ";addr" and get your address, anywhere | ⚡ |
| Transform text | Case, sort, dedupe lines, remove line breaks, trim, slug | ⚡ |
| Count | Words, characters, sentences, reading time | ⚡ |
| Find and replace across files | With regex, preview first | ⚡ |
| Diff text | | ⚡ |
| Translate text | Offline | 🧠 |
| Scratchpad | Quick notes in the bar | ⚡ |
| Emoji and symbols | Search every character | ⚡ |
| Lorem ipsum | | ⚡ |

## 12. Calculators and converters

| Module | What it does | |
|---|---|---|
| Natural calculator | "15% of 2400", "3 hours 20 min in seconds", typed in the bar | ⚡ |
| Units | Length, weight, temperature, data, cooking | ⚡ |
| Currency | Rates refreshed once a day | 🌐 |
| Price tag converter | Point the phone camera at a price, see it in your currency | 🧠📱 |
| Time zones | World clock, meeting planner | ⚡ |
| Dates | Days between, add days, age, working days | ⚡ |
| Loan and EMI | Monthly payment, schedule | ⚡ |
| Tax | GST, VAT, sales tax, inclusive and exclusive | ⚡ |
| Percent, tip, split bill | | ⚡ |
| Health | BMI, calories | ⚡ |
| Timers | Stopwatch, countdown, pomodoro | ⚡ |
| Random | Pick from list, dice, coin, number | ⚡ |

## 13. Developer tools

| Module | What it does | |
|---|---|---|
| Data formats | Format, validate, convert JSON, YAML, XML, TOML, CSV | ⚡ |
| Encode and decode | Base64, URL, HTML entities, Unicode | ⚡ |
| JWT | Decode and inspect | ⚡ |
| Hashes | MD5, SHA family, HMAC, bcrypt | ⚡ |
| IDs | UUID, ULID, nano ID | ⚡ |
| Passwords | Generate strong passwords and passphrases | ⚡ |
| Regex tester | Live matches, explain | ⚡ |
| Cron | Explain and preview schedules | ⚡ |
| Timestamps | Unix, ISO, time zones | ⚡ |
| Colors | HEX, RGB, HSL, OKLCH | ⚡ |
| Minify and beautify | HTML, CSS, JS, SQL | ⚡ |
| Number bases | Binary, hex, octal | ⚡ |
| Network | IP and subnet calculator, local port check, my local IP | ⚡ |
| cURL to code | Convert requests to code in any language | ⚡ |
| Certificates and keys | Decode certificates, PGP keys, CSR | ⚡ |
| LLM tokens | Count tokens and estimate cost for prompts and files | ⚡ |
| DNS and domain | DNS, WHOIS, SPF, DKIM checks | 🌐 |
| Markdown table maker | | ⚡ |

## 14. Privacy and security

| Module | What it does | |
|---|---|---|
| Clean before sharing | Strip location and hidden data from any file, automatic on send | ⚡ |
| Auto-redact | Find and hide personal data in documents and images | 🧠 |
| Encrypt and shred | (see Files) | ⚡ |
| Password breach check | Checks without sending your password | 🌐 |
| Verify downloads | Compare checksums and signatures | ⚡ |

## 15. Business paperwork

| Module | What it does | |
|---|---|---|
| Invoice maker | GST, VAT or sales tax invoices, numbered, saved locally, PDF out | ⚡ |
| Receipts and quotes | Same templates for receipts and quotations | ⚡ |
| India paperwork | Salary slip, rent receipt, delivery challan, HSN and SAC finder, GSTIN check, UPI QR on invoices | ⚡ |

## 16. Print

| Module | What it does | |
|---|---|---|
| Photo print sheets | Passport, wallet, 4x6 layouts | ⚡ |
| Poster tiling | Print a big image across many A4 pages | ⚡ |
| Booklet | Fold-and-staple page order | ⚡ |
| Labels | Address and product labels from a list | ⚡ |

## 17. Your computer (later, Vorssaint and PowerToys territory)

Window snapping, keep awake, app uninstaller, disk cleanup, battery health,
system monitor, per-app volume, keyboard remapping, hosts file editor, hide
menu bar icons, cleaning mode that locks the keyboard, Raycast extension compatibility, mouse button remapping, a dev server and port monitor (what is running on which port, stop it in one click).

## Internet by nature (opt-in, clearly labelled)

Currency rates, password breach check, far away transfer, DNS checks, and media saving from websites. The
last one stays a community module, desktop only, outside the app store builds,
because of platform terms. Demand is huge ("YouTube to MP3" about 2.2M searches a month in the US) but it carries legal risk.

## Language models

Panoply ships no chatbot model. Text-smart features use the model already on
the device (Apple Foundation Models, Gemini Nano, Phi Silica), or the user's
own Ollama, LM Studio or API key, and are hidden when none exists. People's
own AI assistants use every Panoply tool through MCP.

## Model downloads

🧠 modules download their model once, on first use, from the Panoply release
mirror. After that they never touch the network. The app works offline from
then on.
