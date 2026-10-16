# User pass

Every tool used the way a person would use it, with real files, awkward files and every panel state. This is the log of that pass on macOS (Apple Silicon, macOS 26.6, release build of `qa/user-pass`). Windows and Linux are covered by the desktop smoke workflow, see "Windows and Linux" at the end.

How it was done, so it can be repeated:

- **CLI.** `panoply run <tool>` and the tool commands over `tests/corpus/cache` (pictures, PDFs, audio, video) plus awkward inputs made for the pass: an empty file, a truncated JPEG, PDF and MP4, a PDF named `.jpg`, a JPEG named `.png`, random bytes named `.png`, a 1 by 1 and a 16 by 16 PNG, a file name in four scripts, a read-only folder and a 3 MB disk image that is full. 170 jobs, each timed with `/usr/bin/time -l` for peak memory.
- **App path.** `panoply-desktop --selfcheck-userpass --manifest FILE --out DIR` (new, dev only, `--features snapshot`, `apps/desktop/src/selfcheck.rs`). It opens the offscreen app window, opens each tool on real files with real options, presses Run through the same job path as the Return key, and saves the options, running, done, failed or cancelled screen at 2x in light and dark. 49 scenarios cover all 31 tools. Nothing is shown on screen.
- **Judging.** Result pictures and PDF pages were opened as PNGs and looked at, next to the original at 100% where quality was the question.

Severity: **P0** broken or data loss, **P1** clearly below the bar, **P2** polish.

## Speed and memory against FLOWS.md

- **UI thread.** In the 49 app scenarios the slowest single frame while a job ran was 4.39 ms (Passport photo, the 4 ms line is 4.0); the other 48 stayed under 3.9 ms. A tool panel is drawn the first time in 0.9 to 3.0 ms, 8.5 ms for the first panel opened in a fresh process.
- **Jobs.** 170 CLI jobs. Most tools are far inside the budgets: hash of a 300 MB PDF 1.0 s (faster than `shasum`, 2.7 s), PDF split of a 3,000 page file 0.07 s, Compress PDF of a 300 MB file 5.4 s at 17 MB, 3 hour FLAC to MP3 7.2 s at 17 MB, normalize of the same file 7.3 s at 24 MB, a 400 megapixel PNG compressed at 23 MB, passport photo 0.4 to 1.0 s.
- **Over 300 MB.** HEVC/HEIF photos through ImageIO 825 MB to 834 MB (33 MP, documented), camera RAW 624 MB (documented), OCR with Arabic, Hebrew or Hindi 383 to 521 MB (documented up to 550 MB), video compress 0.4 to 1.4 GB after UP-10 (it was 0.5 to 3.6 GB; not documented for hardware encoders).
- **Slow.** Compress PDF with `--under 1mb` on a 228 page scan 25.9 s; PDF to images of 3,000 pages at 36 DPI 14.8 s; OCR of a 42 page Arabic book 7.4 s.

## Counts

| | Found | Fixed on this branch | Open |
|---|---:|---:|---:|
| P0 | 0 | 0 | 0 |
| P1 | 15 | 12 | 3 |
| P2 | 32 | 11 | 21 |

No P0: nothing crashed, hung, lost an original or left a half-written file. A job that failed left no output behind, also on a full disk, and no original was touched in any run.

What held up, for the record: Remove background (hair, flag patch and collar edges are clean on a portrait, `user-pass/cutout-on-dark.jpg`), Upscale (`user-pass/upscale-nearest-bicubic-model.jpg`), the six passport portraits (`user-pass/passport-six-portraits.jpg`), HEIC compress (54% smaller, indistinguishable at 100%), EXIF orientation (all eight orientations resize to the same picture), strip metadata (GPS and camera tags really gone), passport framing and background on six portraits, signature cleaning, OCR text selectable after the pass, 300 MB PDF compress in 5 s at 17 MB peak, a 3 hour FLAC converted in 7 s, a 20,000 by 20,000 PNG compressed in 0.6 s at 23 MB, cancel on video, erase and OCR returns in under 100 ms.

---

## P1

### UP-01 The option a tool cannot run without is folded under "More options" (fixed)

