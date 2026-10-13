# Recent media-utility launches (Oct 2025 to Oct 2026), research as of 2026-10-13

## Method and limits (read first)

- 36 WebSearch queries (the per-turn search budget ran out at 36), mixing natural phrasing, Hinglish phrasing, site:x.com, site:twitter.com, Product Hunt, Reddit, Indie Hackers, Peerlist, Uneed, Microlaunch, DevHunt. Plus roughly 25 queries against the Hacker News Algolia API (`hn.algolia.com/api/v1/search`, Show HN only, created after 2025-10-01) because it is the one source with exact dates, points and comment counts.
- X/Twitter could not be read. The search tool ignores `site:x.com` and `site:twitter.com` and returned zero tweets in 10+ attempts; `x.com/search` returned HTTP 402; Nitter is down (its own notice says X sent cease and desist letters in Aug and Sep 2026); Reddit blocks fetching. So **no tweet-level engagement is reported anywhere below**. Where a maker's X handle appears on a product page it is listed, so the launch tweet can be found by hand.
- Engagement shown is the best public number I could read: HN points/comments, Product Hunt upvotes, GitHub stars (all as of fetch time, 2026-10-13).
- Dates: HN post date when there is an HN post; Product Hunt dates marked "~" are inferred from "launched Nmo ago" on the page.
- "tools" = what the product's own site lists. n/s = not stated on the page.
- Not verified: Brevio (fetch returned 402) and imageconverter.dev (now redirects to resizeimage.dev, which returned 500).

## A) Products

HN link format: `news.ycombinator.com/item?id=N`. PH = Product Hunt.

### Video and editors

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| VidStudio | vidstudio.app/video-editor | 2026-04-21 | HN 47847558 | 301 pts, 109 comments | local (IndexedDB projects) | 1 editor + 8 sibling pages (trim, compress, extract audio, crop, text-based editor, CapCut alt) | "browser video editor that doesn't upload your files" |
| Ez FFmpeg (ezff) | npmjs.com/package/ezff, github.com/josharsh/ezff | 2025-12-27 | HN 46400251 | 420 pts, 198 comments (repo showed 2 stars; commenters hit a 404 at first) | local CLI, pattern match, no AI | ~20 verbs (convert, compress, trim, extract audio, resize, speed, reverse, mute, rotate, flip, thumbnails, merge, crop, fps, loop, add audio, grayscale, stabilize, denoise) | plain English to ffmpeg, dry-run shows the command |
| FFmpeg WebCLI | github.com/tejaswigowda/ffmpeg-webCLI | 2026-06-04 | HN 48404224 | 86 pts, 43 comments, 1.5k stars | local, offline PWA | 8 groups (convert/compress, edit, effects, audio, subtitles+Whisper captions, output, raw ffmpeg panel, batch+chain) | full ffmpeg in the browser, installable |
| Palmier Pro | github.com/palmier-io/palmier-pro | 2026-07-23 | HN 49022911 | 191 pts, 41 comments, 14.6k stars, YC S24 | editor local; generative models likely cloud (n/s) | 1 native Swift editor + MCP server | "macOS video editor built for AI"; later binaries are proprietary |
| FableCut | github.com/ronak-create/FableCut | 2026-07-09 | HN 48845422 | 98 pts, 62 comments, 724 stars | local (localhost:7777) | 1 editor: multitrack, color grade, chroma key, captions, MCP + REST | browser editor an agent can drive |
| Weave | github.com/thiagoalbrecht/weave | 2026-07-26 | HN 49058396 | 17 pts, 4 comments, 30 stars | local (ffmpeg.wasm) | 1 timeline editor that compiles to one ffmpeg command | timeline to ffmpeg |
| Squish | usesquish.me | 2026-06-09 | HN 48459491 | 1 pt | local (browser) | 1 compressor: 8 platform presets, 3 formats, 4 quality levels | compress for X, WhatsApp, Discord etc |
| Tools.video (SubEasy) | tools.video | 2026-03-18 | HN 47426865 | 2 pts | local ("zero uploads") | 26 (video 15, audio 5, image to video, GIF, Live Photo, media info) | one site, every video op; x.com/SubEasy_AI |
| PauseSweep | PH /products/pausesweep | ~2026-06 | PH | 4 upvotes | local | 1 (remove pauses) | Mac, one-time price |

