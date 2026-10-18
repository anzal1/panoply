# Panoply specs: Video (section 4) and Audio (section 5)

Status: draft, 2026-10-13. Template from FLOWS.md. Shared capabilities (entry points, estimate, originals untouched, batch, chains, presets, result card, progress, plain errors, privacy line, keyboard, history, accessibility) are not repeated. Every spec says only what is special. Every spec has a `Budgets:` line after its Pattern line: "standard" when the module fits the table in FLOWS.md, otherwise the exception and the reason. Models follow the Models budget (load on first use in a job, unload after 60 s idle, size shown before the first download). Model sizes are approximate and re-read from each model's `MODEL.json`.

Numbers in this file are targets or gates, never measurements. No benchmark has been run yet. Where a fact about a model or format came from a source it is cited; where it is unverified it says "verify".

---

## 0. Conventions used by every spec (referenced by code, not repeated)

### 0.1 Engine policy

- **ENG.** ffmpeg LGPL build, dynamically linked (libavformat, libavcodec, libavfilter, libswscale, libswresample). No GPL or nonfree parts. Shipped software codecs: SVT-AV1, libvpx (VP8, VP9), openh264, libmp3lame, libopus, libvorbis, native AAC, native FLAC. Not shipped: x264 and x265 (GPL), fdk-aac (nonfree). Consequences: HEVC output is hardware-only (offer H.264 or AV1 when no hardware HEVC encoder exists); AAC on Apple platforms goes through AudioToolbox. openh264 patent coverage applies to Cisco's own binary, so it is fetched from Cisco at first use, not bundled (verify at packaging time).
- **HWL (hardware ladder).** Always try hardware first, fall back to software, and say which ran on the result card. macOS and iOS: VideoToolbox. Windows: Media Foundation, then NVENC, QSV or AMF when present. Linux: VA-API, then NVENC. Android: MediaCodec. Software fallback order: H.264 openh264, AV1 SVT-AV1, VP9 libvpx. Hardware encoders do not do true two-pass, so size targets use SIZE (below) on hardware and real two-pass on SVT-AV1 and VP9.
- **SIZE (compress to a size).** `video_bps = (target_bytes * 0.98 * 8 / duration_s) - audio_bps`, where 0.98 reserves container overhead, audio is kept if already stereo AAC or Opus at or under 128 kbps, otherwise re-encoded (96 kbps stereo, 64 kbps mono, set per module), and duration comes from the probe, not the file size. If `video_bps` falls below the per-codec floor measured on the corpus (set at the VMAF knee, stored in the preset file), lower resolution first, then frame rate, then quality, and say so in the preview. Software two-pass where the codec supports it. On hardware: single pass constrained bitrate, then verify; if the output exceeds the target, rerun once with bitrate scaled by `target / actual * 0.97`. The estimate shows a band, not a point.
- **Probe profile (P).** On drop the host probes without decoding: container, codec, profile, level, pixel format, bit depth, color primaries, transfer, matrix, HDR10 mastering data, HLG, Dolby Vision profile and compatibility id, display-matrix rotation, `r_frame_rate` versus `avg_frame_rate` and a 200-packet pts delta sample (VFR), keyframe map, audio tracks (codec, channels, layout, language, disposition), subtitle tracks (text or bitmap), chapters, attachments, duration, size, bitrate, moov position, MV-HEVC second view or `vexu` and `hero` atoms (spatial). Specs below say what each module does with P, not how P is built.
- **Loudness.** Measured with libebur128 (MIT, verify), not ffmpeg `loudnorm`. Gain is linear when peaks allow, a true-peak limiter otherwise, never dynamic pumping.

### 0.2 House rules for the edge cases (referenced as ROT, VFR, ...)

- **ROT.** Rotation lives in the display matrix. Preserve the flag on any stream copy. Burn it into pixels only when the target cannot carry it (GIF, WebM, some players) or a filter needs upright pixels (crop, overlay, watermark). Never double-rotate: after a bake the flag is cleared.
- **VFR.** Flag VFR when pts deltas vary beyond the nominal frame duration or `r_frame_rate` and `avg_frame_rate` disagree (typical for phone video). Same-container operations keep original timestamps. Operations that need constant rate (GIF, speed, merge with CFR clips, interpolation, some editors) convert to CFR at the nearest standard rate to `avg_frame_rate`, dropping or duplicating minimally, and say so. Audio is never resampled to hide drift.
- **HDR.** Detect HDR10, HLG, Dolby Vision. Default: keep HDR when the target is 10-bit HEVC or AV1 and the encoder is available, with mastering metadata and tags carried over; otherwise tone-map to SDR BT.709 (libplacebo where Vulkan exists, ffmpeg zscale or tonemap elsewhere, BT.2390 curve) and say "converted to SDR". Dolby Vision profile 8.4 from iPhone has an HLG-compatible base layer, treated as HLG. Profile 5 has no compatible base layer, so any re-encode must apply the RPU through libplacebo or refuse with a plain message; plain decoders show purple and green. Stream copy keeps Dolby Vision intact. Verify DV paths on real files in the spike.
- **B10.** 10-bit sources stay 10-bit for HEVC, AV1 and H.264 High 10 where the hardware encodes it; to 8-bit H.264 convert with dithering, never truncation. 4:2:2 and 4:4:4 camera files to a hardware encoder that lacks them: convert to 4:2:0 and say so.
- **BIG.** Files over 4 GB: never loaded whole, never memory-mapped fully, MP4 output uses 64-bit offsets (co64), progress by bytes. Warn when the target is FAT32 or a service with a known cap. Free disk space is checked against the estimate before starting.
- **MOOV.** Final MP4 and MOV outputs are written with the moov atom at the front (faststart) so they stream and upload. Input with a missing or trailing-truncated moov: stop, say "this file looks cut off", and offer Repair (4.18) inline.
- **AUD.** Multiple audio tracks: stream-copy operations keep all tracks; transcodes keep all tracks and the default flag; the preview lists tracks by language and title, and commentary or descriptive tracks (by disposition) are off by default only for size targets.
- **SPATIAL.** iPhone spatial video is MV-HEVC (two views in one stream). FFmpeg 7.1 added MV-HEVC decoding; no ffmpeg encoder is known (verify on a real file). Stream copy preserves both views. Any re-encode that cannot write MV-HEVC says "spatial video will become flat 2D (left eye)" and asks. On macOS and iOS, an AVFoundation writer path may write MV-HEVC (single source reports it, verify with Apple documentation).
- **SMART.** Smart cut: copy whole GOPs, re-encode only the partial GOP at each cut edge with parameters matched to the source (codec, profile, level, bitrate, color tags, SPS and PPS compatible with the copied part), then splice. If the edge cannot be matched (Dolby Vision profile 5, an encoder missing for the codec, open GOP with unavailable references), snap to the nearest keyframe and say so. Result card states "N frames re-encoded".
- **COPY.** Prefer stream copy. A module that can copy tries copy first, and the preview says "No re-encode, lossless, takes seconds".

### 0.3 Models (on-device, downloaded once from the release mirror)

| Job | Model | License (checked) | Notes |
|---|---|---|---|
| Speech to text, default | Whisper via whisper.cpp | MIT code and weights (Whisper README) | Multilingual, timestamps, offline |
| Speech to text, live and English | Moonshine | English models MIT; other languages under the Moonshine Community License (sources disagree: non-commercial versus under 1M revenue) | Ship English only until the full license text is read |
| Speech to text, European batch | Parakeet TDT 0.6B v3 | CC-BY-4.0, 25 European languages (model card) | Attribution required; candidate only |
| OS speech, optional | Apple SpeechAnalyzer where present | OS service | Benchmark against Whisper before using |
| Voice activity | Silero VAD | MIT (LICENSE file checked; some README badges say CC BY-NC, reconcile before release) | Cut points, silence, dictation endpointing |
| Diarization | pyannote segmentation 3.0 plus an embedding model | segmentation 3.0 is MIT but gated on Hugging Face (contact form); embedding model license unchecked | Do not mirror until redistribution is confirmed; fallback below |
| Text to speech, default | Kokoro-82M | Apache-2.0 (model card), 82M parameters | Default voice pack |
| Text to speech, small | KittenTTS nano, micro, mini (15M, 40M, 80M; 25 MB int8 to 80 MB) | Code Apache-2.0; weights licensed per model repo, KittenTTS 2 uses the Stellon Labs Community License | Phone and low-end tier only after reading each model repo license |
| Stem separation | Demucs htdemucs, htdemucs_ft, htdemucs_6s | MIT; Meta repo archived 2025-01-01, maintained fork at adefossez/demucs | `_ft` is about 4x slower per the project; export to ONNX or Core ML |
| Noise suppression | DeepFilterNet | MIT or Apache-2.0 for code; weights license verify | Primary |
| Noise suppression, light | RNNoise | BSD-3 (verify) | Live paths, tiny devices |
| Frame interpolation | RIFE | MIT (LICENSE file checked); weights license per release, verify | Smooth motion |
| Audio to MIDI | Basic Pitch | Apache-2.0 (one secondary source, verify) | |
| Subtitle translation | Opus-MT class models | Mostly CC-BY-4.0 (verify per pair); NLLB excluded, weights are non-commercial | Per language pair download |
| Subtitle OCR | Tesseract through the shared OCR service | Apache-2.0 | PGS and VobSub decoded by ffmpeg |
| Faces, subject, inpainting | Reuse the Images module detectors and the Object eraser model | per Images spec | No new model for video, plus temporal tracking and smoothing |
| Meeting notes, dictation rewrite | None bundled. OS model first (Apple Foundation Models, Gemini Nano, Phi Silica), else the user's own Ollama, LM Studio or API key, else hidden | n/a | Decision: Panoply ships no language model |
| Echo cancellation | WebRTC AEC3 | BSD-3 (verify) | Meeting recorder, mic hears speakers |
| Pitch and tempo | SoundTouch | LGPL-2.1 (verify) | Rubber Band excluded (GPL) |
| Phonemiser | non-GPL English G2P | espeak-ng is GPL-3.0 (verify), keep out of the default path | Kokoro non-English fallback |

Diarization fallback: mic and system audio are separate channels in the Meeting recorder, so "You" versus "Remote" needs no model; other files get speaker labels only when the diarization row clears.

### 0.4 Benchmarks and corpus

All benchmarks run in public-repo CI (not on developer machines) against one fixed corpus, published with the results: iPhone HEVC HDR 4K60 VFR with rotation, Android portrait H.264 VFR, Dolby Vision profile 5 and 8.4, 10-bit 4:2:2 camera, screen recording with long idle, GoPro, interlaced DVD rip, MKV with five audio and three subtitle tracks including PGS, MP4 with trailing-truncated moov, a file over 4 GB, iPhone spatial clip, mono and stereo speech, music with clipping, a podcast with hum. VMAF is libvmaf at equal output size. "Speed versus ffmpeg" means wall time with the same encoder and settings.

### 0.5 Naming

CLI: `panoply video <verb>`, `panoply audio <verb>`. MCP: `video_<verb>`, `audio_<verb>`. Each spec lists its own verb.

---

## 4. Video

### 4.1 Trim and cut

- **Pattern / Job:** C. "Cut out the parts I do not want."
- **Budgets:** standard. Untouched GOPs are copied, so cost is disk speed and memory is constant.
- **Inputs:** one video, or many for the same cuts.
- **On drop:** P, plus the keyframe map, a thumbnail strip and waveform built lazily. Arms the last used mode (keep range or remove range). Black or silent head and tail are detected and offered as one-click cuts.
- **Options:** Mode: keep or remove ranges (keep). Precision: Exact (SMART, default) or Keyframes only (COPY, instant). If every cut already lands within one frame of a keyframe, copy is used silently. Output: one file, or one file per range (one file).
- **Preview / Output / Result:** timeline with keyframe ticks and "No re-encode, 3 s" or "14 frames re-encoded at 2 edges". Same container and name "name (trimmed).mp4". Result: new duration, size, frames re-encoded.
- **Edge cases:** VFR and edit lists (cut on pts, keep A/V offset, B-frame negative dts handled); open GOP HEVC (drop unreferenced leading pictures); Dolby Vision profile 5 edges snap to keyframes; ROT preserved; AUD all tracks cut together; SPATIAL kept on copy, snaps to keyframe when no MV-HEVC encoder; MOOV broken goes to Repair; BIG streamed.
- **Quality bar:** untouched GOPs are bit-identical (frame hash test), cut accuracy within one frame, and total time is dominated by disk speed, faster than CapCut and Clipchamp exports that re-encode the whole file. Edge frames score VMAF at or above their neighbours.
- **Engine:** ffmpeg demux and mux, HWL encoder for edges, SPS and PPS sniffing for SMART.
- **Surfaces:** panel editor, bar "trim", right-click "Trim", phone share sheet, CLI `panoply video trim in.mp4 --keep 0:12-1:40 [--keyframes]`, MCP `video_trim`.

### 4.2 Compress

- **Pattern / Job:** B. "Make this video small enough to send or upload."
- **Budgets:** standard. The VMAF probe runs on the 5-second sample only. Software two-pass runs under the batch CPU budget.
- **Inputs:** one or many videos.
- **On drop:** P. Computes whether the file is already efficient (bits per pixel against codec floor), HDR and bit depth, VFR, audio share of the bytes. Picks target: if the file is over a known app limit the nearest preset is pre-selected, otherwise "Smallest that still looks good".
- **Options:** Target: Smallest that looks good (default), Under a size (WhatsApp, Discord, Gmail, Slack, forms, custom; limits live in preset files with source link and date checked, none hardcoded here), or Quality level. Codec: H.264 for presets named for apps that need it, otherwise HEVC or AV1 when hardware encodes it and the receiver is the user's own device. Resolution: keep, lowered only by SIZE floor. HDR: keep or convert to SDR (SDR for app presets).
- **Preview / Output / Result:** estimated size band and time, one 3-second sample frame pair at 200% before and after, plus a "sample clip" button that encodes 5 seconds from the busiest scene. Output "name (compressed).mp4". Result: before and after size, percent saved, and VMAF against the source, all measured on the actual output.
- **Edge cases:** already efficient ("Already small, saved 3%", original kept, no output); VFR from phones kept as VFR unless the preset needs CFR; HDR Dolby Vision to H.264 target tone-mapped; B10 dither; ROT preserved as flag (burned in for WebM); AUD tracks dropped only if commentary and a size target forces it; SPATIAL warning; MOOV faststart; BIG streamed; target impossible ("30 s at 1 MB leaves 0.25 Mbps, would look like mush; lower to 480p or raise to 2 MB") rather than silently trashing quality.
- **Quality bar:** at equal output size VMAF is no lower than HandBrake's hardware "Fast 1080p30" and software equivalents on the corpus; wall time is within 5% of the ffmpeg CLI with the same encoder; end to end it beats uploading to FreeConvert on a typical home upload line because nothing is uploaded.
- **Engine:** SIZE with HWL encoders, VMAF probe on the 5-second sample, SVT-AV1 two-pass as software path.
- **Surfaces:** bar ("compress video under 25 mb"), panel, right-click, share sheet, CLI `panoply video compress --under 25mb`, MCP `video_compress`.

### 4.3 Convert