- **Tools.** Passport photo (country), Erase object (area or mask), Remove watermark (the "these are my pictures" confirmation and the area), Resize image (longest side, width), Upscale (scale), Remove background (result), Compress video (size limit when a codec is preset).
- **Steps.** Open Passport photo with a photo, add nothing, look at the panel.
- **Expected.** The thing you must say is on screen: which country.
- **Happened.** The four options shown were Print resolution, Cut-out engine, Document and Engine. Country sat under "More options (12)". Pressing Run gives "That did not work: Say which country's rule to use, for example country=IN". Same shape for the others: Resize shows Height and Quality and hides Longest side; Erase shows Format, Quality and Keep metadata and hides the area.
- **Cause.** `Form::from_manifest` ranks fields by a keyword list; none of these names were on it.
- **Fix.** `PINNED` in `apps/desktop/src/form.rs` names the deciding options in order, and a test (`the_option_a_tool_cannot_run_without_is_never_folded_away`) checks 13 tools.
- **Screenshot.** Before: `user-pass/passport-options-before.png`, `user-pass/resize-options-before.png`, `user-pass/passport-nocountry-error.png`. After: `user-pass/passport-options-after.png`, `user-pass/resize-options-after.png`.

### UP-02 A result card hides its warnings (fixed)

- **Tool.** Passport photo (any tool with a long report).
- **Steps.** Passport photo on `fisher.jpg`, country IN, Run.
- **Expected.** "2 warnings" names the two warnings.
- **Happened.** The card said "2 warnings, no failures" and then listed six lines, every one starting "pass:". The two `warn:` lines (Neutral expression, Lighting) were the 8th and 11th lines and the card showed only the first six. The warnings were only in the CLI output and the report file.
- **Fix.** `result_notes` in `apps/desktop/src/ui/panel.rs`: warnings and failures first, in the warning colour, then as many other lines as fit, then "and N more lines". Unit test `warnings_are_never_cut_even_after_many_passes`.
- **Screenshot.** Before `user-pass/passport-done-before.png`, after `user-pass/passport-done-after.png`.

### UP-03 Images to PDF writes a blank page from a cut-off JPEG and says it worked (fixed)

- **Tool.** Images to PDF (CLI `panoply pdf from`, app).
- **Steps.** `panoply pdf from corrupt-truncated.jpg` (a JPEG cut at 3 kB).
- **Expected.** The plain "This JPEG is incomplete" that every other picture tool gives.
- **Happened.** Exit 0, "1 page, 4 KB. 1 JPEG stored without re-encoding." The page is white. The header was readable so the bytes were embedded as they were.
- **Fix.** `write_jpeg` in `crates/panoply-pdf/src/images_in.rs` refuses a JPEG with no end-of-image marker after the scan. Test `a_jpeg_cut_short_is_refused_not_written_as_a_blank_page`.
- **Screenshot.** `user-pass/images-to-pdf-blank-page.png` (the whole page).

### UP-04 Image to text shows the recognised text as a 17 pt title (fixed)

- **Tool.** Image to text (app).
- **Steps.** Image to text on a photographed invoice, Run.
- **Expected.** A short title and the text in a readable block.
- **Happened.** The whole text was the card's title: bold, large, every table cell on its own line, the green tick floating mid-list beside "48,000.00", and the confidence line ("30 words, 99% mean confidence") pushed to the very bottom.
- **Fix.** When a tool answers with text (no output file, a long or multi-line headline) the card shows the summary line as the title and the text in a card below it.
- **Screenshot.** Before `user-pass/image-to-text-done-before.png`, after `user-pass/image-to-text-done-after.png`.

### UP-05 Keeping pages past the end of a PDF blames the wrong thing (fixed)

- **Tool.** Organize PDF pages.
- **Steps.** `panoply pdf pages irs-f1040-2022.pdf --keep 5-9` (the PDF has 2 pages).
- **Expected.** "This PDF has 2 pages."
- **Happened.** "Every page would be removed. Keep at least one page." The module had worked out that pages 5 to 9 are past the end and then threw the note away.
- **Fix.** The error names the page count and the pages that do not exist. Corpus test `keeping_pages_past_the_end_says_so_instead_of_blaming_the_keep_list`.