### Screen capture and recording

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| OpenScreen | github.com/siddharthvaddem/openscreen | 2026-04-10 (Hongkiat write-up, v1.2.0) | github | **39.9k stars**, 3.2k forks, MIT, repo now archived (community fork continues) | local | ~10 (record, mic+system audio, webcam bubble, auto zoom, cursor smoothing, on-device captions, backgrounds, motion blur, annotations, MP4/GIF export) | free Screen Studio alternative |
| Capptivo | capptivo.com | 2026-08-02 | PH /products/capptivo | 333 pts, #2 of the day | local | ~10 (auto zoom, face cam, on-device captions, annotations, narration, backgrounds, device presets), mac/win/linux, MIT | OSS recorder + demo editor; x.com/idboussadel37 |
| Capso | github.com/lzhgus/Capso | 2026-04-11 | PH /products/capso | 264 pts, #3 of the day, 1.4k stars | local (optional own R2 bucket) | ~12 (7 capture modes, scroll capture, annotate, beautify, OCR, capture+translate, pin, history, MP4/GIF editor) | CleanShot X alternative; x.com/lzhgus |
| Glideo | PH /products/glideo | ~2026-07 | PH | 145 pts, #11, 249 followers | local | recorder with auto zoom, pause cut, facecam, 16:9 or 9:16 4K export | free, no watermark, Ko-fi funded |
| Kapshot | kapshot.cloud | ~2026-09-25 | PH /products/kapshot | 75 pts, #17 | n/s | auto zoom + cursor cleanup recorder | "looks like you edited it"; x.com/aryanthakor24 |
| Take (Crest Notch) | crestnotch.app/take | 2026-07-31 | HN 49121729 | 2 pts | local | recorder + auto zoom + dead-air speedup + virtual camera ("Take Camera") + cabled iPhone capture | $24.99 once; solo dev |
| Shotglass | shotglass.app | 2026-05-13 | HN 48119092 | 6 pts, 4 comments | local | screenshots + recording, 3D MacBook scenes, MCP control | "demos like an Apple commercial"; x.com/jakemanger |
| ascreenrecorder | ascreenrecorder.com | 2026-09-29 | HN 49890716 | 2 pts | local | auto zoom, templates, 45+ caption styles, 4K export | $49 once |
| BetterCapture | bettercapture.app | 2026-02-08 (also 02-14) | HN 46934504 / 47014431 | 4-5 pts, 1.7k stars | local | recorder: ProRes 4444/HEVC/H.264, alpha, system+mic audio | free OSS Mac recorder |
| GifJot | gifjot.com | 2026-08-06 | HN 49195445 | 3 pts | local | 3 (area to GIF, silent MP4, PNG) + markup | menu bar GIF recorder |
| Synapse | synapsemac.com | 2026-06-02 | HN 48368142 | 18 pts, 17 comments | n/s | 12 (recorder, screenshot studio, clipboard, keep awake, OCR, QR, notch shelf, speed monitor...) | 4 MB native Mac utility belt; $14.99 lifetime; x.com/SynapseMacApp |
| CopyLasso | copylasso.com | 2026-08-19 | HN 49365721 | 9 pts | local (Apple Vision) | 1 (screen OCR + QR/barcode) | free MIT |