- **Pattern / Job:** A. "Turn this video into MP4 (or MOV, MKV, WebM, AVI, HEVC, AV1)."
- **Budgets:** standard.
- **Inputs:** one or many.
- **On drop:** P. Decides per stream whether the target container can hold it. Ranks targets by what this file type is usually converted to, then by user history.
- **Options:** Target (top 4 as buttons): MP4, MOV, MKV, WebM, AVI, plus "HEVC" and "AV1" as codec targets. Everything else folded away. Default behaviour: container change is COPY whenever codecs fit (MOV to MP4, MKV to MP4 with H.264 or HEVC and AAC); otherwise only the incompatible stream is transcoded (e.g. audio to AAC, video kept) before video is touched.
- **Preview / Output / Result:** the route per stream ("video: copy, audio: AAC, subtitles: mov_text"), time estimate; "name.mp4" next to original. Result: which streams were copied and which transcoded.
- **Edge cases:** WebM needs VP9 or AV1 and Opus or Vorbis, so a copy is not possible from H.264; AVI with B-frames and VFR gets CFR and a warning; MKV subtitles to MP4 keep text subs as mov_text and warn that PGS cannot be carried; HEVC in MP4 gets the hvc1 tag so Apple plays it; Dolby Vision kept on copy; lossless to lossy direction warnings handled by the universal converter; multiple audio kept (AUD); BIG; broken moov goes to Repair.
- **Quality bar:** copy routes are bit-exact on streams and finish at disk speed; transcode routes match the Compress bar at their quality setting. Round-trip tests for every container pair in the corpus.
- **Engine:** ffmpeg remux, HWL for transcodes. Shares the conversion graph with the universal converter (section 0).
- **Surfaces:** bar ("mov to mp4"), rename-to-convert, right-click "Convert to", CLI `panoply video convert in.mov out.mp4`, MCP `video_convert`.

### 4.4 Resize and reframe

- **Pattern / Job:** B. "Make this fit 9:16, 1:1, 16:9 or a size, with the subject in frame."
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback models (tiny face and subject models from Images, under 10 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks them.
- **Inputs:** one or many.
- **On drop:** P. Detects faces and the main subject on sampled frames (Images detectors), finds existing letterbox bars, reads rotation. Default target: opposite orientation of the source (landscape source suggests 9:16).
- **Options:** Target: 9:16, 1:1, 4:5, 16:9, or exact pixels (preset names per platform, sizes in preset files). Method: Follow subject (default when a subject is found), Center crop, Fit with blurred background, Fit with bars. Smoothing: how far the virtual camera may move per second (default gentle).
- **Preview / Output / Result:** the crop path drawn over a timeline of thumbnails, scrub to check, drag to override a keyframe. Output "name (9x16).mp4". Result: output size, any frames where the subject left the crop.
- **Edge cases:** ROT baked before crop; VFR kept; HDR kept or tone-mapped; two subjects (follow the larger, or split in time by who speaks when voice is detected, user can pick); upscale beyond source warned; B10; AUD copied; SPATIAL flattened with a warning.
- **Quality bar:** subject inside crop on at least the proportion of frames set by a labelled test set (gate decided from the first run); camera path has no jitter above a set acceleration; encode speed within 10% of Compress at same settings. Versus CapCut auto reframe and Clipchamp resize: blind comparison on the same clips.
- **Engine:** OS first, own model second. Faces: macOS and iOS: Apple Vision VNDetectFaceRectanglesRequest and VNDetectFaceLandmarksRequest. Android: ML Kit Face Detection. Windows: Windows.Media.FaceAnalysis (FaceDetector, FaceTracker, detection only, no landmarks). Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): YuNet (MIT) from Images. Subject: macOS and iOS: Apple Vision VNGenerateForegroundInstanceMaskRequest (iOS 17, macOS 14, also person instance mask). Android: ML Kit Subject Segmentation (beta, still images only). Windows: Windows AI Image Object Extractor (Copilot+ PCs only, still images, hint-based), else none. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): the Images subject model. ffmpeg crop and scale filters driven by a computed path, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64). Audio is COPY.
- **Surfaces:** panel, bar "make vertical", right-click, CLI `panoply video reframe in.mp4 --to 9:16 --follow`, MCP `video_reframe`.

### 4.5 Crop, rotate, flip

- **Pattern / Job:** C. "Fix this sideways video, or cut the edges off."
- **Budgets:** standard.
- **Inputs:** one or many.
- **On drop:** P. Reads the display matrix and the real pixel orientation; detects black bars with a short cropdetect pass; flags videos whose rotation flag disagrees with the content (face upright test) as "probably sideways".
- **Options:** Rotate 90, 180, 270, flip H or V, crop box with ratio lock, auto-remove bars. Default: if the video is flagged sideways the fix is pre-armed. Pure rotations use the flag only (COPY, instant); crop and flip re-encode.
- **Preview / Output / Result:** live frame with the crop box and a "No re-encode" or "Re-encode needed" label. Output "name (rotated).mp4". Result: size, and whether pixels were touched.
- **Edge cases:** ROT is the whole point: players that ignore the display matrix (some web editors) are told "set flag" versus "rotate pixels" in a toggle; crop dimensions rounded to even numbers and to the encoder's alignment with the nearest legal box; VFR, HDR, B10 kept; SPATIAL: flag rotation is copy-safe, crop flattens with warning; AUD untouched.
- **Quality bar:** a rotation-only job produces a file whose video stream bytes are identical to the source (hash), finishing at disk speed; crop re-encode meets the Compress VMAF bar at the same size.
- **Engine:** ffmpeg display-matrix editing, crop and transpose filters, HWL.
- **Surfaces:** panel, right-click "Rotate", CLI `panoply video rotate in.mp4 --90` and `panoply video crop in.mp4 --box 0,0,1080,1080`, MCP `video_rotate`, `video_crop`.

### 4.6 Merge

- **Pattern / Job:** A, with a reorder list. "Join these clips into one video."
- **Budgets:** standard.
- **Inputs:** many videos (and photos as still clips, optional).
- **On drop:** P for each. Orders by creation time or filename numbers. Compares codec, profile, resolution, frame rate, color, audio layout, rotation. Chooses COPY (concat demuxer) when every clip matches; otherwise normalises only the odd ones.
- **Options:** Order (by time or name). Mismatch handling: Match the first clip (default), Match the largest, Pad with bars. Transition: none (default) or crossfade (forces re-encode).
- **Preview / Output / Result:** list with a green "same format, joins instantly" or amber "2 clips will be converted to match" per clip. Output "name (merged).mp4" named after the first clip. Result: duration, which clips were re-encoded.
- **Edge cases:** mixed ROT (bake the odd ones); VFR clips with CFR clips (normalise to CFR only if mismatched); HDR mixed with SDR (tone-map the HDR to SDR, say so); different audio track counts or layouts (AUD, mono to stereo upmix by duplication); B10 mismatch; Dolby Vision mixed profiles forces re-encode; BIG output; SPATIAL mixed with flat clips flattens all with warning; moov repaired first if broken.
- **Quality bar:** matching clips join with zero re-encode and no A/V drift over a long join (drift under one frame at the end, measured); mismatched clips re-encode only the odd ones. Versus CapCut and Clipchamp: faster because no timeline render.
- **Engine:** ffmpeg concat demuxer, normalising transcodes via HWL.
- **Surfaces:** panel, right-click on multi-select "Merge videos", CLI `panoply video merge a.mp4 b.mp4 -o out.mp4`, MCP `video_merge`.

### 4.7 Split

- **Pattern / Job:** B. "Cut this video into parts by time, size, or scene."
- **Budgets:** standard. Silero VAD (about 2 MB, see 4.16) has no OS engine, so it downloads on first use on every platform and unloads after 60 s idle.
- **Inputs:** one video.
- **On drop:** P, keyframe map, chapters (used as default cut points if present), scene changes and long silences (cheap pass, cached).
- **Options:** By: Every N minutes, Into N parts, Under N MB per part (default when file is over a known limit), By scene, By chapter. Precision: Keyframes (COPY, default for size and time) or Exact (SMART).
- **Preview / Output / Result:** timeline with all cut points, draggable, with each part's duration and size. Output "name (part 1 of 5).mp4" into a new folder. Result: list of parts with sizes; any part over the limit is flagged.
- **Edge cases:** size splitting on keyframes means parts may overshoot slightly; we compute from bitrate and bump cuts earlier to stay under the limit; VFR kept; each part gets its own faststart moov; ROT, HDR, DV (keyframe cuts only for profile 5); AUD all tracks; scene detection can over-split screen recordings, so merge scenes shorter than a minimum; BIG streamed.
- **Quality bar:** copy-mode parts are bit-exact and concatenating them reproduces the source (test); no part exceeds a stated size limit; scene cuts agree with a labelled set. Versus splitting by hand in CapCut or Clipchamp: no re-encode, minutes become seconds.
- **Engine:** OS engine: none. Own model on every platform for silence (Silero VAD, MIT). ffmpeg segment muxer, PySceneDetect-class scene detection written in Rust (own implementation, no model).
- **Surfaces:** panel, right-click "Split", CLI `panoply video split in.mp4 --under 100mb`, MCP `video_split`.

### 4.8 Speed

- **Pattern / Job:** B. "Make it faster, slower, or smooth slow motion."
- **Budgets:** standard. The Smooth method carries the 4.14 budget. OS engines: no download, no resident memory for us. The fallback model (RIFE, see 4.14 for size) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** one or many.
- **On drop:** P. Detects frame rate, VFR, audio tracks, and whether the file is slow-motion capture (iPhone slo-mo carries a rate-change map in metadata; read and honour it).
- **Options:** Speed (0.25x to 8x, presets 0.5, 2, 4; default 2x for speeding up). Audio: pitch-preserving (default), pitch follows, or removed (removed by default above 4x). Slow-motion method: Duplicate frames (default), Blend, or Smooth (interpolation, see 4.14).
- **Preview / Output / Result:** new duration and size; a 3-second looped sample. Output "name (2x).mp4". Result: duration before and after, method used.
- **Edge cases:** VFR is converted to CFR first (VFR rule); a speed ramp is out of scope (cut to Trim, or Smooth for slo-mo); HDR kept; ROT preserved; AUD tracks all processed; stretching audio past 4x is dropped to avoid garble; B10; SPATIAL flattened with warning; BIG.
- **Quality bar:** audio stretch scores no worse than ffmpeg `atempo` and no pitch artifacts on speech in a blind listening check; video encode within 5% of ffmpeg CLI time; A/V stay in sync through the full length.
- **Engine:** OS first, own model second. Smooth (interpolation): macOS and iOS: VideoToolbox VTFrameProcessor (frame rate conversion and low-latency interpolation, macOS 15.4+, iOS 26). Android: none. Windows: none verified for interpolation. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): RIFE (MIT code; weights license verify). The quality bar test decides per platform. ffmpeg setpts and native atempo (LGPL; Rubber Band is GPL, so excluded), HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** panel, bar "speed up 2x", CLI `panoply video speed in.mp4 --factor 2`, MCP `video_speed`.

### 4.9 Reverse and loop

- **Pattern / Job:** A. "Play it backwards, or make it loop (boomerang)."
- **Budgets:** standard. Reverse works in chunks and spills each reversed chunk to a temp file, so memory is constant for any clip length. Free disk is checked against the estimate first.
- **Inputs:** one or many, usually short clips.
- **On drop:** P. Measures duration and temp disk cost (reverse needs frames in reverse GOP order). Default by length: boomerang (forward then back) for clips under about 10 s, plain reverse otherwise.
- **Options:** Mode: Reverse, Boomerang (forward plus reverse, default for short clips), Loop N times (stream-copy concat of the same file, no re-encode). Audio: reversed, muted, or kept forward (muted for boomerang by default).
- **Preview / Output / Result:** a looped sample plus new duration and size. Output "name (boomerang).mp4". Result: duration, size, whether any re-encode happened.
- **Edge cases:** reverse of long clips is done in chunks of a few GOPs, each reversed chunk is written to a temp file and the chunks are concatenated in reverse order, so memory stays constant and no duration limit applies (free disk space is checked against the estimate first); Loop N is COPY; VFR to CFR for reverse (VFR rule); HDR and B10 kept; ROT preserved; the seam of a boomerang is checked for a duplicate frame and removed; SPATIAL flattened for reverse and boomerang, kept for Loop; BIG.
- **Quality bar:** reverse is frame-exact (every source frame appears once in reverse order, hash test); memory stays constant for a clip of any length; Loop adds zero bytes of re-encoded video.
- **Engine:** ffmpeg reverse filter with chunked GOP processing, concat demuxer for Loop, HWL.
- **Surfaces:** panel, right-click "Boomerang", CLI `panoply video reverse in.mp4 [--boomerang | --loop 3]`, MCP `video_reverse`.

### 4.10 Audio (mute, replace, extract, volume)

- **Pattern / Job:** A. "Remove the sound, swap it, pull it out, or turn it up."
- **Budgets:** standard.
- **Inputs:** one or many videos, plus an audio file for Replace.
- **On drop:** P audio side: every track with language, channels, loudness (short EBU scan), peaks, silence. Default action chosen: if the file has no audible audio it says so; if audio is quiet (loudness far below a podcast target) Volume is suggested; if the user last extracted, Extract is armed.
- **Options:** Action: Extract (default), Mute, Replace, Volume. Extract: keep the original codec (COPY, instant, "name.m4a" for AAC, ".opus", ".mp3") or convert; default is COPY. Volume: dB or "match loudness" (EBU R128 to the chosen target). Replace: choose a track, trim or loop to fit, original audio kept as an optional second track. Mute and Replace leave video as COPY.
- **Preview / Output / Result:** track list with waveforms and the action per track; "Video untouched, no re-encode". Output depends on the action; Extract writes next to the source. Result: track kept or created, loudness before and after if changed.
- **Edge cases:** multiple tracks (AUD): choose one, several (one file each), or all in one multi-track container; mono versus stereo kept as is; clipping detected and warned before a Volume boost, gain limited to stay under -1 dBTP unless forced; Dolby (AC-3, E-AC-3) extract keeps codec by COPY; audio longer or shorter than video on Replace (loop, trim, or pad with silence); VFR irrelevant; BIG streamed.
- **Quality bar:** extraction by COPY is bit-exact on the elementary stream; volume changes land within 0.1 LU of target after measurement; faster than FreeConvert's MP4 to MP3 by skipping upload and, for COPY, by skipping decode.
- **Engine:** ffmpeg stream copy and mux, libebur128, native AAC or AudioToolbox, libmp3lame, libopus.
- **Surfaces:** bar ("mp4 to mp3", "mute video"), right-click, CLI `panoply video audio in.mp4 --extract` or `--mute` or `--replace song.mp3` or `--volume +6db`, MCP `video_audio`.

### 4.11 Subtitles