### UP-06 A full disk or a locked folder shows the operating system's words (fixed)

- **Tools.** Every tool that writes (PDF engine, picture tools, video and audio, ID photos).
- **Steps.** Compress a PDF into a 3 MB disk image that has 1 MB free; compress a picture in a read-only folder.
- **Expected.** A sentence a person can act on.
- **Happened.** PDF: ": No space left on device (os error 28)" (a leading colon, no subject, an error number). Pictures: "Could not write /path/photo (compressed).jpg (Permission denied (os error 13))". No partial file was left behind in either case, which is right.
- **Fix.** `panoply_core::naming::plain_io` turns the error kind into words ("the disk is full", "Panoply does not have permission to write there") and drops the "(os error N)" tail; the picture, ID and media writers and the PDF error mapping use it. Unit test `system_wording_and_codes_never_reach_a_person`.

### UP-07 The privacy line says "Uses the network" on tools that, on this computer, never do (open, picture tools done under UP-47)

- **Tools.** 19 of the 31: every media tool (including Video and audio info, which only reads a file), OCR, Compress PDF and PDF to images, the AI picture tools, Passport photo.
- **Steps.** Open Video and audio info on a Mac that has `ffmpeg`, or Passport photo on a Mac.
- **Expected.** FLOWS.md capability 11: every screen states where the work ran. "Processed on this computer" when nothing leaves it.
- **Happened.** The header chip says "Uses the network" and a banner above the files says "Downloads the video engine (about 140 MB) once" or "Only on Windows and Linux, ... A Mac uses Apple Vision and downloads nothing." The CLI prints the same line to stderr before every run, including the tenth run of a model that is already on disk. The result card then says "Processed on this computer", which contradicts the header. A person reading the chip would think a photo of their passport goes online.
- **Cause.** The line comes from the static `network = true` in each manifest. It does not know the platform or what is already on disk.
- **Direction.** Decide per run: show the banner only when an item the tool needs is missing on this machine (the consent sheet already knows), and make the chip "Downloads once, first use" in that case. Needs the manifest to list the items per platform, so not done here.
- **Screenshot.** `user-pass/passport-options-before.png`, `user-pass/extract-audio-error.png`.

### UP-08 The result card never shows the result (open)

- **Tools.** All picture tools, passport photo, print sheet, PDF to images, cutout, erase, upscale.
- **Expected.** FLOWS.md shared capability 8 and the tuned-transform pattern: before and after you can see. Passport photo is "preview with head guides".
- **Happened.** After Run the card shows two size tiles ("Before 1.2 MB, After 7.7 MB") and the file name. To see a cutout, an erased area or a passport crop you have to press Open and leave the app. For tools whose point is not a size (print sheet "209% bigger", cutout "547% bigger", normalize "13% smaller", strip metadata "about the same size") the tiles say something unhelpful or alarming.
- **Direction.** A thumbnail of the first output (and a before and after slider for the picture tools), and size tiles only where size is the goal.
- **Screenshot.** `user-pass/sheet-done.png`, `user-pass/unmark-done.png`.

### UP-09 "Smallest that still looks good" returned VMAF 52 and 59 and the card says nothing (fixed)

- **Tool.** Compress video, default options.
- **Steps.** `panoply run media.compress phone-clip-vid20240601.webm` and `camera-clip-mvi0131.ogv`.
- **Expected.** Either quality that looks good (every other clip scored VMAF 88 to 96) or a warning.
- **Happened.** The notes read "Goal: the smallest that still looks good ... Quality VMAF 59.5" and "... VMAF 52.6". Those are visibly degraded. The result saved 32% and 65%. No warning, no suggestion to keep the original.
- **Fix.** `crates/panoply-media/src/ops/compress.rs`: in the default mode the first result is measured; under VMAF 75 it is encoded again with up to 1.8 times the bit rate, twice at most and never above the source's own. If it is still under 75 and either under 60 or saves under 25%, the original is kept and the card says why; otherwise the card carries a warning first. On the two clips the hardware encoder cannot do better (59 to 61 and 53 to 56 after the retries), so both now keep the original with "left as it is: the best Panoply could do saves 4% but looks visibly worse". Test `a_clip_that_comes_out_visibly_worse_is_encoded_again_or_says_so`. Clips that already scored 81 to 96 are unchanged.