### Transcription and dictation

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| BetterWispr | betterwispr.com | 2026-10-11 | HN 50042333 | **208 pts, 126 comments in 2 days** | local by default | dictation + 9 speech models + cleanup levels + per-app tone + voice commands + notetaker | free OSS Wispr Flow alt; x.com/code_kartik |
| Ghost Pepper | github.com/matthartman/ghost-pepper | 2026-04-06 | HN 47666024 | **467 pts, 200 comments**, 3.2k stars | local | dictation, meeting transcription, LLM cleanup, OCR | hold Control to talk |
| FreeFlow | github.com/zachlatta/freeflow | 2026-02-16 | HN 47040375 | 277 pts, 132 comments, 2.9k stars | cloud (Groq) by default, local optional | dictation + edit-selected-text-by-voice + vocab | free alt to Wispr Flow, Superwhisper, Monologue |
| Scriber Pro | scriberpro.cc/hn | 2025-10-15 | HN 45591222 | 137 pts, 113 comments | local | 1 (6 input formats, 8 export formats) | long-file offline transcription on Mac |
| Trace | traceapp.info | 2026-06-13 | HN 48521236 | 205 pts, 84 comments | local ("0 uploaded ever") | ~16 (capture, translate, key moments, speaker labels, summaries, CLI, export with auto redaction) | offline meeting transcripts |
| Parrot | openparrot.app | 2026-09-30 | HN 49910328 | 39 pts, 38 comments | local or own keys | bot-free recorder, live assistant, reports, PDF export | GPL-3.0; x.com/uturantekin |
| yapsnap | github.com/kouhxp/yapsnap | 2026-05-20 | HN 48214399 | 97 pts, 49 comments, 377 stars | local | 1 CLI (URL/file to text, diarization) | CPU-only, YouTube/TikTok/X/Reels links |
| Qwen Scribe | github.com/VladUZH/qwen-scribe | 2026-07-29 | HN 49098260 | 97 pts, 21 comments, 219 stars | local | 2 (file transcribe + SRT, dictation) | Apple Silicon, Qwen3-ASR |
| Yap (Frigade) | github.com/FrigadeHQ/yap | 2026-07-27 | HN 49073834 | 104 pts, 44 comments, 454 stars | local | 1 dictation, no model download | uses Apple Speech framework |
| Vocal Slice | vocalslice.com | 2026-08-10 | HN 49245021 | 51 pts, 41 comments | local | 1 (cut audio by selecting transcript text) | GPL-3.0, free |
| Kekoso | kekoso.app | 2026-09-02 | HN 49539043 | 8 pts, 3 comments | mostly local | dictation, file + YouTube transcription, call recording, MCP server with 12 tools | $29 once; x.com/a_chmerev |

### Audio and TTS

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| AudioMass | audiomass.co | 2026-05-24 | HN 48258015 | **553 pts, 120 comments**, ~3.0k stars | local | multitrack editor (tool list n/s) | free OSS web audio editor |
| Audionaut | github.com/kvoltmer/Audionaut | 2026-10-02 | HN 49931031 | 155 pts, 47 comments, 254 stars | local | editor + demucs stem split + CLI + MCP | agent-drivable DAW-lite |
| KittenTTS | github.com/KittenML/KittenTTS | 2026-03-19 | HN 47441546 | **561 pts, 181 comments, 15.9k stars** | local CPU | TTS, 47 voices, 20 languages, voice cloning | smallest model under 25 MB |
| Moonshine | github.com/moonshine-ai/moonshine | 2026-02-24 | HN 47143755 | 316 pts, 81 comments, 11.2k stars | local | STT, TTS, intent | open STT beating Whisper large v3 |
| demucs-rs | github.com/nikhilunni/demucs-rs | 2026-03-03 | HN 47234566 | 19 pts, 150 stars | local (WASM/WebGPU, CLI, VST3) | stem separation (4 and 6 stem) | Demucs in Rust, in the browser |
| Make Loops | makeloops.online | 2026-01-05 | HN 46502148 | 70 pts, 31 comments | n/s | audio loop maker | tiny single-purpose |