- **Pattern / Job:** C. "Add subtitles to this video, offline."
- **Budgets:** exception on peak memory, which applies to the fallback path only: Whisper size is chosen from device memory (base about 150 MB, small about 470 MB, medium about 1.5 GB, size shown before the first download). Loaded per job, unloaded after 60 s idle. Audio is streamed in 30 s windows, so memory does not grow with video length. OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. Silero VAD (about 2 MB) has no OS engine, so it downloads on first use and unloads after 60 s idle on every platform.
- **Inputs:** one video (or audio, same engine as 5.7); an existing SRT, VTT or ASS to attach.
- **On drop:** P, existing subtitle tracks (text or bitmap, languages), speech detection with Silero VAD and language detection by Whisper on the first speech segment. If subtitles already exist, the module opens them for editing and offers to skip generation.
- **Options:** Language (detected, default), Model size (chosen from device memory and speed, "Fast" or "Accurate"), Word highlight (off), Output: Soft track in MP4 or MKV (default, COPY of video and audio, instant), Burn in (re-encode, style options), Sidecar SRT or VTT. Max characters per line and line count follow the platform default (broadcast style, default 42 characters, 2 lines).
- **Preview / Output / Result:** video plus transcript editor with timestamps, speaker labels where available; burn-in style previewed live. Output "name.srt" next to the video, or "name (subtitled).mp4". Result: language, word count, duration of processing.
- **Edge cases:** music and silence cause hallucinated text, so VAD gates the model and low-confidence segments are marked rather than invented; multiple languages in one video (segment by detected language, warn); several speakers (labels when diarization is available, otherwise none); VFR timestamps from pts, not frame counts; AUD: the user picks the speech track, default is the first non-commentary track; MP4 holds text subs as mov_text only, so styled ASS goes to MKV or is burned in; burn-in respects ROT, HDR and keeps B10; BIG streamed.
- **Quality bar:** word error rate and timing offset measured on a labelled multilingual set against MacWhisper at the same model size (gate: not worse); wall time with hardware acceleration at least as fast as MacWhisper at equal model; timing within a stated tolerance of word boundaries.
- **Engine:** OS first, own model second. Speech to text: macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): whisper.cpp (MIT), Parakeet optional for European batch. Subtitle timing matters here. The OS engine must pass the timestamp-accuracy part of the quality bar test, not only word error rate. Voice activity for gating and cut points: OS engine: none. Own model on every platform (Silero VAD, MIT). ffmpeg subtitles and mux, libass for burn-in.
- **Surfaces:** panel, right-click "Add subtitles", CLI `panoply video subtitles in.mp4 [--lang en] [--burn]`, MCP `video_subtitles`.

### 4.12 Edit by transcript

- **Pattern / Job:** C. "Delete words from the text and the video follows, remove filler words."
- **Budgets:** as 4.11 (shared transcript service, OS engines first, fallback only on platforms where the quality bar test picks it). Splicing is standard.
- **Inputs:** one video (and audio, same module as 5.8).
- **On drop:** P, runs the Subtitles engine with word timestamps, marks filler words ("um", "uh", "you know", language aware), long pauses, and false starts. Arms "Remove fillers" with the list highlighted.
- **Options:** Filler removal: off, review each (default), automatic. Pause trimming: keep pauses under a set length (default 0.6 s, from the speaker's own pace). Crossfade at cuts in audio (default 10 ms to hide clicks). Video handling: Jump cut (default) or Smart zoom to hide cuts (re-encode).
- **Preview / Output / Result:** transcript with struck-through words, timeline of resulting cuts, resulting duration; scrub to hear each cut. Output "name (edited).mp4". Result: words removed, time saved, number of cuts, frames re-encoded.
- **Edge cases:** cuts inside a GOP use SMART for video edge frames; audio cuts snap to zero crossings; word timestamps are imprecise inside fast speech, so cuts move to the nearest VAD boundary and the user can nudge; VFR kept on pts; AUD: the speech track drives the cuts and every track is cut identically; HDR and DV follow SMART limits; many speakers need diarization or the cut applies to whoever speaks; BIG.
- **Quality bar:** zero audible clicks at cuts in a blind check; cuts land within a stated tolerance of true word boundaries on a labelled set; faster than Descript-style re-render because only edge GOPs re-encode.
- **Engine:** OS first, own model second. Speech to text is the shared transcript service (4.11) with its per-platform ladder. macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): whisper.cpp (MIT) with Silero VAD (MIT). This module cuts on word timestamps. The OS engine must pass the timestamp-accuracy part of the quality bar test, not only word error rate. Voice activity for the nearest cut boundary: OS engine: none. Own model on every platform (Silero VAD). ffmpeg SMART splicing, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** panel, CLI `panoply video edit-text in.mp4 --remove-fillers`, MCP `video_edit_by_transcript`.

### 4.13 Bitmap subtitles to SRT

- **Pattern / Job:** A. "Turn image-based subtitles (PGS, VobSub) into a normal SRT."
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback model (size verified at spike and shown before the first download; Tesseract language data was under 20 MB per language) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** MKV, MP4, M2TS, SUP, or VobSub .idx and .sub files.
- **On drop:** P lists bitmap subtitle tracks with language and forced flags. Defaults to the track matching the system language, else the first non-forced track.
- **Options:** Track (default as above), OCR language (from track tag, else detected), Include forced-only track (off), Output: SRT (default) or soft track muxed back (COPY of video and audio).
- **Preview / Output / Result:** first 10 recognised lines shown beside their source images, with low-confidence lines flagged. Output "name.en.srt". Result: line count, lines needing review.
- **Edge cases:** italics and sound effects in brackets kept as text; uppercase "I" versus lowercase "l" confusion fixed with a language dictionary; overlapping bitmaps; HDR and Blu-ray PGS palettes; empty or broken tracks reported plainly; BIG files read only the subtitle stream.
- **Quality bar:** character error rate on a labelled Blu-ray and DVD sample no worse than the best open tool used as baseline (Subtitle Edit with Tesseract); lines flagged for review are the ones that were wrong.
- **Engine:** OS first, own model second. OCR: macOS and iOS: Apple Vision VNRecognizeTextRequest, and RecognizeDocumentsRequest (iOS and macOS 26). Android: ML Kit Text Recognition v2. Windows: Windows.Media.Ocr (OcrEngine), and Windows AI Text Recognition on Copilot+ PCs. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): PaddleOCR-class model (Apache-2.0) through the shared OCR service, with Tesseract (Apache-2.0) kept as the CI baseline. ffmpeg to extract bitmaps, dictionary correction.
- **Surfaces:** panel, right-click "Subtitles to SRT", CLI `panoply video subs-ocr in.mkv --track 3`, MCP `video_subtitles_ocr`.

### 4.14 Smooth motion

- **Pattern / Job:** B. "Make it smoother or slow it down without choppiness."
- **Budgets:** exception on compute, which applies to the fallback path only: RIFE weights are small (tens of MB, verify) but frames run as float tensors on the GPU. Memory is constant (a window of a few frames, never the clip). Heavy files get a time estimate and a section option. OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** one or many videos.
- **On drop:** P. Reads frame rate, VFR, motion level and scene cuts. Default: double the frame rate (24 or 30 to 48 or 60) at original speed; if the user came from Speed, slow motion at the matching factor.
- **Options:** Target rate or slow-down factor. Scene-cut handling: never interpolate across a cut (on, not optional). Quality: Fast or Accurate (from device speed).
- **Preview / Output / Result:** side-by-side crop playing original and interpolated, with a warning region where artifacts are likely (fast motion, occlusion). Output "name (60fps).mp4". Result: output rate, frames synthesised.
- **Edge cases:** VFR to CFR first; scene cuts and fades produce ghosts, so cuts are detected and frames duplicated there; very fast motion and text overlays smear, flagged; HDR and B10 processed in 10-bit tensors; ROT baked; AUD COPY (and tempo kept for slow-motion only when asked); heavy files: compute is checked against a time budget and the user chooses a cut-down region; BIG.
- **Quality bar:** interpolated output beats ffmpeg `minterpolate` on a standard interpolation benchmark and a blind preference test; real-time or better throughput on a recent laptop GPU is the target, measured.
- **Engine:** OS first, own model second. Interpolation: macOS and iOS: VideoToolbox VTFrameProcessor (frame rate conversion and low-latency interpolation, macOS 15.4+, iOS 26). Android: none. Windows: none verified for interpolation. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): RIFE (MIT code; weights license verify) via Core ML, DirectML, Vulkan or NNAPI. The quality bar test decides per platform. ffmpeg decode, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** panel, CLI `panoply video smooth in.mp4 --fps 60`, MCP `video_smooth`.

### 4.15 Translate subtitles

- **Pattern / Job:** A. "Translate this subtitle track into another language, on my device."
- **Budgets:** shares the engine and the exception of Translate text. The exception (a per-pair model of about 100 MB class, peak under 600 MB including the runtime) applies to the fallback path only. OS engines: no download, no resident memory for us. The fallback model (size shown before the first download) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** SRT, VTT, ASS, or a video with a text subtitle track.
- **On drop:** reads source language (track tag, else detected), line count, timing and styles. Default target: the system language; if the same, the second most used language by history.
- **Options:** Target language (default above), Keep original as second track (on), Line fitting: reflow to the reading speed limit (on, default 20 characters per second adapted per language).
- **Preview / Output / Result:** first 10 lines before and after. Output "name.de.srt", or muxed track. Result: lines translated, lines reflowed, language pair model used.
- **Edge cases:** ASS styling and positioning tags preserved; line breaks inside sentences re-joined for translation then re-split; names and honorifics kept (glossary option); right-to-left languages; sentence split across cues translated as one unit; if no model for the pair, say so and offer pivot through English (with a quality warning); per-pair download size shown before fetch.
- **Quality bar:** chrF or BLEU on a held-out subtitle set per pair, reported with the model name; no timing changes; no model shipped with non-commercial weights.
- **Engine:** OS first, own model second. Translation: macOS and iOS: Apple Translation framework (TranslationSession). Android: ML Kit Translation. Windows: none found. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): Opus-MT class small permissive models (CC-BY-4.0 mostly, verify per pair) via CTranslate2 or ONNX; NLLB excluded for non-commercial weights. Source language detection: macOS and iOS: Apple NaturalLanguage NLLanguageRecognizer. Android: ML Kit Language Identification. Windows: none. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): a small language ID model.
- **Surfaces:** panel, right-click on subtitle files, CLI `panoply video subs-translate in.srt --to de`, MCP `video_subtitles_translate`.

### 4.16 Subtitle files (convert, shift, sync)

- **Pattern / Job:** A, with a sync dial. "Convert SRT to VTT, or fix subtitles that are out of sync."
- **Budgets:** standard. Silero VAD (about 2 MB) loads only for sync. It has no OS engine, so it downloads on first use and unloads after 60 s idle.
- **Inputs:** SRT, VTT, ASS, SSA, SUB, TTML, or a video with text tracks.
- **On drop:** parses format, encoding (UTF-8, UTF-16, Windows-1252 and others detected by byte statistics), overlap, cue count, and checks against the video if one is dropped alongside: aligns cues to detected speech with VAD to estimate offset and frame-rate drift (23.976 versus 25).
- **Options:** Target format (VTT for web, SRT default). Shift: milliseconds, or "sync to video" (default when a video is present) which sets offset and linear drift from two anchor points. Encoding to UTF-8 (on).
- **Preview / Output / Result:** first and last cues before and after, plus a 3-point waveform view showing cue starts against speech. Output "name.vtt". Result: offset and drift applied, encoding fixed.
- **Edge cases:** ASS to SRT loses styling (warned, formatting tags kept as simple tags); SRT line limits; overlapping cues merged or flagged; frame-rate-based formats (SUB) need the fps; HTML tags and entities; broken numbering renumbered; BOM and CRLF normalised.
- **Quality bar:** round trip SRT to VTT to SRT is lossless on a corpus; sync offset estimate within 100 ms on labelled files; versus Subtitle Edit: same result with one click.
- **Engine:** For sync: OS engine: none. Own model on every platform (Silero VAD, MIT). Own Rust parsers, ffmpeg for track extract.
- **Surfaces:** bar ("srt to vtt"), right-click, CLI `panoply video subs in.srt --to vtt --shift +1.5s`, MCP `video_subtitles_convert`.

### 4.17 Streams (extract or swap tracks, unpack)

- **Pattern / Job:** C (a list of tracks). "Take out or swap an audio, subtitle or video track, or unpack this MKV."
- **Budgets:** standard.
- **Inputs:** one or many MP4, MKV, MOV, M2TS, WebM.
- **On drop:** the list of all streams with codec, language, title, default and forced flags, size, plus attachments (fonts, cover) and chapters. Defaults: remove nothing; for a file with more than one language the track for the system language is marked default.
- **Options:** Per stream: keep, drop, extract to file, replace from file, set default or forced, rename language. Output container: same (default) or MKV when a stream will not fit in MP4. Everything is COPY.
- **Preview / Output / Result:** the stream table with the resulting file's size; "No re-encode". Output "name (streams).mkv" or extracted files in a folder named after the source. Result: streams kept, dropped, added.
- **Edge cases:** MP4 cannot hold PGS, ASS, or some audio (reported before running with the container to use instead); MKV attachments (fonts) kept; AUD default flags set so players pick the right one; replaced audio of different length (offset and padding options); VFR irrelevant (copy); chapters and global tags preserved; broken moov goes to Repair; BIG; SPATIAL preserved.
- **Quality bar:** every kept stream is bit-identical to the source (hash); a 20 GB MKV unpacks at disk speed. Versus MKVToolNix: the same operations with one drop and no menus.
- **Engine:** ffmpeg stream copy, own Matroska chapter and attachment handling.
- **Surfaces:** panel, right-click "Streams", CLI `panoply video streams in.mkv --extract 2 --drop 5`, MCP `video_streams`.

### 4.18 Repair video

- **Pattern / Job:** A. "This video will not play, fix it."
- **Budgets:** standard. The media scan streams, so a 20 GB file stays flat.
- **Inputs:** one or many broken or unplayable files (MP4, MOV, MKV, AVI, TS).
- **On drop:** diagnoses instead of assuming: moov missing or at a truncated end, mdat present, sync loss, corrupt GOPs, audio-only readable, wrong duration, bad timestamps, partially downloaded file. Shows what it found in plain words, and which repair it will run.
- **Options:** Method (chosen by diagnosis): Rebuild index from the media data, Reference file (a working clip from the same camera supplies codec parameters, default offered when the diagnosis is "moov missing"), Drop bad frames and re-mux, Re-time. Output container: same.
- **Preview / Output / Result:** before the run, a recoverable estimate ("about 94% of the data is intact") from scanning the media data; a short playable preview of the first seconds. Output "name (repaired).mp4". Result: recovered duration and frames, lost spans with timestamps.
- **Edge cases:** the camera crashed mid-recording so moov never got written (MOOV): needs a reference file or parameters guessed from the first IDR frames; fragmented MP4 recovered fragment by fragment; files over 4 GB (BIG) scanned streaming; VFR timestamps rebuilt from sample deltas; HDR and DV config read from the bitstream; audio and video offset restored from the first frames; encrypted or DRM data: say so and stop; GoPro and dashcam loops with appended garbage.
- **Quality bar:** recovery rate on a corpus of deliberately truncated files (truncated at 10, 50, 90 percent, moov stripped, bytes zeroed in the middle), against untrunc and ffmpeg's own recovery flags as baselines; no output is declared repaired unless it plays start to end in a decode test.
- **Engine:** own Rust MP4 and MKV parser and index rebuilder, ffmpeg for decode check and mux.
- **Surfaces:** panel, right-click "Repair video", also invoked inline by other modules' MOOV error; CLI `panoply video repair broken.mp4 [--reference ok.mp4]`, MCP `video_repair`.

### 4.19 Restore old video