### UP-10 Compress video of one 5 second 4K clip peaked at 3.5 GB resident (fixed)

- **Tool.** Compress video, default (VideoToolbox HEVC).
- **Steps.** `/usr/bin/time -l panoply run media.compress 4k-hevc-5s.mp4`.
- **Expected.** FLOWS.md: "constant for video and audio regardless of length (streamed)", and KNOWN-LIMITATIONS lists about 1.3 GB for software AV1 at 1080p as the worst case.
- **Happened.** Resident set 3,579 MB (ffmpeg child process). The 1080p clips measured 861 MB (10 s H.264), 886 MB (10 s WebM), 914 MB (portrait phone clip) and 1,081 MB (10-bit HDR HEVC, 5 s); 720p clips 0.2 to 0.5 GB. KNOWN-LIMITATIONS lists only software AV1 (about 1.3 GB at 1080p) as a heavy case, so the default hardware path is heavier than the page says. A person on an 8 GB Mac would feel it.
- **Cause.** Not the encoder. With `measure_quality=false` the same job peaks at 709 MB (4K) and 243 MB (1080p); the 3 GB and 0.6 GB on top are the quality check, which ran libvmaf with up to eight threads on frames at the source's own size.
- **Fix.** `apps`-independent, in `crates/panoply-media/src/ops/quality.rs`: both pictures are compared at no more than 1080p (the size VMAF's model is made for) with four threads. Test `the_comparison_never_runs_above_1080p`. Re-measured with the fix: 4K clip 3,584 to 1,370 MB, 1080p H264 861 to 485 MB, 10-bit HDR 1,081 to 650 MB, 1080p WebM 886 to 432 MB (the encoder alone is 709 MB for the 4K clip, 243 MB for 1080p).

### UP-11 The "Compress image" similarity floor lets smooth gradients block (fixed)

- **Tool.** Compress image, default.
- **Steps.** `day-earth-smiled.jpg` (9000 by 3500). Result: 5.0 MB to 268 KB at quality 21, similarity 0.979.
- **Expected.** "Clean" at the size a person would zoom to.
- **Happened.** At fit-to-screen it is indistinguishable. At 100% the smooth blue sky shows 8 by 8 blocking and a banded halo. The similarity score (SSIM on luminance) passes at 0.979 because the picture is mostly black.
- **Cause.** Nothing stops the quality search at a floor: it went down to 10 as long as the score held. Without a floor `day-earth-smiled.jpg` settles at quality 21, `ultra-hdr.jpg` at 18, and the 28 KB synthetic pictures at 10 to 16.
- **Fix.** `QUALITY_FLOOR` (40) and `quality_floor` in `crates/panoply-image/src/ops/search.rs`, used by both JPEG searches (`compress.rs` in memory, `big.rs` in strips). A source saved below 40 is never pushed above itself (the floor is capped by the search ceiling), and a similarity asked for by name under 0.97 is a request for a smaller file and has no floor. Tests `the_floor_holds_at_the_default_and_never_passes_the_ceiling` and `the_default_target_never_goes_under_the_quality_floor`.
- **Screenshot.** `user-pass/compress-image-blocking-100pct.jpg` (original left, result right, bottom row at 100%).

### UP-38 Remove watermark (auto) leaves a clear watermark and edits the subject's mouth (open)

- **Tool.** Remove watermark, default options, "These are my pictures" on.
- **Steps.** A portrait with a large semi-transparent white "SAMPLE" mark across the middle (the case KNOWN-LIMITATIONS says it finds: white letters over a picture). Run with the defaults.
- **Expected.** The mark is gone, or the card says nothing was found.
- **Happened.** "found text at the top (17% x 5% of the picture) ... 1 mark cleaned (0.48% of the picture changed)". The changed area is the subject's chin and mouth (pixels 500 to 800, 200 to 400); the watermark is untouched. The card is the green success state. Naming the area by hand (`--rect`) removes it but leaves a garbled patch, see UP-39.
- **Direction.** Treat a detection on a face (the face detector is already in the app) as not a watermark, report "no watermark found" when the best candidate is small and sits on skin, and run the detector on the large low-contrast mark class before the "text at the top" class. A test picture with a watermark belongs in the corpus; the beta smoke matrix has none, which is why this was not seen before.
- **Screenshot.** `user-pass/remove-watermark-auto-before-after.jpg` (original left, result right), `user-pass/remove-watermark-rect-and-auto-crops.jpg` (top original, middle auto, bottom by hand).

### UP-47 The Modules page warns "Uses the network" on tools that download a model once (fixed)

- **Tools.** Passport photo, Erase object, Remove background, Remove watermark (and Upscale and the two OCR tools, which list models the same way).
- **Steps.** Open Modules.
- **Expected.** What these tools really do: fetch a model once after a yes, and no file ever leaves the computer.
- **Happened.** The row said "Uses the network" and carried an orange "Network" badge, the colour of a warning, so a person would think the photo they cut out is uploaded.
- **Fix.** `Capabilities::network_use` in `crates/panoply-manifest/src/schema.rs` reads the manifest: network access plus a listed model is `ModelDownload`. The Modules page, the panel header chip and the panel note read it through `network_words` in `apps/desktop/src/ui/widgets.rs`: "Downloads a model once" and a neutral "Model download" badge, "Your files never leave this computer." in the note, "{tool} downloads a model once and keeps it on this computer." on the consent sheet, and "Model download:" in front of the CLI's line. The video and PDF engine downloads are not models and keep "Uses the network" (see UP-07). Screenshots at 2x: `user-pass/modules-model-download-light.png`, `user-pass/modules-model-download-dark.png`. Tests `the_network_is_told_by_the_models_a_module_lists`, `the_shipped_modules_that_fetch_a_model_are_model_downloads` and `the_consent_sheet_and_the_header_say_the_same_thing_about_a_model`.

### UP-44 The command bar answers "No tools match" to natural requests (fixed)

- **Tool.** Command bar (hotkey, and the search box on Home), the way most people start.
- **Steps.** Type `passport photo india` with a photo on the bar; `resize to 800px` with a photo; `loudness -16` with a sound; `trim 0:02-0:06` with a video; `split every 1` with a PDF; `whatsapp` with a video. 60 queries were run against a real catalog and real files.
- **Expected.** The tool, with the number or the country as a hint.
- **Happened.** "No tools match ..." for all six (and "resize 800", "under 2mb" aside). `passport` alone worked. The matcher needed every typed word to be in a tool's name or summary, so one extra word (a country, a size, a level, a time range) cancelled the match, and the services named in presets (WhatsApp, Discord, Gmail) were not searchable at all.
- **Fix.** `from_query` in `apps/desktop/src/search.rs`: when nothing matches every word, match on the words that name something (not numbers, not "to") when more than half are found; service names from the `preset` option are searched. A request Panoply cannot do (`pdf to word`) still answers "No tools match". Test `a_name_with_a_country_a_size_or_a_level_next_to_it_still_finds_the_tool`.

### UP-46 The command bar flashes "No tools match" while "under 50 kb" is being typed (fixed)

- **Tool.** Command bar, the way most people ask for a size.
- **Steps.** Type `under 50 kb` one key at a time, with and without a picture on the bar.
- **Expected.** The list stays put while the sentence is unfinished, and only says "No tools match" for a query that cannot become a match.
- **Happened.** `und`, `under` and `under 5` match no tool by themselves, so the empty state ("No tools match") flashed between keystrokes, and on the first letters the best match changed with each key, so the four rows jumped about.
- **Fix.** `search::Typing` in `apps/desktop/src/search.rs`, used by the command bar and by Home: while a query is plainly unfinished (`intent::is_unfinished`: only qualifier words, numbers, sizes or a trailing `to`, or a prefix of one) and the last answer had rows, that answer stays. A typo (`under 50 kbx`) or nonsense gets the empty state at once. Queries of one or two letters keep the catalog's order, so each letter only removes rows and never reorders them. Test `typing_under_50_kb_never_flashes_no_tools_and_never_reorders` feeds `under 50 kb` and `make this under 50 kb` key by key, with and without a file.

---

## P2

| ID | Tool | What happened | Status |
|---|---|---|---|
| UP-12 | Every tool | Two options that do not apply to the chosen target are shown anyway: Compress image with Target "Smallest" shows "Stay under" and "Quality". The options form has no "only when" rule. | Open |
| UP-13 | Compress image, Signature cleaner, Compress video | The first keyboard focus lands in a field that is not the main one (Stay under, Height, Largest shorter side) with a blue ring, instead of on Run or the first option that matters. | Fixed (first focus goes to the first option shown, or the panel) |
| UP-14 | Remove background | The Engine choice reads "Auto / Os / Model". "Os" is not a word; "System / Model". Result, the main choice, is the last of the four shown. | Fixed ("System"; the order is fixed by UP-01) |
| UP-15 | Passport photo | Country is a free-text box with no list and no marker that it is required; Run is enabled without it and the error says "country=IN", the command-line spelling, in a window with no command line. | Open |
| UP-16 | Passport photo | A 16 by 16 picture answers "No face was found" where FLOWS.md says "very low resolution (refuse and say why)". | Open |
| UP-17 | Passport photo, Print sheet | The size tiles read "1.6 MB to 236 KB, 85% smaller" for a passport photo and "1.2 MB to 3.7 MB, 209% bigger" for a print sheet. | Open (see UP-08) |
| UP-18 | App | Cancelling a running job returns to the options with no sign that it was cancelled. | Fixed ("Cancelled. Nothing was written.") |
| UP-19 | Convert, Resize, Compress, Passport | A PDF named `.jpg` says "This file is not an image Panoply can read. Check that it is a JPEG, PNG..." though the bytes are a PDF and Panoply knows it. Say "This is a PDF. Use PDF to images." | Fixed (`not_an_image_message`, test `a_pdf_named_like_a_picture_is_called_a_pdf`) |
| UP-20 | Convert image | An SVG says "not an image Panoply can read" (SVG is on the limitations page, the message should say SVG is not supported yet). | Fixed (`not_an_image_message`, test `a_pdf_named_like_a_picture_is_called_a_pdf`) |
| UP-21 | App | Every failure is titled "That did not work", and the next step "Check the file and the options, then try again." is printed under messages that already say what to do ("Make it smaller first, or leave it as it is."). | Open |
| UP-22 | Resize to exact KB | "The picture was reduced from 3200 x 4000" for a file that is 3684 x 4605: it reports its own pre-shrunk working copy as the original. | Open |
| UP-23 | Resize to exact KB | With no size or limits set it re-encodes the picture and writes `jemison (3200x4000, 1665KB).jpg`, a copy that is the same size. It should ask for a limit. | Open |
| UP-24 | Resize to exact KB | Anchor "top" is ignored when the shape differs by more than 15% (it pads instead); the card says why, but the option stays selectable. | Open |
| UP-25 | Strip metadata | "Private" (the default) removes exactly what "GPS" removes (location) and keeps camera make, model, software and date. The name promises more. | Open |
| UP-26 | Images and PDF | A file reached through a symlink gets its result next to the real file, not next to the link the person chose. Found by accident: a run through symlinks wrote results into the corpus cache. | Fixed in `CapabilityGuard::output_dir_for` (test `a_result_goes_beside_the_link_the_person_chose_not_beside_its_target`) |
| UP-27 | CLI | `panoply run image.compress` with no file exits 0 and prints nothing. | Fixed |
| UP-28 | CLI | `panoply run nope.tool` says "This module could not be loaded: Could not read nope.tool ... (os error 2). Reinstall the module". It is a typo, not a broken install. | Fixed |
| UP-29 | Home, command bar | The search box said "Search 31 tools and files" and "your files". There is no file search yet (it is on the launch gate), so a person who types a file name finds nothing. | Fixed (says "tools") |
| UP-30 | CLI | When stderr is not a terminal every percent is a new line (`28% Encoding`, `29% Encoding`, ...). 40 lines for one picture. Print progress only on a terminal, or one line per ten percent. | Fixed (one line per ten percent or step) |
| UP-31 | Compress PDF | "Under 1 MB" on a scan took 26 s and finished at 1.9 MB with similarity 0.865 (the floor elsewhere is 0.97). It says it could not reach the size; it could say it stopped at a quality it does not recommend. | Open |
| UP-32 | Trim | A 4 second cut at "auto" precision re-encodes all 240 frames ("0 frames copied untouched"), so it is not lossless. The card says so. | Open |
| UP-33 | Compress video | The default writes HEVC in an MP4 on a Mac. That plays on Apple devices and not in every browser or in WhatsApp web. Documented in KNOWN-LIMITATIONS, but the result card does not say it. | Open |
| UP-34 | Convert audio | A truncated MP3 converts to a WAV 18 s long from 30 s. The card warns ("Heads up"), which is good; it comes first but is not styled as a warning. | Open |
| UP-35 | Compress image | A tiny file ("already well compressed, 69 bytes against 69, nothing was written") is correct, but exits 0 with no output, so a batch script cannot tell it from a success. | Open |
| UP-36 | Settings | Every row looks like a control (Appearance, Reduce Motion, Network) and none can be changed; the page is a status list. A person looking for a dark-mode switch finds none. | Open |
| UP-37 | Find, Nearby | The empty pages said "comes in a later phase". "Phase" is the plan's word, not the person's; Nearby said sharing is not here while `panoply share` works. | Fixed (says not in the app yet, and names the command) |
| UP-39 | Erase object, Remove watermark | A 500 by 400 pixel fill (on a 2400 by 3000 picture) shows a hard rectangular seam and stretched texture; the fill model works at 512 and the area is scaled up. Small areas on plain backgrounds are fine. | Open |
| UP-40 | Remove background | A small piece of the old background survives in the gap between the arm and the torso (white and orange, lower left of the cutout). Edges, hair and the flag patch are otherwise clean. | Open |
| UP-41 | Upscale | 4x is far better than bicubic but the skin is smooth and painterly and the flag's stars melt into a pattern. Fine for a profile picture, not for a print. The card does not say it is a model's guess. | Open |
| UP-42 | Modules | Network tools show a "Network" pill and the words "Uses the network" in the same row; the pill adds nothing and the page is crowded on a narrow window. | Open (the model tools now read "Model download", see UP-47) |
| UP-43 | Compress, Convert, Resize and the other picture tools | An animated GIF or WebP stops with "Choose \"first frame\" (animation=first) to keep one picture ... Panoply does not write animated files yet." That is the command-line spelling in an app window, the option is under More options, and KNOWN-LIMITATIONS says animated inputs "use the first frame and say so". One of the two should change. | Open |
| UP-45 | Command bar | With an MP3 on the bar the first suggestion is "Compress video", then "Convert video or audio". The tool names say video for sound files. | Open |

---

## Windows and Linux

The tools themselves were run on macOS only; the desktop smoke workflow opens the shipped app on a real desktop and does not drive tools. Run 38708566257 (Desktop smoke, latest prerelease v0.1.2-beta, 2026-10-15):

- **Windows (Server 2025, 1920 by 1080): pass.** The installed app starts, shows a window titled Panoply and a notification-area icon, the command bar opens centred (736 by 468 at 22% from the top), the process holds 42.8 MB and 0.3 s of CPU, dark theme is followed, closing the window keeps the tray icon and Quit from the tray exits.
- **Linux (Ubuntu 24.04, Xvfb, openbox, lavapipe): fail, a known one.** The window opens in 0.5 to 1.5 s at 1160 by 720 with 171 to 181 MB, but both scenarios end with "command bar still on screen after the hotkey toggled it off". This is the hotkey race that the LEDGER lists as "Ready for the next release (v0.1.3)". The shipped v0.1.2-beta AppImage still has it. Until v0.1.3 is tagged, Linux fails the launch gate's real-desktops line.
- Also on Linux: `exited_after_close` is false in both scenarios (the process stays after its window is closed). The smoke script does not count it as a failure; whether that is intended should be decided and written down.
- **Not covered, needs a person at a real machine** (the gate already says so): HEIC, RAW and JPEG XL on Windows and Linux (refused in plain words by design), Windows OCR, the hardware video encoders (Media Foundation, NVENC, VA-API), the ffmpeg download path, and every state of every panel on Windows and Linux. The offscreen renderer in this pass is macOS only.