### Images, converters, background removal

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| OnlyJPG / AnyJPG | anyjpg.com | 2025-10-17 | HN 45615454 | 64 pts, 42 comments | local (WASM) | 1 tool, 30 input formats (HEIC, RAW, PSD, JXL, EXR...) | "any image to JPG" |
| imageconverter.dev | resizeimage.dev | 2025-10-24 | HN 45693904 | 48 pts, 38 comments | claims local | not verified | privacy-first converter |
| WebPtoPNG | bulkwebptopng.com | 2025-12-24 | HN 46379054 | 22 pts, 19 comments | local | 1 (2 directions, 0.5x to 4x scale, bg fill) | single-job site |
| Brevio | brevio.pro | 2026-06-17 | HN 48568023 | 89 pts, 32 comments | claims no upload | "184 tools" per title | not verified |
| ToolKuai | toolkuai.com | 2026-01-31 | HN 46837885 | 8 pts, 5 comments | local | 15 (bulk video compress, video to MP3, bulk OCR, bulk bg remover, HEIC, PDF set, QR) | x.com/MaxSplashWang |
| MultiMatte (Feyn) | usefeyn.com/blog/multimatte | 2026-09-10 | HN 49645803 | 57 pts, 8 comments | weights local, or browser demo | 1 model: text-prompted background removal | "keep the dog, drop the rest" |
| bgbgone | github.com/Arthur-Ficial/bgbgone | 2026-06-07 | HN 48432899 | 2 pts, 19 stars | local (Apple Vision) | bg remove + 49 Core Image filters, HTTP mode | Unix-style CLI |
| GeoImageTagger | geoimagetagger.com | 2026-07-24 | HN 49036339 | 50 pts, 23 comments | upload | 8 (metadata viewer, compressor, converter, EXIF remover, editor, AI location finder, image editor, watermark) | x.com/GeoImageTagger |
| Make your logo brighter than white | soverybright.com | 2026-08-22 | HN 49402521 | 81 pts, 88 comments | upload (memory only) | 1 (HDR gain-map JPEG, BT.2100 PQ for LinkedIn) | HDR logo trick |
| OldRoll | oldroll.io | 2026-10-09 | HN 50016840 | 16 pts, 5 comments | upload (browser) | retro filters, date stamp, light leaks, collage | Y2K/digicam look |
| photoc | github.com/ahmetomerv/photoc | 2026-10-03 | HN 49944196 | 65 pts, 10 comments, 130 stars | local CLI | 13 commands (review, exif, stats, timeline, duplicates, focus, rename, sort, compress, contact, scrub...) | CLI for photographers |
| SCM | github.com/allenv0/SCM | 2026-10-04 | HN 49952111 | 183 pts, 74 comments, 478 stars | local | 5 search modes (files, scenes, OCR, dialogue, local LLM) | search every photo and video frame |
| Mail Memories | mailmemories.com | 2026-07-02 | HN 48762000 | 103 pts, 58 comments | local | 1 (rescue photo attachments from Gmail) | free 100 photos, $29 once |
| textsnap | github.com/kouhxp/textsnap | 2026-06-05 | HN 48410841 | 19 pts, 17 comments, 191 stars | local | 1 OCR CLI (markdown tables, clipboard, URL) | CPU only |

### Compressors, downloaders, PH-only listings

| Product | URL | Date | Launch / listing | Engagement | Local or upload | Tools | Angle |
|---|---|---|---|---|---|---|---|
| TinyFast | tinyfast.app | 2025-10-18 | HN 45626269 | 3 pts | local Mac app | 1 app: images, video, PDF, docs, audio | free 5 files/day, 24.99 EUR lifetime |
| MediaOptim | PH /products/mediaoptim-mobile | ~2026-05-08 | PH | 3 upvotes | local | image + video compress, batch | free unlimited |
| ClipUtils | PH /products/clip-utils (hunted.space/product/cliputils) | n/s | PH | 2 upvotes, 1 comment | local (ffmpeg.wasm) | 7 (trim, compress, video to GIF, HEIC to JPG, bg remove, compress image, resize image) | resize to a target KB |
| H5Kit | PH /products/h5kit | ~2026-07 | PH | 2 upvotes | local WASM | 4 (image compress, convert, audio, video) | privacy toolkit |
| ConvertLocal | PH /products/convertlocal | ~2026-07-16 | PH | 3 upvotes | local | video/audio convert, image resize+convert, AI image gen | zero uploads |
| MediaForge | PH /products/mediaforge-6 | 2026-06-14 | PH | 1 upvote | local WASM | 30+ formats, plays MKV/AVI/WMV/FLV | solo side project |
| PhotoFit | PH (hunted.space/product/photofit) | n/s | PH | 10 upvotes, 1 comment | local | 1 (exact KB or pixels for passport, visa, Aadhaar, PAN) | Android app too |
| Enlarger | PH /products/enlarger | ~2026-07 | PH | 6 upvotes | local, offline | 1 upscaler, deterministic, no AI look | $39 once, Mac + Windows |
| VODForge | getvodforge.com | 2026-09-06 | HN 49590354 | 39 pts, 13 comments | local | download, library, watch, files, recovery (yt-dlp + FFmpeg) | MIT desktop UI |
| OpenBrief | github.com/tantara/openbrief | 2026-05-25 | HN 48272393 | 93 pts, 18 comments | local-first | video downloader + summarizer | n/s |
| Menura | menura.download | 2026-10-10 | HN 50030988 | 7 pts | n/s | download video/audio/photos from 17 sites + convert + compress | 50 links at once |
| compress.lol (outside window) | compress.lol | 2025-09-11 | r/selfhosted | ~1.4k upvotes, 162 comments (from a search snippet, not opened) | local (ffmpeg.wasm) | 1 | included only as a benchmark for what a viral compressor looks like |