- **Pattern / Job:** B. "Clean up old or low quality video: noise, interlacing, low resolution."
- **Budgets:** exception on time and model size. The model size applies to the fallback path only: a Real-ESRGAN-class upscaler (a few MB to about 65 MB) runs per frame, so a long clip takes minutes to hours. The ETA and a restore-a-section option are shown up front. Memory is constant (a window of frames). OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** one or many (DVD rips, camcorder tapes, phone clips from long ago, 480i and 576i files).
- **On drop:** P, interlacing detection (field order by combing metric), noise level, resolution, aspect ratio flags, black borders, color cast. Default: deinterlace if interlaced, mild denoise if noise is above a threshold, upscale only if the picture is under 720p.
- **Options:** Deinterlace (auto), Denoise (off, light, strong), Upscale (off, 2x, 4x; default 2x for under 720p), Sharpen (off), Crop borders (on). Order is fixed: deinterlace, denoise, upscale.
- **Preview / Output / Result:** a split frame (before and after) at 200% plus a 5-second sample render; ETA, which can be long, shown in plain words. Output "name (restored).mp4". Result: resolution, time taken, steps applied.
- **Edge cases:** telecined film (3:2 pulldown) detected and inverse-telecined, not deinterlaced; anamorphic DVD aspect ratio kept; VFR from old captures to CFR; B10 and HDR irrelevant for most (but SDR BT.601 to BT.709 conversion handled); upscaling on hardware time budget: the user is shown a time estimate and can restore only a section; AUD COPY.
- **Quality bar:** deinterlace via bwdif is the baseline; upscale scores higher VMAF-style full reference on a downscaled-then-restored set and wins a blind preference over HandBrake's deinterlace plus Lanczos; no added flicker between frames.
- **Engine:** OS first, own model second. Upscale: macOS and iOS: VideoToolbox VTFrameProcessor super resolution (macOS 15.4+, iOS 26). Android: none. Windows: Windows AI Video Super Resolution (Copilot+ NPU, also CPU). Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): Real-ESRGAN-class upscaler from the Images module (BSD-3, verify). Denoise: ffmpeg nlmeans or hqdn3d stay the default and use no model. On macOS and iOS the VTFrameProcessor temporal noise filter is an optional OS engine if it passes the quality bar test. The quality bar test decides per platform. ffmpeg bwdif, pullup, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** panel, CLI `panoply video restore in.mp4 --upscale 2`, MCP `video_restore`.

### 4.20 Tidy screen recording

- **Pattern / Job:** B. "Cut the dead time and pauses out of my screen recording."
- **Budgets:** standard. Frame differencing runs on decoded low-resolution frames only. Silero VAD (about 2 MB) has no OS engine, so it downloads on first use and unloads after 60 s idle.
- **Inputs:** one or many screen recordings (MP4, MOV, WebM, MKV).
- **On drop:** P, then detects screen-recording type (constant resolution, few scene changes, cursor present). Finds idle spans by frame differencing, audio silence (Silero VAD), and cursor idle; builds a map of "action" and "idle" spans, and a duration estimate.
- **Options:** Idle handling: Cut, Speed up (default 8x for idle spans over 2 seconds), Keep. Threshold: idle length (default 1.5 s), sensitivity. Keep narration silence untouched while the screen is moving (on). Pad each cut by 0.2 s (on).
- **Preview / Output / Result:** timeline coloured by action versus idle, each span clickable; "9:40 becomes 3:12". Output "name (tidy).mp4". Result: time saved, spans cut and sped.
- **Edge cases:** loading spinners count as motion (a small region mask option); videos with typing and pauses (words are speech, silence is the cut); VFR from screen capture (very common) handled by pts; HDR screen capture from macOS tone-mapped on request; high-DPI sources huge (BIG) streamed; audio dropped during sped spans (muted or sped); cuts use SMART; rotation irrelevant.
- **Quality bar:** idle spans detected agree with human-labelled idle spans on a set of screen recordings (precision and recall gates set after the first labelled run); output has no mid-word cuts; export time near copy time when cuts land on keyframes.
- **Engine:** For narration silence: OS engine: none. Own model on every platform (Silero VAD, MIT). Rust frame diffing on decoded low-res frames (no model), ffmpeg SMART splice.
- **Surfaces:** panel, right-click, CLI `panoply video tidy in.mov --idle 1.5s --speed 8`, MCP `video_tidy`.

### 4.21 Text and watermark

- **Pattern / Job:** C. "Put a title, text, or my logo on this video."
- **Budgets:** standard.
- **Inputs:** one video; a logo (PNG, SVG, WebP) or text.
- **On drop:** P and a safe-area check (platform safe zones for 9:16 etc.). Default position: bottom right for a logo, lower third for text. If a logo is dropped with the video, it is armed with opacity and size defaults chosen to be a stated fraction of frame height.
- **Options:** Text (font from system, size, color, outline), Logo (position, opacity, margin, size), Timing (whole video, start to end), Fade in and out. Fonts must be installed or bundled (bundled fonts only for export on all devices).
- **Preview / Output / Result:** live frame with the overlay on any frame, safe-area guides. Output "name (titled).mp4". Result: overlay region, re-encode info.
- **Edge cases:** ROT baked before overlay; HDR video with an SDR logo (logo mapped to the correct nit level so it is not blinding or grey); VFR kept; unicode and right-to-left text shaped correctly; emoji; transparent PNG gamma; overlay always requires re-encode of the region, so the whole file is re-encoded (stated up front); B10 kept; AUD COPY; BIG streamed.
- **Quality bar:** overlay pixels match a reference render (SSIM gate) and the file passes the Compress VMAF bar at equal size; text rendering equals libass quality with correct kerning. Versus Clipchamp and CapCut: no watermark on output, no upload.
- **Engine:** ffmpeg overlay and drawtext, libass for styled text, HWL.
- **Surfaces:** panel, CLI `panoply video overlay in.mp4 --text "Title" --pos lower-third` or `--logo logo.png`, MCP `video_overlay`.

### 4.22 Remove watermark

- **Pattern / Job:** C. "Remove this logo, timestamp, or burned-in text."
- **Budgets:** standard for static delogo (no model). Tracked inpainting adds a LaMa-class model (about 200 MB) and optical flow in a temporal window, with no OS engine, so it downloads on first use, loads only on that path and unloads after 60 s idle. Memory is constant, never the whole video.
- **Inputs:** one video.
- **On drop:** P, then a first-pass detector for static regions across frames (logos, timestamps) and text boxes. Proposes the region(s) and whether the object is static (one mask) or moves (needs tracking).
- **Options:** Region (proposed, editable), Method: Static (ffmpeg delogo or removelogo, ⚡, default for static logos), Tracked inpainting (🧠, default for moving or complex backgrounds), Crop it out (if the mark sits at an edge, loses the least frame), Blur it (fast fallback).
- **Preview / Output / Result:** mask drawn on three frames at 200% with before and after, then a 5-second sample render. Output "name (clean).mp4". Result: method, frames processed, time.
- **Edge cases:** semi-transparent animated logos; timestamps that change per frame (tracked per frame); text over complex backgrounds produce smeared fills, flagged by a confidence measure; rights: shown once as a plain line that the user is responsible for having the right to edit; ROT baked; VFR; HDR; AUD COPY; BIG streamed with temporal window.
- **Quality bar:** on a labelled set of watermarked clips, residual mask contrast and temporal flicker (frame-to-frame difference inside the mask) beat the static delogo baseline for moving backgrounds; no flicker at scene cuts. Versus hosted removers: nothing uploaded.
- **Engine:** Inpainting: OS engine: none verified for video. Own model on every platform: the Object eraser inpainting model from Images (LaMa-class, Apache-2.0, weights verify) plus optical-flow propagation for temporal consistency. ProPainter is not planned until its license is read. ffmpeg delogo for static (no model).
- **Surfaces:** panel, CLI `panoply video unmark in.mp4 --region 1620,40,280,90 [--track]`, MCP `video_remove_watermark`.

### 4.23 Blur faces or regions

- **Pattern / Job:** C. "Blur faces, plates, or any box in this video."
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback model (YuNet, under 1 MB) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. The tracker is own code. Memory is constant.
- **Inputs:** one video; optional list of faces to keep sharp.
- **On drop:** P, runs the Images face detector over sampled frames, clusters faces into people, and shows a gallery. Default: blur every face that is not marked "keep"; if the file is portrait phone video with one face, the face is pre-marked keep.
- **Options:** Targets: faces (default), license plates, manual regions. Style: Gaussian blur (default), pixelate, solid box. Strength is chosen so the face is not recoverable (pixelate size and blur radius relative to face size, set by an irreversibility test). Padding around the face. Tracking: on.
- **Preview / Output / Result:** timeline with track bars per person, scrub to check; any frame where a tracked face is lost is marked red. Output "name (blurred).mp4". Result: faces blurred, tracks, frames where a face was uncertain.
- **Edge cases:** side profiles, masks, heavy motion, tiny faces in crowds; face re-appearing after occlusion keeps its person id; a gap-fill pass blurs the region across missed frames rather than leaving a flash; blur must be applied to the burned pixels, never as a mask the viewer can remove; metadata (thumbnail, chapters) can contain faces: stripped from the output with a note; ROT baked; VFR; HDR; AUD COPY, voices are not changed (the result card says so); BIG streamed.
- **Quality bar:** recall on a labelled set (a miss in any frame counts); the output is checked by running the same detector on it, and no confident face may remain (self-test is run on every output and its result shown).
- **Engine:** OS first, own model second. Faces: macOS and iOS: Apple Vision VNDetectFaceRectanglesRequest and VNDetectFaceLandmarksRequest. Android: ML Kit Face Detection. Windows: Windows.Media.FaceAnalysis (FaceDetector, FaceTracker, detection only, no landmarks). Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): YuNet (MIT) from Images. License plates: OS engine: none. Own model on every platform (own detector, chosen at spike). Tracker: ByteTrack-class, own implementation, no model. ffmpeg filter graph, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** panel, CLI `panoply video blur in.mp4 --faces [--keep face3]`, MCP `video_blur`.

### 4.24 Stabilize

- **Pattern / Job:** B. "Smooth out this shaky footage."
- **Budgets:** standard. The analysis pass keeps a motion path (numbers), not frames.
- **Inputs:** one or many.
- **On drop:** P. Runs a motion analysis pass (global motion from feature tracks), measures shake level, rolling shutter skew and gyro track if present (iPhone and GoPro carry gyro data in metadata). Default strength proportional to measured shake. Reads ROT.
- **Options:** Strength (auto), Crop versus Zoom (auto crop to hide borders, default), Keep horizon (off), Rolling shutter correction (on when skew is detected). Tripod mode (lock the frame) as a preset.
- **Preview / Output / Result:** split view with original and stabilised trajectories and the crop that will be lost ("3% of frame lost"). Output "name (stable).mp4". Result: shake reduction measured as path jitter, crop percent.
- **Edge cases:** panning shots mistaken for shake (low-pass tuned so intended pans survive); moving subjects fooling the tracker (outlier rejection); scene cuts reset the path; VFR to CFR; HDR and B10 kept; ROT baked; AUD COPY; SPATIAL flattened; BIG chunked with overlap so path is continuous.
- **Quality bar:** trajectory jitter reduction at equal crop beats ffmpeg vidstab on a labelled shaky clip set, and a blind preference check; no wobble or jelly artifacts counted by an automatic residual check.
- **Engine:** own motion estimation in Rust (ffmpeg vidstab is GPL, not linkable in the LGPL build), ffmpeg warp via libavfilter remap, gyro data where present, HWL.
- **Surfaces:** panel, CLI `panoply video stabilize in.mp4`, MCP `video_stabilize`.

### 4.25 Frames (stills, contact sheet, best thumbnail)

- **Pattern / Job:** B. "Grab frames, make a contact sheet, or pick the best thumbnail."
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback models (tiny face and aesthetics models from Images) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks them.
- **Inputs:** one or many videos.
- **On drop:** P and a sampled scan: sharpness, faces with eyes open, exposure, composition, scene changes. Default: "Best thumbnail" with a top-3 shortlist.
- **Options:** Mode: Grab this frame (editor scrub), Every N seconds or N frames, At scene changes, Contact sheet (grid, count, with timestamps), Best thumbnail (default). Format: JPG or PNG (JPG default, PNG for lossless), HDR stills tone-mapped to SDR sRGB or kept as HEIC 10-bit.
- **Preview / Output / Result:** thumbnail strip with the ranked candidates; contact sheet layout. Output "name frame 00:01:23.jpg", sheet "name (contact).jpg". Result: count and sizes.
- **Edge cases:** exact-frame seeking on VFR and B-frames (decode from the previous keyframe, never approximate); ROT applied to the stills; HDR still flattening handled; closed-eye and motion-blurred frames penalised; Live Photo and spatial video yield 2D left-eye frames; BIG, seek only.
- **Quality bar:** the exact requested frame is returned (hash match against a full decode); the shortlist contains the human-chosen thumbnail on a labelled set at a stated rate; speed: single-frame grab under a second on a 4K file from a hot cache, measured.
- **Engine:** OS first, own model second. Faces and eyes open: macOS and iOS: Apple Vision VNDetectFaceRectanglesRequest and VNDetectFaceLandmarksRequest. Android: ML Kit Face Detection. Windows: Windows.Media.FaceAnalysis (FaceDetector, FaceTracker, detection only, no landmarks). Linux: fallback. Where the job needs landmarks, Windows has none in FaceDetector, so Windows uses the fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): YuNet (MIT) from Images, with MediaPipe Face Landmarker (Apache-2.0) where landmarks are needed. Aesthetics ranking: OS engine: none. Own model on every platform (Images aesthetics model). ffmpeg seeking and decode.
- **Surfaces:** panel, right-click "Grab frame", CLI `panoply video frames in.mp4 --every 5s` or `--best 3` or `--sheet 4x4`, MCP `video_frames`.

### 4.26 Video to GIF (and back)

- **Pattern / Job:** B. "Make a GIF from this video, or a video from this GIF."
- **Budgets:** standard. Frames are streamed in two passes (palette, then encode), never all held.
- **Inputs:** a video (any) or a GIF or animated WebP, APNG.
- **On drop:** P. For video: duration, motion, color count in the busiest scene. For GIF: frame count, delays, palette, loop count. Default: GIF at a width and frame rate chosen to land under a typical chat limit when the clip is short; if the clip is over about 10 seconds a "trim first" nudge is shown.
- **Options:** Range (editor), Width (default 480), Frame rate (default 15, lower for long clips), Quality (palette per frame and dithering Floyd-Steinberg default), Format: GIF, animated WebP, APNG (WebP and APNG are far smaller and the default offer for sites that accept them), Size target via the same loop as SIZE (reduce width and rate to fit). GIF to video: MP4 (H.264) or WebM with looping.
- **Preview / Output / Result:** frame count, size estimate, a looping sample. Output "name.gif". Result: size, frames, palette mode.
- **Edge cases:** VFR to CFR; GIF frame delays below browser minimum clamped; HDR tone-mapped; ROT baked; transparency kept in GIF to video via a background color choice; very large GIFs (over several hundred MB) flagged; audio dropped, said once; dithering banding on gradients.
- **Quality bar:** at equal size the GIF scores a higher color-difference measure than ffmpeg's default palette filter and Gifski (baseline); GIF to MP4 reduces size on the corpus and loops with no visible jump.
- **Engine:** ffmpeg palettegen and paletteuse, gifski-style encoder in Rust if its license allows (verify, AGPL risk), libwebp, APNG muxer.
- **Surfaces:** bar ("video to gif"), right-click, CLI `panoply video gif in.mp4 --width 480 --fps 15` and `panoply video ungif in.gif`, MCP `video_gif`.

### 4.27 Frame rate

- **Pattern / Job:** A. "Change this to 24, 30, or 60 fps."
- **Budgets:** standard. OS engines: no download, no resident memory for us. The fallback model (RIFE, see 4.14 for size) downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it.
- **Inputs:** one or many.
- **On drop:** P, nominal and average rate, VFR flag. Default target: for VFR phone video, the nearest standard rate to the average (it just becomes CFR); for CFR video, none (target chosen by the user).
- **Options:** Target rate (23.976, 24, 25, 29.97, 30, 50, 59.94, 60 and custom). Method: Drop or duplicate frames (default, no re-encode artifacts), Blend (smoother when reducing), Interpolate (links to Smooth motion 4.14). Reducing rate by an integer factor (60 to 30) drops cleanly.
- **Preview / Output / Result:** "VFR, average 29.7, becomes 30 constant"; frames dropped or duplicated count. Output "name (30fps).mp4". Result: rate before and after, method.
- **Edge cases:** 29.97 versus 30 and 23.976 versus 24 mixups (the 0.1% difference breaks A/V sync in long files; audio is kept in sync by pts, not retimed); 24 to 60 judder from duplication (offered Interpolate); HDR, B10, ROT kept; AUD COPY; a rate change on HDR Dolby Vision requires rewriting RPU timing, so DV is tone-mapped or stripped with a warning (verify); BIG.
- **Quality bar:** A/V sync drift under one frame at the end of a long file, measured; matches ffmpeg `fps` filter output frame-for-frame in Drop mode.
- **Engine:** OS first, own model second. Interpolation: macOS and iOS: VideoToolbox VTFrameProcessor (frame rate conversion and low-latency interpolation, macOS 15.4+, iOS 26). Android: none. Windows: none verified for interpolation. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): RIFE (MIT code; weights license verify). The quality bar test decides per platform. ffmpeg fps filter, HWL encode (VideoToolbox, MediaCodec, Media Foundation, VA-API; no x265, decision 64).
- **Surfaces:** bar ("30 fps"), right-click, CLI `panoply video fps in.mp4 --to 30`, MCP `video_fps`.

### 4.28 HDR to SDR

- **Pattern / Job:** A. "Fix this washed-out iPhone video on other screens."
- **Budgets:** standard.
- **Inputs:** one or many HDR videos (HDR10, HLG, Dolby Vision).
- **On drop:** P identifies the exact HDR flavour and the DV profile, peak luminance from mastering metadata or measured, and confirms "this looks washed out on non-HDR screens because it is HDR". If the video is SDR, say so and do nothing.
- **Options:** Tone-map curve (BT.2390, default; Hable and Mobius alternatives), Peak brightness target (auto), Saturation protection (on), Output codec (H.264 for broad play, HEVC 10-bit allowed). Method fixed: tone-map in linear light with a gamut map from BT.2020 to BT.709.
- **Preview / Output / Result:** side-by-side original (HDR, as an SDR screen will see it) and converted at 200%. Output "name (SDR).mp4". Result: curve, output codec, size.
- **Edge cases:** Dolby Vision profile 5 needs the RPU applied (libplacebo, Vulkan) or the image is purple and green; profile 8.4 treated as HLG; HLG shows pleasing values after OOTF handling; scene-adaptive peak detection avoids pumping; B10 dither on the way to 8-bit; ROT and VFR kept; spatial flattened; AUD COPY; BIG; devices with no Vulkan fall back to zscale and the preview says "approximate" for DV 5.
- **Quality bar:** on the corpus clips, delta-E against a reference grade (DaVinci or Apple's own SDR rendering, whichever is available) within a stated gate; no clipping of highlights beyond a stated percent; faster than HandBrake's CPU tone-map.
- **Engine:** libplacebo (LGPL-2.1+, verify) or zscale tonemap, ffmpeg, HWL.
- **Surfaces:** bar ("hdr to sdr"), right-click, CLI `panoply video sdr in.mov`, MCP `video_hdr_to_sdr`.

### 4.29 Picture in picture

- **Pattern / Job:** C. "Put one video on top of another."
- **Budgets:** standard.
- **Inputs:** two videos (background and overlay), optionally an image or webcam clip.
- **On drop:** P for both, which is the main (larger or longer by default), frame rates, audio tracks, and loudness of each. Default overlay: bottom right, one quarter width, rounded corners.
- **Options:** Position (corners and free drag), Size, Border and shadow, Shape (rect, rounded, circle), Audio: main only (default), mix (overlay at -6 dB), overlay only. Timing: overlay start offset, duration, loop or hold last frame.
- **Preview / Output / Result:** live compose with scrub. Output "name (pip).mp4". Result: duration, audio mix applied.
- **Edge cases:** different frame rates and VFR (both normalised to the main's rate); different durations; ROT baked on both; HDR main with SDR overlay (map the SDR up correctly); audio sync offsets; transparent overlays (WebM VP9 alpha, ProRes 4444); overlay re-encode is whole file; SPATIAL flattened; BIG.
- **Quality bar:** overlay edges are antialiased (no jaggies on rounded corners) and A/V sync within a frame after composing; render speed within 10% of the ffmpeg CLI doing the same overlay. Versus CapCut: same compose, no watermark, no upload.
- **Engine:** ffmpeg overlay, scale, alphamerge; HWL; zero ffmpeg GPL filters.
- **Surfaces:** panel, CLI `panoply video pip main.mp4 inset.mp4 --pos br --size 25%`, MCP `video_pip`.

### 4.30 Audio plus image to video

- **Pattern / Job:** B. "Turn a podcast or song into an MP4 with cover art and a waveform."
- **Budgets:** standard.
- **Inputs:** one audio file (or several) plus an optional image.
- **On drop:** reads duration, embedded cover art, title, artist, loudness, channels. Default: cover art as the background (if absent, the user is asked for an image or gets a plain background from the palette of the art), a waveform bar chart, 1080x1080 for social, 1920x1080 for video platforms (default chosen by the last used).
- **Options:** Canvas (16:9, 1:1, 9:16, 4:5), Visualisation (waveform line, bars, none), Title and artist text (from tags, on), Background (cover, blurred cover, solid), Frame rate (low, default 30; static backgrounds use a low rate to keep file small).
- **Preview / Output / Result:** frame preview and a sample; size estimate (small, since mostly static). Output "name.mp4". Result: duration, size, audio kept as COPY if AAC.
- **Edge cases:** very long audio (over several hours, BIG) streamed; static frames mean keyframe interval is long, so some platforms re-encode anyway; audio tracks with chapters (markers shown); mono audio; cover art of low resolution (upscale warning); non-square art cropped with a focal point; hardware encoders at tiny bitrates; sound plus waveform stays in sync via pts.
- **Quality bar:** audio is bit-exact when COPY; the waveform matches the audio peaks at the same timestamps (sync gate within one frame); output size for a one-hour podcast with a still image is dominated by the audio track. Versus free "audio to video" web tools: no upload of the audio.
- **Engine:** ffmpeg showwaves or own rendering in Rust for the waveform, libass for text, HWL.
- **Surfaces:** panel, right-click on audio "Make video", CLI `panoply video from-audio ep12.mp3 --cover cover.jpg --canvas 9:16`, MCP `video_from_audio`.

### 4.31 Video to Live Photo (and back)

- **Pattern / Job:** A (Apple devices only). "Turn a short video into a Live Photo, or save the video out of a Live Photo."
- **Budgets:** standard.
- **Inputs:** a short video (up to about 3 seconds ideal) or a Live Photo (HEIC or JPG plus MOV pair, or a Live Photo from the Photos library on Apple devices).
- **On drop:** for video: duration, a key photo frame chosen by sharpness. For a Live Photo: the pair, the content identifier, the key photo time. Live Photo to video is the default if a pair is detected; otherwise video to Live Photo.
- **Options:** Key frame (auto), Duration crop (default last 3 seconds), Audio (kept). Output Live Photo to the photo library (Apple platforms only) or as the file pair with a shared asset identifier.
- **Preview / Output / Result:** key frame picker; the pair. Output "name.MOV" (Live Photo to video) or "name.HEIC + name.MOV". Result: pair created and identifier.
- **Edge cases:** Live Photo creation requires writing the asset identifier into both files (QuickTime metadata and Apple MakerNotes); on Windows, Linux and Android Live Photos can be exported but not imported into Photos, so the pair is offered as "Motion Photo" for Android (verify format), with a clear message; length and size limits; HDR kept as is; ROT; spatial: flattened; rate conversion.
- **Quality bar:** a Live Photo made by Panoply opens as a Live Photo in Photos on a real device (manual test per release); a video extracted from a Live Photo is a bit-exact copy of the MOV.
- **Engine:** ffmpeg, own metadata writer, PhotoKit on Apple platforms.
- **Surfaces:** panel, phone share sheet, CLI `panoply video livephoto in.mp4` and `panoply video from-livephoto in.heic`, MCP `video_live_photo`.

### 4.32 Teleprompter

- **Pattern / Job:** C (a canvas that follows your voice). "Show my script on screen and scroll as I speak."
- **Budgets:** exception while active only: the streaming recognizer keeps the mic and some CPU busy while the window is open. Idle cost is zero. The review recommends deferring this module. OS engines: no download, no resident memory for us. The fallback model (Moonshine English, tens of MB) downloads on first use, loads when the window opens and unloads 60 s after it closes, only on platforms where the quality bar test picks it.
- **Inputs:** a script (pasted text, TXT, DOCX, Markdown), a microphone, optionally the camera for a recording.
- **On drop:** script length and reading time, estimated at the user's measured speaking pace (calibrated on the first few lines). Default: voice-following scroll with a mirrored option off.
- **Options:** Mode: Follow my voice (default), Fixed speed, Manual. Text size and width, mirror (for hardware teleprompter glass), margin and reading line height, Highlight the current word, Countdown. Record: off by default; on, saves video through the Recorder pipeline.
- **Preview / Output / Result:** the script with a reading line; nothing is rendered. Output: a recording only when asked. Result: elapsed time, pace, words skipped.
- **Edge cases:** the speaker ad-libs, repeats, or skips a line (forced alignment with a fuzzy match, scroll does not run ahead of the voice); background noise; loud room; accents; numbers and acronyms spoken versus written; non-English scripts; low-latency needs a streaming recogniser; phone: portrait and landscape; full-screen overlays on desktop sit above the camera app window.
- **Quality bar:** the scroll position is within a stated number of words of the actual spoken word, median latency measured; no model download beyond the STT model. Versus paid teleprompter apps: nothing uploaded and works offline.
- **Engine:** OS first, own model second. Streaming speech to text: macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): Moonshine English (MIT) streaming; Whisper streaming for other languages. This module needs low latency, so the OS engine must also pass the latency part of the quality bar test. Alignment against the script, Silero VAD (OS engine: none. Own model on every platform).
- **Surfaces:** panel and phone, CLI `panoply video teleprompter script.md` (opens the window), MCP none (interactive).

### 4.33 Info

- **Pattern / Job:** I. "Tell me everything about this video and what is wrong with it."
- **Budgets:** standard.
- **Inputs:** one or many videos.
- **On drop:** the report is the drop result: P in full, plus problems found: VFR, HDR flavour, DV profile, rotation flag set, moov at the end (slow to start on the web), truncated file, multiple audio tracks without a default, loudness far from broadcast targets, audio clipped, chapters, hidden data (GPS, device name, subtitles with metadata), interlaced, non-square pixels, odd dimensions, and 10-bit that many players cannot decode.
- **Options:** none beyond "Copy as JSON" and a verbosity switch (summary, full).
- **Preview / Output / Result:** the report; each problem has a one-click fix button that opens the right module with options filled: faststart (rewrite moov, COPY), set the default audio, fix sideways, HDR to SDR, Repair, Compress, Metadata clean (the Metadata module, section Privacy). Output: none by default; JSON on request.
- **Edge cases:** the same facts are shown in plain words next to the technical value ("HLG, looks washed out on non-HDR screens"); files too broken to probe (go to Repair); BIG files read only headers and the tail; MV-HEVC views listed; multiple videos shown as a table with sortable columns (codec, bitrate, resolution).
- **Quality bar:** every field agrees with ffprobe on the corpus (automated diff); report appears in under one second on a hot cache for a file of any size, measured; fix buttons land on pre-filled modules without extra steps.
- **Engine:** ffmpeg libavformat probe, own MP4 box walker for moov position and atoms ffmpeg hides.
- **Surfaces:** bar ("video info"), panel, right-click, CLI `panoply video info in.mp4 [--json]`, MCP `video_info`.

---

## 5. Audio

### 5.1 Convert

- **Pattern / Job:** A. "Turn this audio into MP3, AAC, M4A, WAV, FLAC, OGG or OPUS."
- **Budgets:** standard.
- **Inputs:** one or many audio files, or videos (the audio is taken, see 4.10).
- **On drop:** P audio side: codec, bitrate, sample rate, bit depth, channels, tags and art, chapters, encoder delay and padding, whether the source is lossy. Target buttons are ranked by what this source is usually converted to. Bitrate defaults never exceed the source's own (no upscaling a 128 kbps MP3 to 320).
- **Options:** Target (top 4, rest searchable). Quality: MP3 VBR (V2 default), AAC 128 to 256 kbps by content, Opus 96 to 128 kbps for music and lower for speech, FLAC level 5, WAV 16 or 24 bit matching the source. Sample rate: keep, except Opus (48 kHz). Mono and stereo kept.
- **Preview / Output / Result:** route ("FLAC to MP3 VBR V2"), size estimate. Output "name.mp3" next to the source, tags and cover art carried. Result: size before and after, "copied" versus "re-encoded".
- **Edge cases:** COPY when only the container changes (ADTS AAC to M4A, MP4 audio to M4A) with no loss; lossy to lossless says "gains nothing, file will be larger" and asks; lossy to lossy warns about generation loss and picks a bitrate at or above the source; gapless playback kept by writing encoder delay and padding (iTunSMPB, LAME tag); 24-bit to 16-bit with TPDF dither; WAV over 4 GB written as RF64 or W64, not a corrupt WAV; multi-channel (5.1) to stereo only on request, with a standard downmix; VBR MP3 duration fixed with an accurate header; ID3v2 and Vorbis comment mapped correctly, non-Latin tags in UTF-8; chapters kept for M4B.
- **Quality bar:** lossless paths (WAV to FLAC to WAV) are bit-exact on the PCM (hash test); lossy paths at equal bitrate are equal to ffmpeg with the same encoder and settings by ViSQOL score; wall time within 5% of the ffmpeg CLI and, because nothing uploads, faster than FreeConvert.
- **Engine:** ffmpeg, libmp3lame, libopus, libvorbis, native AAC and FLAC, AudioToolbox on Apple, libsoxr resampler (LGPL).
- **Surfaces:** bar ("wav to mp3"), rename-to-convert, right-click, CLI `panoply audio convert in.wav out.mp3`, MCP `audio_convert`.

### 5.2 Trim, merge, fade

- **Pattern / Job:** C. "Cut this audio, join several, and fade in or out."
- **Budgets:** standard.
- **Inputs:** one or many audio files.
- **On drop:** P, waveform, silence at head and tail (offered as a one-click trim), clipping markers, sample rates and channel layouts of all files. Mode inferred: one file opens Trim, many files open Merge with an order list.
- **Options:** Trim: keep or remove ranges, snapping to zero crossings. Merge: order, gap length (0 default), crossfade length (0 default). Fade in and out: length and curve (equal power for crossfades). Precision for lossy: Frame-exact (COPY, default; boundary lands on a codec frame, within about 26 ms for MP3 at 44.1 kHz) or Sample-exact (re-encodes once, warned).
- **Preview / Output / Result:** waveform with the selection and audible preview of the edges; "No re-encode" label when COPY applies. Output "name (trimmed).mp3", "name (merged).mp3". Result: duration, copied or re-encoded.
- **Edge cases:** fades and crossfades force a re-encode of the affected audio (lossless formats lose nothing); merging files of different sample rates, channel counts or codecs converts the odd ones to the first file's format; mono joined to stereo by duplication; clipping at the join (summed crossfades) limited to stay under full scale; MP3 bit reservoir at cut points handled by starting on a frame whose back-reference is available (else re-encode that frame); WAV over 4 GB (RF64, BIG); M4B chapters carried or rebuilt; tags merged from the first file; multiple audio tracks in a container (AUD) trimmed together.
- **Quality bar:** COPY trims are bit-exact outside the boundary frames and gapless; a crossfade of two sines shows no level dip beyond a stated tolerance; speed at disk speed for COPY. Versus Audacity: the same edits with no project, no export dialog.
- **Engine:** ffmpeg demux and mux, own frame-accurate splicer for MP3 and AAC, libswresample.
- **Surfaces:** panel editor, bar "trim audio", right-click, CLI `panoply audio trim in.mp3 --keep 0:10-1:40 --fade-out 2s` and `panoply audio merge a.mp3 b.mp3`, MCP `audio_trim`, `audio_merge`.

### 5.3 Compress

- **Pattern / Job:** B. "Make this voice note or podcast smaller."
- **Budgets:** standard. The speech-and-music classifier (target under 5 MB) has no OS engine, so it downloads on first use, loads per job and unloads after 60 s idle on every platform.
- **Inputs:** one or many audio files or video files (audio only).
- **On drop:** P, and a speech versus music classification, plus a channel-correlation test (if left and right are nearly identical the file is really mono). Default: speech goes to mono Opus, music keeps stereo at a higher rate.
- **Options:** Target: Smallest that sounds good (default), Under a size (SIZE rule for audio: `bps = target_bytes * 0.98 * 8 / duration`, stereo and sample rate lowered only as far as the floor allows), or Quality. Format: Opus for personal use, M4A (AAC) when the file is going to other people or apps (preset names carry the choice). Mono for speech (auto), sample rate for speech lowered to 24 kHz or 16 kHz.
- **Preview / Output / Result:** a 10-second A/B (loudness matched) and size estimate. Output "name (compressed).m4a". Result: "48 MB to 6 MB, 87% smaller, mono, speech intelligibility unchanged".
- **Edge cases:** already efficient files left alone (say so); music mistaken for speech (the classifier has a confidence and asks when unsure); clipped audio kept clipped, not boosted; chapters and tags preserved; multiple tracks (AUD) compress each, or only the chosen; WAV over 4 GB (BIG) streamed; very low targets that would break intelligibility are refused with the nearest workable size.
- **Quality bar:** at equal size, ViSQOL and a blind listening panel favour or tie ffmpeg libopus at default settings; for speech, word error rate after compression (measured with Whisper) differs from the original by no more than a stated margin, which is measurable and published.
- **Engine:** Speech and music classifier: OS engine: none. Own model on every platform (small model, license of the model chosen at spike). libopus, AudioToolbox or native AAC, libsoxr.
- **Surfaces:** bar ("compress audio under 5 mb"), right-click, CLI `panoply audio compress in.wav --under 5mb`, MCP `audio_compress`.

### 5.4 Normalize

- **Pattern / Job:** A, with a target dial. "Make the loudness even and meet the broadcast or podcast standard."
- **Budgets:** standard.
- **Inputs:** one or many audio files or videos (see 4.10 for the video-side path).
- **On drop:** loudness measured with libebur128: integrated LUFS, loudness range, short-term maximum, true peak, plus a clipping count and channels. Default target comes from content: speech or podcast: -16 LUFS; music: -14 LUFS; a file that came from a broadcast delivery (a drop labelled with a standard) uses that standard. Presets: EBU R128 (-23 LUFS, -1 dBTP), Apple Podcasts and streaming targets (values live in preset files with the source link and date), ATSC A/85 where requested.
- **Options:** Target (above), Mode: Loudness (default), Peak, or ReplayGain tags only. Linear gain when peaks allow; otherwise a true-peak limiter with a short look-ahead, never dynamic compression. Mono voice targets use the mono convention noted in the preset.
- **Preview / Output / Result:** before and after numbers on a meter and a loudness-matched A/B. Output "name (normalized).mp3". Result: LUFS before and after, true peak, gain applied, limiter engaged or not.
- **Edge cases:** ReplayGain tags (COPY, nothing re-encoded) and MP3 global-gain (lossless but in 1.5 dB steps, shown in the preview as approximate) are offered for lossy files to avoid generation loss; very low-level files with a loud click (gain limited by the click, user asked whether to limit it); files with a long silence (gated measurement handles it); mono versus stereo; multiple audio tracks (AUD) normalised separately or as a mix; clipped input warned and not "fixed"; 5.1 content measured with channel weights; BIG.
- **Quality bar:** integrated loudness within 0.5 LU of target and true peak at or below ceiling on every corpus file; meter validated against the EBU Tech 3341 and 3342 test signals (verify the exact test set); versus ffmpeg loudnorm single pass: no pumping, versus two pass: same result at least as fast.
- **Engine:** libebur128 (MIT, verify) for measurement, ffmpeg for gain and a true-peak limiter, libswresample for oversampling.
- **Surfaces:** bar ("normalize to -16 lufs"), right-click, CLI `panoply audio normalize in.mp3 --target -16`, MCP `audio_normalize`.

### 5.5 Remove noise

- **Pattern / Job:** B. "Get rid of the hum, hiss and background chatter."
- **Budgets:** standard. DeepFilterNet (a few MB, verify) or RNNoise (under 1 MB) has no OS engine, so it downloads on first use, loads per job and unloads after 60 s idle on every platform. Audio is streamed in blocks with overlap.
- **Inputs:** one or many audio files or videos (audio replaced, video COPY).
- **On drop:** noise floor, estimated signal-to-noise, mains hum at 50 or 60 Hz with harmonics (FFT peaks), steady hiss, speech versus music, reverberation estimate, clipping. Default method per finding: hum: notch filters (⚡, no model); hiss and room noise on speech: DeepFilterNet; music: only notch and light steady-noise reduction, with a note that the model is trained for speech.
- **Options:** Strength (set from the measured SNR, default higher for noisier files), Keep some of the original (dry mix, small default to avoid an over-processed sound), Remove reverb (off, not in v1 default), Hum frequency (auto). Channels processed independently.
- **Preview / Output / Result:** a 10-second loudness-matched A/B so the user hears noise removal, not volume, and a spectrogram before and after. Output "name (clean).wav" or the source format, video kept as COPY. Result: noise floor before and after in dB, hum removed or not.
- **Edge cases:** the model works at 48 kHz so other rates are resampled and returned at the original; stereo recordings with different noise per side; speech and a second person in the background is not removed unless it is quiet (said honestly); overly aggressive removal causes watery speech, so the strength is capped by a speech-quality check; clipping first; multiple audio tracks (AUD); mono and stereo; video file keeps video untouched, audio re-encoded once to the same codec; BIG streamed in blocks with overlap.
- **Quality bar:** DNSMOS (Microsoft, verify) and a blind listening panel against Adobe Podcast Enhance on the corpus; plus the stricter test that matters here: word error rate after processing is not worse than before and speaker similarity stays above a stated threshold, i.e. we remove noise without changing the voice. Real-time factor reported on a recent laptop.
- **Engine:** Noise removal: OS engine: none verified. Own model on every platform: DeepFilterNet (MIT or Apache-2.0 for code, weights license verify), RNNoise (BSD-3, verify) for light paths. ffmpeg notch and highpass filters, FFT hum detector (no model).
- **Surfaces:** panel, right-click, CLI `panoply audio denoise in.wav [--hum-only]`, MCP `audio_denoise`.

### 5.6 Vocal remover (stems)

- **Pattern / Job:** B. "Split vocals, drums, bass and the rest."
- **Budgets:** exception on peak memory and time: htdemucs has about 170 MB of weights per model (htdemucs_ft is a bag of four, run one at a time). Peak is about 2 GB, set by the chunk size, which is chosen from device RAM. Overlap-add chunks keep it constant for any song length. There is no OS engine, so the model downloads on first use, loads per job and unloads after 60 s idle on every platform. Stems appear progressively.
- **Inputs:** one or many songs or videos.
- **On drop:** duration, sample rate, channels, music versus speech, lossy source detection (MP3 artifacts mean stems carry them), and a compute estimate on this device. Default: two stems, vocals and instrumental, because karaoke is the common ask.
- **Options:** Stems: 2 (vocals, instrumental), 4 (vocals, drums, bass, other), 6 (adds piano, guitar). Quality: Fast (htdemucs, default) or Best (htdemucs_ft, about 4 times slower per the project, its four models run one at a time). Output format: WAV or FLAC (default FLAC), plus "mix selected stems" with gain per stem.
- **Preview / Output / Result:** stems appear progressively, processed in overlapping segments, so the user can solo and mute within the first minute; estimate shows the full time. Output: a folder "name (stems)" of files. Result: stems, format, time taken.
- **Edge cases:** mono sources duplicated to stereo; sample rates other than 44.1 kHz resampled in and out; long files processed in overlapping chunks with overlap-add so there are no seams; hardware memory budget chooses the chunk size; stems from heavily compressed files carry artifacts; clipping after summing stems; very short clips; speech files sent to this module are redirected to Remove noise; BIG.
- **Quality bar:** separation quality (SDR) on MUSDB18-HQ matches the reference PyTorch htdemucs to within a stated tolerance after export to ONNX or Core ML, so the port loses nothing; wall time against Ultimate Vocal Remover on the same device and model, with progress that is useful before completion.
- **Engine:** Stem separation: OS engine: none verified. Own model on every platform: Demucs (MIT; Meta repo archived, maintained fork at adefossez/demucs) exported to ONNX or Core ML, run via the platform ML backend.
- **Surfaces:** panel with a stem mixer, right-click, CLI `panoply audio stems in.mp3 --stems 4`, MCP `audio_stems`.

### 5.7 Transcribe

- **Pattern / Job:** C. "Turn speech into text with timestamps and speakers, offline."
- **Budgets:** exception on peak memory, which applies to the fallback path only: as 4.11 (Whisper by device memory, VAD, 30 s windows). OS engines: no download, no resident memory for us. The fallback model downloads on first use and unloads after 60 s idle, only on platforms where the quality bar test picks it. Optional Parakeet (0.6B parameters) loads only when chosen. Diarization models, if cleared, load only when speakers are on.
- **Inputs:** one or many audio files or videos (same engine as Video Subtitles, 4.11), or a drop of the Meeting recorder output.
- **On drop:** duration, language (first speech segment through Whisper language detection), speech ratio (Silero VAD), channel layout (stereo with one speaker per channel, typical for call recordings, is detected by channel energy complementarity and used for speakers), loudness (quiet files are gain-staged before recognition), sample rate (phone-band audio flagged). The default model size is picked from device memory and file length.
- **Options:** Language (detected), Model: Fast, Balanced (default), Accurate. Speakers: per-channel when separated, otherwise diarization when the model row clears (see 0.3), else off. Timestamps: segment (default) or word. Vocabulary hints (names, terms) as a prompt. Output: TXT, SRT, VTT, JSON, Markdown.
- **Preview / Output / Result:** the transcript fills in as it runs, click a word to play from there, edit in place. Output next to the audio, "name.txt". Result: language, duration, word count, real-time factor.
- **Edge cases:** hallucinated text on silence or music (VAD gates segments, low confidence marked); long files chunked at VAD boundaries with context carried so words are not cut or repeated; code-switching between languages (segment by language, warn); numbers, names and acronyms; overlapping speech; multiple audio tracks (AUD) choose or merge by time; 8 kHz phone audio upsampled; hours of audio streamed (BIG); only Moonshine English MIT models are used for live paths, the community-licensed multilingual Moonshine models are not shipped until their terms are read.
- **Quality bar:** word error rate and word timing on a labelled multilingual set, reported per model, gate: not worse than MacWhisper at the same Whisper model size and faster in wall time through hardware acceleration (Core ML, Metal, DirectML, Vulkan); speaker error rate reported separately and only when diarization is on.
- **Engine:** OS first, own model second. Speech to text: macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): whisper.cpp (MIT), optional Parakeet TDT 0.6B v3 (CC-BY-4.0) for European batch. The Timestamps option offers word level. The OS engine must pass the timestamp-accuracy part of the quality bar test, not only word error rate. Voice activity: OS engine: none. Own model on every platform (Silero VAD, MIT). Diarization: OS engine: none. Own model on every platform: pyannote-based, only if redistribution is confirmed.
- **Surfaces:** panel, bar ("transcribe"), right-click, CLI `panoply audio transcribe in.m4a --srt`, MCP `audio_transcribe`.