## B) Tasks they offer that are NOT in MODULES.md

Image
- Text-prompted background removal ("keep the dog") with alpha matte output (MultiMatte).
- Visible AI watermark removal (Gemini, Sora corner marks) and C2PA/AI-metadata check and strip (several HN launches, each 1 to 4 pts).
- Write GPS and EXIF location, AI location guess, AI tags and descriptions into photo metadata (GeoImageTagger).
- Photo culling: interactive pick/reject on JPEGs, focus/sharpness check, EXIF stats and timeline (photoc).
- Photo forensics: "is this photo edited" check (HN 49641861, 18 pts).
- Rescue photo attachments out of an email account (Mail Memories).
- Make an HDR gain-map JPEG so a logo glows brighter than white (soverybright).
- Retro/film looks: Y2K digicam, date stamp, light leaks, grain (OldRoll).
- Device-targeted image optimization for e-ink manga (KCC, HN 49879758, 83 pts).
- OCR to structured Markdown with tables, LaTeX and code (textsnap, OCR Buddy).
- Identify a font from an image (What the Font, HN 45575634, 31 pts).
- Rebuild an image from primitive shapes (primitive-playground, 43 pts).

Video
- Edit video by editing the transcript, with filler-word removal (VidStudio, ScriptEdit, Poddie).
- Video to Live Photo and Live Photo merge, MP3 plus image to MP4 (Tools.video, neuralframes).
- Frame interpolation as its own "Frame Booster" tool (Tools.video).
- Convert bitmap subtitles (PGS) to SRT by OCR (HN 47415044, 38 pts).
- Search spoken words and scenes inside videos, by meaning, with timecodes (SCM, sentrysearch).
- Auto-zoom on clicks, cursor smoothing, styled canvas, 3D device scenes for screen recordings (OpenScreen, Capptivo, Capso, Glideo, Take, Shotglass).
- Virtual camera that sends a staged screen to Meet/Zoom (Take).
- Capture a cabled iPhone screen (Take).
- Voice-tracked teleprompter (promptme-ai).
- Language-learning clips and flashcards from your own videos or audio (SubSmith 78 pts, LingoChunk 92 pts).

Audio and voice
- Cut audio by selecting words in a transcript (Vocal Slice).
- Dictation cleanup: filler removal, per-app tone, "scratch that", custom vocabulary learning, edit selected text by voice (BetterWispr, FreeFlow).
- Bot-free call and meeting recording of mic plus system audio, key-moment flags, live recap, auto-redacted export (Trace, Parrot, Kekoso).
- Audio to MIDI (Muscriptor, HN 49389846, 58 pts).
- Loop maker (Make Loops, 70 pts).
- Voice cloning TTS and expression tags (KittenTTS).
- Play legacy dictation formats DSS/DS2 (speechtext.ai).
- Multitrack timeline audio editing (AudioMass, Audionaut).
- Audio metering and visualization (OpenMeters).