### 5.8 Cut by transcript

- **Pattern / Job:** C. "Select words to delete them from the audio."
- **Budgets:** as 5.7 and 4.11 (shared transcript service, OS engines first, fallback only on platforms where the quality bar test picks it). Splicing is standard.
- **Inputs:** one audio file or video (shared with Edit by transcript, 4.12).
- **On drop:** runs the Transcribe engine with word timestamps; marks fillers ("um", "uh", repeated words, false starts) and long pauses, and the speaker of each word when known. Arms "Remove fillers" with the list highlighted.
- **Options:** Filler removal: off, review each (default), automatic. Pause cap (default from the speaker's own average pace). Cut treatment: crossfade (default 10 ms) and Room tone fill (default on: the gap is filled with a sample of the recording's own background, not silence). Output format: same as the source.
- **Preview / Output / Result:** struck-through words, the resulting waveform, scrub to hear each cut, original and edited duration. Output "name (edited).mp3". Result: words removed, time saved, cuts made, whether a re-encode happened.
- **Edge cases:** word boundaries are fuzzy in fast speech, so cuts snap to the nearest VAD boundary and zero crossing and can be nudged; lossless sources (WAV, FLAC) are exact; lossy sources re-encode once (stated); breath sounds before words removed with the word; two speakers overlapping (cut the selected speaker's words only when diarized, else warn); multi-track recordings (AUD) are cut together; stereo versus mono; BIG streamed.
- **Quality bar:** no audible clicks at cuts in a blind check; cuts land within a stated tolerance of true word boundaries on a labelled set; speed: edit is exported at least as fast as the Transcribe step. Versus Descript: nothing uploaded, no subscription.
- **Engine:** OS first, own model second. Speech to text is the shared transcript service with its per-platform ladder. macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): whisper.cpp (MIT) with Silero VAD (MIT). This module cuts on word timestamps. The OS engine must pass the timestamp-accuracy part of the quality bar test, not only word error rate. Voice activity: OS engine: none. Own model on every platform (Silero VAD). Own splicer and ffmpeg, room-tone sampler.
- **Surfaces:** panel, CLI `panoply audio edit-text in.wav --remove-fillers`, MCP `audio_edit_by_transcript`.

### 5.9 Meeting recorder

- **Pattern / Job:** G. "Record my meeting, no bot joins, and give me notes, offline."
- **Budgets:** exception: always on while a meeting runs, idle otherwise. With no meeting: zero CPU, the meeting-app detector uses OS audio-device notifications (no polling) and stays under 10 MB resident. During a call: capture, echo cancellation and incremental transcription run under a call-safe CPU budget so the call does not degrade. OS engines: no download, no resident memory for us. On the fallback path Whisper downloads on first use, loads at meeting start and unloads 60 s after the last transcription. Notes use a language model only from the ladder in the Engine line: no download for us on OS engines, run once after the meeting ends, never during the call. We ship no language model, so where no engine is available the notes feature is hidden and the transcript still works.
- **Inputs:** none. Microphone plus system audio.
- **On drop (on start):** detects the meeting app using the mic (Zoom, Meet, Teams and others) and offers to start; reads the calendar event title for the name if calendar access is granted; checks devices, permission status and whether the mic can hear the speakers (echo risk). The default is mic plus system audio on two separate tracks.
- **Options:** Sources (mic and system, default), Auto-start when a meeting app opens the mic (off, opt-in, detected through OS audio-device notifications, never polling), Notes after the meeting: transcript only, or transcript plus summary, action items and key moments (default), Consent reminder shown before the first recording (a plain notice that some places require telling participants; no legal advice). Retention: keep audio, or delete it after transcription (default keep).
- **Preview / Output / Result:** level meters for both tracks, a "recording" indicator that is always visible, elapsed time. Output: folder "Meeting 2026-10-13 Standup" with a two-track audio file, transcript, notes. Result: duration, speakers, action items found.
- **Edge cases:** the mic hears the speakers, so echo cancellation uses the system track as the reference (WebRTC AEC3, BSD-3, verify) before transcription; "You" versus "Remote" comes free from the two tracks, remote speakers are separated only when diarization clears (0.3); Bluetooth headsets that switch to low-quality call mode are detected and warned; device change mid-meeting; sleep and lock; crash recovery from segment files; meetings over four hours transcribed incrementally in the background under a CPU budget so the call is not degraded. Platform limits: macOS needs system audio capture permission (ScreenCaptureKit or Core Audio taps, verify the minimum OS); Windows uses WASAPI loopback; Linux uses the PipeWire monitor source; iOS cannot capture other apps' audio, so the phone mode is in-room mic only; Android restricts playback capture, calls are blocked. Protected-content apps may record silence and are reported.
- **Quality bar:** word error rate on a two-channel meeting corpus; recall of action items against labelled meetings; CPU and battery use during a live call under a stated budget; no audio or text ever leaves the device (network inspection test in CI). Versus MacWhisper's transcription at the same model size.
- **Engine:** OS first, own model second. Transcription: macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): whisper.cpp (MIT) with Silero VAD (OS engine: none. Own model on every platform). Notes (summary, action items, key moments): macOS and iOS: Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices). Android: Gemini Nano via ML Kit GenAI APIs (Summarization, Proofreading, Rewriting, Prompt; beta, listed devices). Windows: Windows AI Phi Silica (Copilot+ only, being replaced by Aion Instruct). Else the user's own Ollama, LM Studio or API key. Else the feature is hidden. We ship no language model (DECISIONS.md), so there is no fallback language model. Diarization: OS engine: none. Own model on every platform. Platform capture APIs, WebRTC AEC3 (DSP, no model).
- **Surfaces:** tray and panel, global hotkey, CLI `panoply audio meeting start|stop`, MCP `audio_meeting_start`, `audio_meeting_stop`.

### 5.10 Dictation

- **Pattern / Job:** G. "Talk into any text field, anywhere."
- **Budgets:** exception: always on key hook, event driven, zero wake-ups when idle, under 10 MB resident. OS engines: no download, no resident memory for us. On the fallback path the speech model loads on key down and unloads after 60 s idle. The small streaming model (tens of MB) loads in well under 100 ms, so loading overlaps the user starting to speak. A larger Whisper model pays the load once after a long idle. The headline budget is latency from key release to text.
- **Inputs:** none. Microphone and the focused text field.
- **On drop (on key press):** detects the focused app and field type (plain text, rich text, code editor, terminal, password field), keyboard language, mic and its noise level. Password and secure fields are never dictated into. English with a quiet mic uses the low-latency streaming model; other languages or noisy rooms use Whisper. The model loads on key down, so loading overlaps the user starting to speak, and it unloads after 60 s idle.
- **Options:** Key (push to talk is the default, toggle optional; the default is a key that is not already bound), Language (auto from keyboard layouts, else detected), Insert method: paste with clipboard restore (default for long text), typed events (default for short text and apps that block paste), Cleanup level (see 5.11), Custom words (see 5.11).
- **Preview / Output / Result:** a small floating indicator with level and a live partial transcript; the text is inserted at the cursor when the key is released. No file output. Result: words, duration, engine used, kept in History.
- **Edge cases:** one undo removes the whole insertion; the clipboard is restored after pasting; apps that block accessibility or paste (a fallback to typed events); Wayland restricts global input, so Linux needs a documented portal-based path (verify); iOS keyboard extensions cannot record from the microphone directly, so the phone path is an app handoff (verify, spike); Android uses an input method; Bluetooth mic latency; long dictations (over a minute) are chunked at pauses and inserted progressively; the user speaks over music; multi-language sentences.
- **Quality bar:** latency from key release to text appearing, median and 95th percentile, on a mid-range laptop and a phone; word error rate on a noisy dictated corpus; both against Wispr Flow on the same machine and passage. Ours works with the network off.
- **Engine:** OS first, own model second. Streaming speech to text: macOS and iOS: Apple SpeechAnalyzer with SpeechTranscriber (macOS and iOS 26, handles long audio). Android: SpeechRecognizer.createOnDeviceSpeechRecognizer (short-utterance oriented) and ML Kit GenAI Speech Recognition (alpha). Windows: Windows AI Speech Recognition (SpeechRecognitionModel, experimental, not production yet, batch from file or streaming) and Windows.Media.SpeechRecognition. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): Moonshine English (MIT) streaming and whisper.cpp (MIT). The OS engine must also pass the latency part of the quality bar test, since the headline budget is key release to text. Voice activity for endpointing: OS engine: none. Own model on every platform (Silero VAD). Platform accessibility and input APIs.
- **Surfaces:** system-wide hotkey, tray, CLI `panoply audio dictate` (stdout, for scripts), MCP `audio_dictate` returns text from a recording.

### 5.11 Dictation cleanup

- **Pattern / Job:** G. "Remove the ums, handle 'scratch that', learn my words, and edit selected text by voice."
- **Budgets:** standard for the Light level (rules only, no model). Rewrite and voice edits use an OS language model where present (no download, no resident memory for us). We ship no language model, so on a platform with none (and no user Ollama, LM Studio or API key) Rewrite is hidden. Rewrite is off by default.
- **Inputs:** the raw dictated text (from 5.10), selected text plus a spoken command, or a file's transcript.
- **On drop (on text):** detects context (email, chat, code, notes) from the app and field, the user's dictionary and replacements, and whether text is selected. Context sets defaults: chat gets casual casing, code gets no auto-capitals, email gets full sentences.
- **Options:** Level: Light (default: remove fillers and repeats, fix punctuation and casing), Rewrite (tidy phrasing with the small language model, off by default), Off. Voice commands on: "scratch that", "new line", "new paragraph", "make that shorter", "translate to Spanish". Dictionary: words learned automatically when the user corrects the same word twice; replacements ("my email" expands to the address).
- **Preview / Output / Result:** a diff the user can peek at for 3 seconds after insertion with one key to restore the raw version. Output is the text in place. Result: what changed (fillers removed, words learned), raw kept in History.
- **Edge cases:** never change numbers, names, dates, quoted text or code; "um" inside a quoted word stays; "scratch that" removes the last sentence only, not the last insertion unless that is the sentence; commands versus content ("write the words scratch that"); mixed language; selected text edits show the old text for undo; very short utterances skip the model; Rewrite is optional because of its cost.
- **Quality bar:** edit distance to human cleanup on a labelled set; an invariant test that numbers and proper nouns are unchanged across thousands of cases; Light level adds no more than a stated latency; blind preference against Wispr Flow's cleaned output.
- **Engine:** OS first, no own model. Rewrite and voice edits (text-smart): macOS and iOS: Apple Foundation Models framework (macOS and iOS 26, Apple Intelligence devices). Android: Gemini Nano via ML Kit GenAI APIs (Rewriting, Proofreading, Prompt; beta, listed devices). Windows: Windows AI Phi Silica (Copilot+ only, being replaced by Aion Instruct). Else the user's own Ollama, LM Studio or API key. Else the feature is hidden. We ship no language model (DECISIONS.md), so there is no fallback language model. Rule layer and the STT model's own punctuation for the Light level (no model), on-device except a key the user supplies themselves.
- **Surfaces:** part of dictation, panel for dictionary, CLI `panoply audio cleanup transcript.txt`, MCP `audio_dictation_cleanup`.

### 5.12 Text to speech

- **Shipped (v1):** `text.to_speech` (pattern A with a text option, so it takes a file or pasted words). The operating system's own voices only: AVSpeechSynthesizer on macOS, Windows.Media.SpeechSynthesis on Windows. No model, no download, nothing resident. Input: pasted text, TXT, MD (markup removed) or the text layer of a PDF. Options: voice (name, identifier or language tag; `panoply audio voices` lists them), speed 0.5 to 3.0. Output: WAV, written to a file and never played; long text is read in chunks of whole sentences with progress and cancel. Linux hides the tool: Piper and Kokoro depend on espeak-ng (GPL-3.0), so the fallback model described below is blocked until a phonemiser with a clean licence exists (docs/KNOWN-LIMITATIONS.md). Not built yet: M4A, MP3 and M4B output, the pronunciation dictionary, per-language voice switching, text normalisation of numbers and dates beyond what the system voice does, EPUB and article input, the Read aloud now mode.
- **Pattern / Job:** E. "Read this text aloud with a natural voice, offline."
- **Budgets:** standard for CPU and UI. OS engines: no download, no resident memory for us. The fallback voice model (Kokoro-82M about 90 MB quantized, about 330 MB full, or KittenTTS 25 to 80 MB on phones, size shown before the first download) downloads on first use, loads when playback or rendering starts and unloads after 60 s idle, only on platforms where the quality bar test picks it. Long text renders in chunks behind the first sentence.
- **Inputs:** none: pasted text, a selection, the clipboard, a TXT, MD, EPUB or article file.
- **On drop:** language per paragraph (detected), length, reading time, structure (headings, chapters, lists), numbers, dates, currency and abbreviations. Default voice is chosen for the detected language and the user's last pick.
- **Options:** Voice (named list with a one-sentence sample), Speed (1.0), Pauses (from punctuation and headings), Pronunciation dictionary (words to say differently), Output: M4A or MP3 (default), WAV, or M4B with chapters for long text. Read aloud now (stream to speakers) or save as a file.
- **Preview / Output / Result:** the first sentence plays within moments, the rest renders behind it; estimated duration and file size. Output "name.m4a". Result: duration, voice, real-time factor.
- **Edge cases:** text normalisation (numbers, ordinals, dates, currency, URLs, code) is the part that makes or breaks quality, so it is its own tested layer per language; mixed-language text switches voices per span; long texts chunked by sentence with smooth joins; chapter files produce one audio per chapter plus one M4B; G2P and licensing: Kokoro's pipeline can fall back to espeak-ng, which is GPL-3.0 (verify), so the default path uses a non-GPL phonemiser or runs it only as a separate optional download; unsupported language is stated plainly; very long input over a book length handled by streaming (BIG).
- **Quality bar:** round-trip intelligibility (text to speech to Whisper transcript, word error rate) and a blind listening panel against the OS voices and against Piper as baselines; real-time factor reported per device class; no robotic seams at chunk joins (a seam detector test).
- **Engine:** OS first, own model second. macOS and iOS: AVSpeechSynthesizer. Android: Android TTS. Windows: SAPI and OneCore. Linux: fallback. Fallback model (any platform where the OS engine is absent or fails the quality bar test in CI): Kokoro-82M (Apache-2.0) as default on laptops; KittenTTS nano, micro or mini as the phone and low-end tier once each model repo's license is read (the KittenTTS code is Apache-2.0, model licenses are per model); ONNX runtime with platform accelerators.
- **Surfaces:** bar ("read this aloud"), selection hotkey, panel, CLI `panoply audio speak "text" --voice <name> -o out.m4a`, MCP `audio_speak`.

### 5.13 Speed and pitch