Files and agents
- Natural-language to exact ffmpeg command with dry-run preview showing the command (ezff, Media Convertifly).
- MCP/REST control so an agent can run the editor or recorder (Palmier Pro, FableCut, Audionaut, Shotglass, Kekoso).
- Per-recipient invisible watermark for images and video, not only PDF (Etchv, HN 50005024).
- Cull duplicates by content hash while renaming (SCM, photoc duplicates).
- Rename-to-convert: change a file extension in Finder to convert it (HN 47340433, a Mac app, 2 pts).

## C) Standout traction and why

Numbers are as read on 2026-10-13. No X numbers (see limits).

1. **OpenScreen, 39.9k GitHub stars.** Free, MIT, Mac/Windows/Linux Screen Studio clone; the repo is now archived and a community fork carries on. Why: a paid, subscription-priced hero app with an obvious feature set (auto zoom, cursor smoothing, backgrounds) that one dev could clone. It then set off a wave: Capptivo (PH 333 pts, #2), Capso (PH 264, #3), Glideo (PH 145), plus about 20 Show HN recorders, almost all at 1 to 6 pts. Source: github.com/siddharthvaddem/openscreen, hongkiat.com post dated 2026-04-10.
2. **Local dictation is the hottest media category.** BetterWispr 208 pts and 126 comments in two days (HN 50042333), Ghost Pepper 467/200 and 3.2k stars (HN 47666024), FreeFlow 277/132 and 2.9k stars (HN 47040375), Yap 104/44 (HN 49073834), Qwen Scribe 97/21, Scriber Pro 137/113. The HN dictation search alone returned 226 Show HNs since Oct 2025, so it is also the most crowded. Why: paid incumbent (Wispr Flow), clear privacy pitch, small models now good enough on Apple Silicon.
3. **Small open models.** KittenTTS 561/181 and 15.9k stars (TTS under 25 MB on CPU); Moonshine 316/81 and 11.2k stars (STT that beats Whisper large v3 on its own claim). Why: "runs on a CPU, no API" is a headline by itself.
4. **Agent-drivable editors.** Palmier Pro 14.6k stars and 191 pts (HN 49022911, YC S24, later releases moved to proprietary binaries); FableCut 98/62; Audionaut 155/47; Shotglass and Kekoso ship MCP servers. Why: the editor becomes the tool an LLM calls.
5. **AudioMass 553 pts, 120 comments, ~3.0k stars** and **VidStudio 301/109.** Both are plain "full editor in a tab, nothing uploaded" products with a verifiable privacy claim (network tab).
6. **Ez FFmpeg 420 pts, 198 comments** (HN 46400251) for a tool whose repo had 2 stars when fetched: the idea ("plain English to ffmpeg") carried the post, not the repo.
7. **Meeting transcripts, offline.** Trace 205/84, Scriber Pro 137/113, Parrot 39/38.
8. **Weak signal for generic converter and compressor sites.** Of the ~30 compressor, converter, bg-remover and upscaler Show HNs found, almost all sit at 1 to 8 pts. Exceptions: OnlyJPG 64, imageconverter.dev 48, Brevio 89, WebPtoPNG 22. Product Hunt shows the same: ClipUtils 2, H5Kit 2, MediaForge 1, ConvertLocal 3, PhotoFit 10. The pattern that wins is a named incumbent to replace, or a very specific job (exact-KB photos, HDR logos, cut audio by text), not "another local converter".

## Open items

- The user can send a follow-up to continue, since the search budget ran out. Highest value next: find the launch tweets for OpenScreen, Capptivo (x.com/idboussadel37), Capso (x.com/lzhgus), BetterWispr (x.com/code_kartik), Shotglass (x.com/jakemanger), Kekoso (x.com/a_chmerev), Parrot (x.com/uturantekin), Kapshot (x.com/aryanthakor24) by hand and note likes.
- Reddit (r/macapps, r/SideProject, r/InternetIsBeautiful) and Indie Hackers were reachable only through search snippets, so no post-level numbers except compress.lol.