- **Pattern / Job:** B. "Change the speed or the pitch without changing the other."
- **Budgets:** standard.
- **Inputs:** one or many audio files, or videos (see 4.8 for the video-side path).
- **On drop:** speech versus music, duration, BPM and key (from 5.18's engine, so musical presets make sense), channels. Default mode: Speech for speech (voice-tuned, formant-preserving pitch), Music otherwise (transient preserving).
- **Options:** Speed (percent, or "match BPM to N"), Pitch (semitones and cents, or "to key"), Link them (Varispeed, tape style) as an off option, Formant preservation (on for speech), Quality (Fast, Best).
- **Preview / Output / Result:** a 10-second A/B, new duration and size. Output "name (1.5x).mp3". Result: duration, factors, whether re-encoded.
- **Edge cases:** speech above about 2 to 3 times speed turns to mush (shown as a warning with the highest speed that stays intelligible per a quick check); pitch shifts beyond an octave sound unnatural; stereo phase coherence kept by locking channels; transients (drums) smeared by naive stretch, so Music mode uses transient detection; lossy sources re-encode once (stated); multiple audio tracks (AUD); mono and stereo; long files (BIG) streamed.
- **Quality bar:** artifact level versus the Rubber Band library as a reference only (not linked, GPL), measured by blind listening and spectral smearing on drums and speech; sync preserved to one sample on lossless outputs.
- **Engine:** SoundTouch (LGPL-2.1, verify) or an own WSOLA and phase-vocoder implementation, libsoxr, ffmpeg atempo for simple cases.
- **Surfaces:** panel, bar ("speed up audio 1.5x"), CLI `panoply audio speed in.mp3 --factor 1.5 --pitch -2`, MCP `audio_speed_pitch`.

### 5.14 Remove silence

- **Pattern / Job:** B. "Tighten this recording by cutting the dead air."
- **Budgets:** standard. Silero VAD (about 2 MB) has no OS engine, so it downloads on first use, loads per job and unloads after 60 s idle.
- **Inputs:** one or many audio files or videos (see 4.20 for screen recordings).
- **On drop:** noise floor (so the threshold adapts to a noisy room, not a fixed dB), speech versus music, count and total length of silent spans, breaths. Default: shorten long silences instead of deleting all of them, so speech keeps its rhythm.
- **Options:** Threshold (auto from noise floor and VAD), Minimum silence length (0.5 s), Keep padding around speech (0.15 s), Mode: Shorten to a cap (default, cap at 0.35 s), Remove fully, or Only trim the start and end.
- **Preview / Output / Result:** waveform with the cut spans shaded, a clickable list, and "12:40 becomes 10:05". Output "name (tight).mp3". Result: time removed, spans cut.
- **Edge cases:** music with quiet passages (the module warns and defaults to Off for music); a high noise floor makes level thresholds fail, so VAD decides; word onsets clipped by cutting too tight (padding and a lookahead prevent it); room tone continuity (a cut leaves a bed of the recording's own room tone, not digital silence); breaths kept at the cap rather than removed; lossy sources re-encode once, lossless exact; multiple tracks cut together (AUD); mono and stereo; BIG streamed.
- **Quality bar:** precision and recall of cut spans against labelled files; the share of words clipped at the onset or offset (measured with transcript alignment) is under a stated gate; versus ffmpeg `silenceremove` at default settings, which clips words and removes breathing room.
- **Engine:** Voice activity: OS engine: none. Own model on every platform (Silero VAD, MIT). Own level analysis (no model), ffmpeg splice.
- **Surfaces:** panel, CLI `panoply audio unsilence in.wav --cap 0.35s`, MCP `audio_remove_silence`.

### 5.15 Stereo to mono (and channels)

- **Pattern / Job:** A. "Make it mono, swap, split or merge channels."
- **Budgets:** standard.
- **Inputs:** one or many audio files or videos; two mono files for Merge.
- **On drop:** channel layout, per-channel loudness, inter-channel correlation, a mono-compatibility test (does summing cancel?), a "one side is silent" detector (common when a mic was plugged into one channel), a polarity-inverted channel detector. Default action follows the finding: silent left channel offers "use the live channel for both".
- **Options:** Downmix (default: equal-power mix with a check for cancellation, falling back to mid or a single channel if sum loses energy), Use left, Use right, Swap L and R, Split to two mono files, Merge two mono files into stereo, Mono to stereo (duplicate). 5.1 downmix uses the standard coefficients.
- **Preview / Output / Result:** channel meters and the plan in words ("left is silent, will duplicate right"). Output "name (mono).wav". Result: layout before and after, loudness change.
- **Edge cases:** summing two correlated channels raises level (gain compensated and clipping checked); out-of-phase channels cancel (detected, fixed by flipping polarity, said plainly); mid and side encoded files; stereo tracks inside a video (AUD); lossless splits and swaps are bit-exact; lossy sources re-encode once, offered as WAV or the same codec; BIG.
- **Quality bar:** loudness after downmix within a stated tolerance of the stereo original; split and swap are bit-exact on lossless inputs (hash); zero cancellation on the corpus of phase-inverted files after the compatibility check.
- **Engine:** ffmpeg pan and channelsplit, libswresample.
- **Surfaces:** bar ("stereo to mono"), right-click, CLI `panoply audio channels in.wav --mono` or `--split` or `--merge l.wav r.wav`, MCP `audio_channels`.

### 5.16 Ringtone maker

- **Pattern / Job:** C. "Make a ringtone from this song for my iPhone or Android."
- **Budgets:** standard.
- **Inputs:** one audio file or video.
- **On drop:** duration, a proposed best segment (the most repeated passage by self-similarity, else the highest-energy window), BPM for beat-aligned in and out, loudness. Platform defaults from the device the user sends to, else the host OS.
- **Options:** Platform (iPhone, Android, generic), Start and end (proposed), Fade in and out (0.5 s), Loudness (normalised to a ringtone-appropriate target from the preset file, which also carries per-platform length limits with their source link and date), Format by platform: M4R (AAC in an M4A container) for iPhone, MP3 or OGG for Android.
- **Preview / Output / Result:** waveform with the chosen window, audible preview, estimated size. Output "name.m4r" or "name.mp3". Result: length, size, and the platform's next step in one line (iPhone requires importing through Finder or Music, or GarageBand on the phone; verify the current steps per iOS version).
- **Edge cases:** iPhone length and format limits differ between ringtones and text tones (preset file); AAC sources are COPY for the clip when edge frames allow, else re-encoded once; cut at a beat and a zero crossing to avoid a click; stereo and mono; clipping after loudness; DRM-protected purchased files cannot be used (said plainly); video sources use the audio track chosen (AUD).
- **Quality bar:** the file is accepted by real iPhone and Android devices (manual test per release on one model of each); no click at the loop-start and fade points (waveform discontinuity test).
- **Engine:** ffmpeg, AudioToolbox or native AAC, libebur128, own segment finder.
- **Surfaces:** panel, phone share sheet, right-click "Make ringtone", CLI `panoply audio ringtone song.mp3 --platform iphone`, MCP `audio_ringtone`.

### 5.17 Tag editor

- **Pattern / Job:** C (a table). "Fix the title, artist, album and cover art, for many files at once."
- **Budgets:** standard.
- **Inputs:** one or many audio files, or a folder.
- **On drop:** reads all tag formats (ID3v1, ID3v2.3 and 2.4, APE, Vorbis comments, MP4 atoms, RIFF INFO), art, ReplayGain, encodings. Finds problems: mojibake (Latin-1 versus UTF-8), missing track numbers, mixed album artists, art that is huge or missing, tags that disagree between formats. Offers each fix with a count ("fix encoding on 32 files").
- **Options:** Field edits, patterns: Tags from filename and filename from tags, Case fixing (off by default, per field), Art: embed from file, resize (default long side from a preset, below a stated size), remove. Lookup by fingerprint or name is network-based and opt-in, shown as such; the default is entirely offline.
- **Preview / Output / Result:** a before and after table across all files, nothing written until Return. Writes in place with the original kept (originals are sent to Trash only if overwrite is chosen; tag-only edits can be written as a sibling copy). Result: files changed and fields changed.
- **Edge cases:** tag rewrite never touches audio frames (COPY, hash verified), uses padding to avoid rewriting the file; if a rewrite is needed (M4A with the atom at the end, BIG) it is streamed; ID3v2.3 with UTF-16 written for compatibility with players that mishandle 2.4; multiple art types (front, back); chapters; non-Latin scripts; read-only files and permissions; duplicate tag blocks removed.
- **Quality bar:** every written file reads back identically in at least three reference readers (mutagen, Music or iTunes, foobar2000), and the audio stream hash is unchanged; faster than opening each file in a GUI tag tool, measured on 1,000 files.
- **Engine:** lofty (Rust; MIT or Apache-2.0, verify) and own mapping layer; optional MusicBrainz and AcoustID lookups, network, opt-in.
- **Surfaces:** panel, right-click, CLI `panoply audio tags song.mp3 --set artist="X"` and `panoply audio tags ./album --from-filename "{track} - {title}"`, MCP `audio_tags`.

### 5.18 BPM and key

- **Pattern / Job:** I. "Tell me the tempo and the musical key."
- **Budgets:** standard.
- **Inputs:** one or many audio files or videos.
- **On drop:** the report is the result: tempo (and tempo changes), beat grid, half and double tempo candidates, key and mode with a Camelot code, confidence, tuning offset in cents, whether the file has a clear beat at all. A DJ mix or a very long file is split into segments with a tempo and key per segment.
- **Options:** Range hint (default 60 to 200 BPM, resolves half and double ambiguity toward the more common range), Write to tags (BPM, initial key; off), Show beat grid on waveform.
- **Preview / Output / Result:** the report with a clickable beat grid and a tap-along to verify; the fix buttons: write tags, Speed and pitch to match BPM or key (5.13), Loop maker (5.20). Output: none by default. Result: BPM, key, confidence.
- **Edge cases:** ambient and rubato music with no beat (reports "no steady tempo" and does not invent one); tempo changes; modulations; detuned recordings (A=432 or tape speed) reported with cents offset instead of a wrong key; speech and non-musical audio flagged not applicable; mono and stereo; clipping irrelevant; huge files analysed in segments.
- **Quality bar:** tempo accuracy (both Accuracy1 and Accuracy2) and key accuracy (Giant Steps dataset and a labelled electronic set) reported against open baselines (librosa; Essentia is AGPL so used as a reference only, not linked); results appear in a stated time per track.
- **Engine:** own Rust spectral-flux onset detection, tempogram and chroma templates, no model required (⚡).
- **Surfaces:** bar ("bpm of this"), panel, CLI `panoply audio bpm in.mp3 [--json]`, MCP `audio_bpm_key`.

### 5.19 Audio to MIDI

- **Pattern / Job:** B. "Get the melody and notes out of a recording."
- **Budgets:** standard. Basic Pitch (under 1 MB) has no OS engine, so it downloads on first use, loads per job and unloads after 60 s idle on every platform.
- **Inputs:** one audio file or video; a recording from the Recorder.
- **On drop:** monophonic versus polyphonic estimate, instrument family guess (voice, guitar, piano), tempo and key (5.18 engine), noise level. A mixed song triggers "separate first" with a one-click chain to Vocal remover (5.6), because notes are far more accurate on an isolated stem.
- **Options:** Mode: Melody (monophonic, default for voice) or Polyphonic, Sensitivity (note onset and frame thresholds), Minimum note length, Quantize (off, 1/8, 1/16), Pitch bends (on for voice and guitar), Tempo (detected, else 120).
- **Preview / Output / Result:** a piano roll beside the waveform, playback of the notes with a small bundled General MIDI sound font (a permissive-licensed font chosen at spike) layered over the original to hear the match. Output "name.mid". Result: notes, range, tempo.
- **Edge cases:** drums and noise produce false notes (filtered by a minimum confidence); octave errors; vibrato and glides cut into too many notes (pitch-bend merge); tuning offset (cents detected and compensated); very low or high notes outside the model range; polyphony in mixes is unreliable and said so; long files chunked with overlap; mono and stereo summed; BIG.
- **Quality bar:** note-level F-measure on a standard transcription set (MAESTRO for piano) reproducing the published Basic Pitch results within a stated tolerance after port; a blind "does the MIDI sound like the recording" test.
- **Engine:** Audio to MIDI: OS engine: none verified. Own model on every platform: Basic Pitch (Apache-2.0 on one secondary source, verify) exported to ONNX. Own post-processing and quantiser (no model).
- **Surfaces:** panel, right-click, CLI `panoply audio to-midi in.wav --mode melody`, MCP `audio_to_midi`.

### 5.20 Loop maker

- **Pattern / Job:** B. "Make a loop with no visible jump from this clip."
- **Budgets:** standard.
- **Inputs:** one audio file or video.
- **On drop:** duration, BPM and beat grid (5.18 engine), whether the clip already loops (end meets start), reverb tail length. Default: the loop length is a whole number of bars (1, 2, 4 or 8) that fits the clip, start and end snapped to beats and zero crossings, with the best seam found by searching for minimum discontinuity near those points.
- **Options:** Bars (auto), Seam: none, Crossfade (auto length, minimum that hides the seam), Tail wrap (overlap-add the reverb tail onto the start, on for music with tails), Repeat count or total duration (for video backgrounds), Format: WAV (default), FLAC, OGG with loop tags (LOOPSTART, LOOPLENGTH) for game engines, M4A or MP3 with a gapless warning.
- **Preview / Output / Result:** the waveform with the loop region and an endless loop player with the seam marked, plus a seam score. Output "name (loop).wav". Result: length in bars and seconds, seam score.
- **Edge cases:** no beat (ambient: spectral similarity instead of the beat grid); tempo drift flagged; MP3 and AAC encoder padding creates audible gaps at the seam (so lossless is the default and lossy warns); stereo phase at the crossfade (equal power); loops of speech make no sense (flagged); long clips; mono and stereo.
- **Quality bar:** seam discontinuity (sample jump and spectral difference across the join) under a stated threshold on the corpus, and a blind test where listeners cannot locate the seam; loop length is exactly the intended bars in samples.
- **Engine:** the 5.18 beat tracker, own seam search and crossfader, ffmpeg and libsoxr.
- **Surfaces:** panel, CLI `panoply audio loop in.wav --bars 4 [--repeat 8]`, MCP `audio_loop`.

### 5.21 Recorder

- **Pattern / Job:** E (no file in, file out). "Record a voice memo."
- **Budgets:** standard. Capture writes small segments to disk, so memory is constant for multi-hour recordings.
- **Inputs:** none. Microphone.
- **On drop (on open):** lists input devices, picks the last used (the built-in mic if none), measures the noise floor and level in a short pre-roll, checks free storage, and warns if a Bluetooth headset is selected (many drop to low-quality call audio when their mic is used). Defaults: mono, 48 kHz, no automatic gain (level shown with a clipping warning), voice memo preset.
- **Options:** Device, Channels (mono default, stereo), Format: records losslessly to disk during capture and converts on stop to M4A or Opus (default M4A), Live noise suppression (off, light RNNoise-style when on), Markers (a key adds a timestamp), Pause and resume, Record in background and with the screen locked on phones.
- **Preview / Output / Result:** waveform, level with a peak hold, elapsed time and remaining space. Output "Voice memo 2026-10-13 15.21.m4a" in the chosen folder; the result card's Send to device moves it to the phone. Result: duration, size, peak level, markers.
- **Edge cases:** crash safety (audio is written in small segments with a recoverable header, a crashed WAV is repaired on the next launch); phone calls and other apps interrupting; device unplugged mid-recording (continue on the default device with a visible marker); sample rate mismatch between device and file; very long recordings (BIG, WAV over 4 GB written as RF64); clipping warned live and offered Normalize and Remove noise as one-click next steps; permissions flow explained in plain words; echo avoided by not playing back while recording.
- **Quality bar:** no dropped or duplicated samples across a multi-hour recording (a known-signal continuity test); start-to-first-sample latency under a stated figure; battery cost of a one-hour phone recording reported against the platform's own voice-memo app.
- **Engine:** platform audio APIs (cpal in Rust, AVAudioEngine, Oboe on Android), libFLAC or WAV writer during capture, AudioToolbox or libopus on stop.
- **Surfaces:** panel and phone first, tray, global hotkey, CLI `panoply audio record -o memo.m4a --until-silence`, MCP `audio_record` (starts and stops with explicit calls).

---

## 6. Review of the module list (merge, split, cut, order)

54 modules in scope: 33 video, 21 audio.

### Hardest (most engineering risk)

1. **Compress video (4.2).** Hardware encoders have no true two-pass, so the size target needs the SIZE estimate and verify loop, while the quality bar (VMAF at equal size against HandBrake) is measurable and public. HEVC output is hardware-only in an LGPL build, and HDR and Dolby Vision decisions sit on top.
2. **Dictation and Dictation cleanup (5.10, 5.11).** Latency against Wispr Flow is the product, and text insertion into any app differs per OS: Wayland input limits, iOS keyboard extensions that cannot record, apps that block paste.
3. **Meeting recorder (5.9).** System audio capture is a permission and API maze per OS and iOS cannot do it at all; echo cancellation, a call-safe CPU budget, and a consent line are all required. Close behind: Remove watermark (4.22) and Blur faces (4.23), where one missed frame is a visible failure, and Smart cut (4.1) for Dolby Vision and open-GOP edges.

### Merge

- **Transcribe (5.7) and Subtitles (4.11):** one transcript service, two entry points.
- **Edit by transcript (4.12) and Cut by transcript (5.8):** one module, input type decides.
- **Dictation (5.10) and Dictation cleanup (5.11):** one module with a cleanup level.
- **Bitmap subtitles to SRT (4.13), Translate subtitles (4.15), Subtitle files (4.16):** one "Subtitle tools" module with three modes.
- **Audio (4.10) and Streams (4.17):** one "Tracks" module (mute, replace, extract, volume are track operations).
- **Crop, rotate, flip (4.5) into Resize and reframe (4.4):** one canvas, two entry points.
- **Reverse and loop (4.9) into Speed (4.8)**, and **Frame rate (4.27) into Convert (4.3)** as an option (keep the bar alias).
- **Split (4.7) into Trim and cut (4.1)** as a multi-output mode.
- **Stereo to mono (5.15) into Convert (5.1)** as a channels option, keeping the bar alias.
- **Info (4.33) into the shared Metadata inspector** with a video lens.

### Split

- **Remove watermark (4.22):** static logos by delogo (⚡, ship early) versus tracked inpainting (🧠, later).
- **Restore old video (4.19):** deinterlace and denoise (⚡) versus AI upscale (🧠).
- **Video to Live Photo (4.31):** two directions; Live Photo to video is trivial and cross-platform, the reverse is Apple-only.
- **Trim, merge, fade (5.2):** a timeline editor in one module is fine, but merge can ship first as pattern A.

### Cut or defer

- **Teleprompter (4.32):** not a file transform; belongs in Capture, defer.
- **Audio to MIDI (5.19) and BPM and key (5.18):** low evidence of demand in the sheet, ship late; BPM engine is reused by Loop maker and Speed and pitch, so keep the engine.
- **Remove noise "remove reverb":** left out of the first version.
- **Podcast cleanup** is not a module: it is a chain (Remove noise, Remove silence, Normalize) saved as a named action.

### Ship order

Wave 1 (pattern A and COPY paths, instant): Convert, Audio, Streams, Info, Trim, Repair, HDR to SDR, Stereo to mono, Normalize. Wave 2 (the demand-ranked ones): Compress video, Compress audio, Transcribe and Subtitles, Remove noise, Text to speech. Wave 3: dictation, Meeting recorder, stems, reframe, interpolation.

### Open license and fact checks before build

Moonshine non-English terms; pyannote embedding model and redistribution of gated weights; KittenTTS per-model licenses; DeepFilterNet and RIFE weights; Basic Pitch license file; Silero README badge versus MIT LICENSE; espeak-ng GPL path in Kokoro; Phi-4-mini; libebur128, RNNoise, SoundTouch and libplacebo license files; MV-HEVC decode and encode on a real iPhone spatial clip; Dolby Vision profile 5 re-encode through libplacebo; openh264 binary redistribution.
