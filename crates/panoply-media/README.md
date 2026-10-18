# panoply-media

The video and audio engine behind eleven Panoply modules, wave 1 of `docs/specs/video-and-audio.md`
(section 6, "Ship order"): `media.convert`, `media.extract_audio`, `media.trim`, `media.compress`,
`media.info`, `media.repair`, `media.normalize`, `media.denoise`, `media.to_mono`, `media.hdr_to_sdr` and `media.streams`.
The host wraps it in `crates/panoply-host/src/builtin/media.rs`; the command line is
`panoply video <convert|trim|compress|audio|streams|info|repair|sdr>` and
`panoply audio <convert|trim|normalize|channels|info>`; the manifests are in `modules/builtin/media-*/`;
measured results, losses included, are in `docs/research/media-results.md`.

## The modules

| module | spec | job | how |
|---|---|---|---|
| `media.convert` | 4.3, 5.1 | video to MP4, MOV, MKV, WebM, AVI, HEVC, AV1; audio to MP3, M4A, AAC, WAV, FLAC, Ogg, Opus | per-stream routing, copy first (`plan.rs`): only a stream the target cannot hold is encoded, audio and subtitles before video. An MP4 with its index at the end is rewritten with it at the front by own code, no re-encode |
| `media.extract_audio` | 4.10 | the sound out of a video | stream copy by default (bit-exact), or an encode on request |
| `media.trim` | 4.1, 5.2 | keep or remove ranges | MP4 and MOV: own index code, whole groups of pictures and every audio frame copied, only the frames at a cut that is not on a keyframe re-encoded (hardware) and spliced in. Other containers: stream copy snapped to a keyframe. Audio files: frame-exact copy |
| `media.compress` | 4.2, 5.3 | fit a size or an upload limit, or the smallest that looks good | `target * 0.98 * 8 / duration - audio` bits, resolution then frame rate down only to the measured floor, verify and retry, VMAF on the result. Presets for WhatsApp, Discord, Gmail, Slack, Telegram, X and form uploads are data files with source links and dates |
| `media.info` | 4.33 | everything about a file and what is wrong with it | one probe, own box walker, 200 packet times; never a decode unless `deep` asks for the loudness scan |
| `media.repair` | 4.18 | a file that will not play | diagnoses first. Own code: cut-off file with an index, missing index rebuilt from a reference (or parameter sets found in the stream). The helper re-reads anything else. Nothing is called repaired unless it decodes start to end |
| `media.normalize` | 5.4 | loudness to a target or a standard | own EBU R128 meter, linear gain, a true-peak limiter only when the peaks need it, the result measured again and corrected; ReplayGain tags without touching a sample |
| `media.denoise` | 5.5 | background noise out of a recording or the sound of a video | RNNoise in pure Rust (`nnnoiseless`, BSD-3-Clause, model inside the program, no download); 48 kHz per channel, resampled in and back out; strength is the wet share (light 60%, normal 85%, strong 100%); streamed in 10 ms frames, constant memory; the picture is copied untouched and the sound encoded once |
| `media.to_mono` | 5.15 | mono, swap, split, merge | looks at the channels first (silent side, out of phase, ordinary); swaps and splits of lossless sound are exact |
| `media.hdr_to_sdr` | 4.28 | HDR10, HLG and Dolby Vision 8.x to standard range | one pass: a 3D lookup table built by own code (PQ or HLG decode, BT.2390, Hable or Mobius tone mapping, BT.2020 to BT.709, BT.709 transfer) applied by ffmpeg's `lut3d`. Dolby Vision profile 5 is refused in plain words |
| `media.streams` | 4.17 | take out, drop, relabel tracks | stream copy; every kept stream is bit-identical; shows the stream table with no option |

## How it works, and why

**A helper process, not a linked library.** The heavy codec work runs in ffmpeg and ffprobe, an LGPL build, as a
separate process (`src/engine/`). Decision and reasons:

- *The license line is a process boundary.* Panoply is MIT or Apache-2.0 and the workspace forbids `unsafe`
  (`unsafe_code = "forbid"`), so nothing links or loads libav*, and "LGPL ffmpeg only as a separate process"
  (ADR 0002) holds with no static or dynamic linking question to answer. The helper is a program the user runs
  through us, fetched on first use and checksum pinned, never committed.
- *A damaged file cannot take the app down.* A crashing demuxer ends the helper; the job reports a plain error.
- *Cancel is a kill, memory is the helper's own streaming (flat for any duration), idle cost is zero.*
- *The OS encoders come with it.* The hardware encoders are ffmpeg's wrappers over the OS and vendor APIs
  (VideoToolbox, Media Foundation, NVENC, QSV, AMF, VA-API), allowed in an LGPL build.

The price is one process start per run (about 25 ms) and a download on first use (see below). Dynamic loading would
need `unsafe` FFI and would put the helper's crashes in the app.

**Which build, per platform** (`src/engine/pins.rs`, `engine/README.md`):

| platform | helper | pinned by |
|---|---|---|
| Windows x64, arm64; Linux x64, arm64 | BtbN FFmpeg-Builds `lgpl` variant, tag `autobuild-2026-09-30-13-08`, FFmpeg n9.0.2: no `--enable-gpl`, no `--enable-nonfree` | URL and SHA-256, checked before anything is unpacked. The tag is a month-end build BtbN keeps for two years; mirror the four archives in a Panoply release |
| macOS | none exists (every prebuilt macOS ffmpeg found is GPL). A recipe and an Actions workflow build one (`engine/`, **not yet run**) | until it is pinned: `PANOPLY_FFMPEG`, else the `ffmpeg` and `ffprobe` on `PATH`. Panoply only asks for LGPL-set encoders, so a user's own GPL ffmpeg gives the same results |

Lookup order: `PANOPLY_FFMPEG` (ffprobe beside it or `PANOPLY_FFPROBE`), the pinned build in the engines folder
(fetched once with `curl` and `tar`, https only, label and progress shown, never for a job that is not allowed the
network), then `PATH`. The cache folder is the same as pdfium's: `PANOPLY_ENGINES_DIR`, else the user's cache folder.

**Encoder ladder** (`src/encoders.rs`, spec 0.1 HWL). The first encoder that is listed *and* passes a two-frame test
encode wins: macOS VideoToolbox; Windows Media Foundation (forced to hardware), NVENC, QSV, AMF; Linux VA-API, NVENC,
QSV, AMF. Software, all permissive: SVT-AV1 (BSD), libvpx (BSD), and OpenH264 only in an ffmpeg the user supplies,
because BtbN's build carries its own copy that has no Cisco patent grant (the grant covers Cisco's binary, downloaded
by the end user). x264 and x265 are never requested, and a test asserts no candidate names them. HEVC has no software
encoder at all. With no hardware H.264 encoder the message says so and offers AV1 or VP9 instead. The result card names
which encoder ran, "hardware" or "software".

**Own code, where the helper cannot do the job** (all in `src/mp4/`, `src/loudness.rs`, `src/hdr.rs`):

- `mp4/boxes`, `mp4/movie`: the box layer and the sample tables of every track (offset, size, time, composition
  offset, sync flag), checked sample for sample against ffprobe on the corpus.
- `mp4/write`: rebuilds an index and copies sample bytes from any number of source files in time order, in one pass,
  with 64 bit offsets above 4 GB. Because a sample can point into a re-encoded piece or a copied one and the track
  can carry more than one sample description, this is how a smart cut is spliced (ffmpeg's muxer writes one).
- `mp4/cut`: plans a lossless cut from the tables: which groups of pictures are copied, which presentation
  intervals are re-encoded, and where each piece sits on the new timeline (picture delay, edit lists).
- `mp4/repair`, `mp4/poc`: finds pictures in media data with no index (H.264 and HEVC, length prefixed NAL units
  with plausibility checks tuned on real files) and puts them in display order from their slice headers (picture order
  count), without decoding.
- `loudness`: EBU R128 / BS.1770-4 meter (K-weighting derived for any sample rate and equal to the standard's
  coefficients at 48 kHz, gating, loudness range, true peak by 4x oversampling), 1300 times real time.
- `hdr`: the tone-mapping table.

## Conventions every module follows

- **Stream copy whenever possible** (COPY). The result card says "No re-encode, lossless" when that is what happened.
- **Never overwrites an input.** Results get a hidden temporary name and are moved into place only when the job
  succeeded and nothing of that name exists (`io::Stage`): `name (trimmed).mp4`, `name (trimmed 2).mp4`.
- **Cancellation** kills the helper, stops own writes between chunks, and removes every file the job made.
- **Plain errors**: every message says what happened and what to do next; the helper's raw log is never shown
  (`proc::explain`). A cut-off file points at Repair.
- **Constant memory.** The helper streams. Own code reads sample tables (about 32 bytes per sample, at most 6
  million per track, more goes to the helper) and copies data through a 4 MB buffer, so a 3 hour FLAC and a 1 hour
  video cost what a 10 second one costs plus the tables.
- **Rotation stays a flag** (`-noautorotate`), HDR tags and mastering metadata travel on HEVC and AV1 output, 10-bit
  to 8-bit goes through error diffusion, and MP4 and MOV results have the index first.
- **Presets are data.** Upload limits (`modules/builtin/media-compress/presets/*.toml`), loudness standards
  (`modules/builtin/media-normalize/presets/*.toml`) and the quality floors (`.../floors.toml`) each carry their
  source link, the quote read there, the date, and a status; `PANOPLY_PRESETS` adds or replaces files.

## Testing

`cargo test -p panoply-media` runs unit tests and integration tests over `tests/corpus` video and audio
(`python3 -I tests/corpus/fetch.py --kind video,audio`, then `python3 tests/corpus/generate.py --kind video,audio`; the
default tier is enough, the 3 hour FLAC and the 1 hour video are in the large tier). Everything is silent: nothing is
played. A test skips, with a message, when the corpus or an ffmpeg is missing. Results are checked by an independent
reader: ffprobe packet hashes (bit-identical copies), the decoder (a clean decode start to end), frame hashes, SSIM and
VMAF, the EBU meter on the decoded result, and for HDR a pixel-by-pixel comparison against the mapping function.
Splice fixtures are made with the test machine's libx264 and libx265 (open and closed groups, B-frames); they are
test inputs and never shipped. `panoply-host` has tests that run jobs through the real capability guard and cancel one
mid-run; `panoply-cli` has end to end tests of the binary.

Examples that produce the numbers in `docs/research/media-results.md`: `compress_sweep` (the quality floors),
`quality_report` (VMAF at equal size against other encoders), `speed_report` (wall time against the ffmpeg CLI),
`cut_report` (what a frame-exact cut copies and how close the re-encoded edges are), `repair_report` (recovery from
broken copies of corpus files against `ffmpeg -c copy`), `repair_scan` (what the repair scanner finds in a file).

## Budgets

Measured with `panoply-bench` (CLI runner, release build, 16 audio or 19 video default-tier files per module) and
`/usr/bin/time -l`; the tables and the machine are in `docs/research/media-results.md`.

| budget (FLOWS.md) | result |
|---|---|
| First preview, under 100 ms | `media.info` median 27 ms, slowest 46 ms; `media.streams` the same class (PASS in the harness) |
| Peak memory, constant for video and audio | The `panoply` process peaks at 3.7 to 15 MB on every module. With the helper counted: copy, convert and trim 21 to 73 MB on a 10 second clip and a 4.6 GB, 1 hour 13 minute file alike (the growth is the sample table, about 200 bytes a picture); normalize 25 MB on a 3 hour FLAC; hardware compress 214 MB; smart cut 239 to 263 MB; HDR to SDR about 300 MB; software AV1 compress 1.3 GB at 1080p (known, see limits) |
| Huge files | a 4.6 GB file with 64 bit offsets converts in 2.1 s and trims (own writer, `co64`) in 1 s at 73 MB |
| Idle cost | zero: no resident process, the helper exits with the job |
| Speed, at least as fast as the native CLI | faster for normalize (0.6x the time of `loudnorm`), within 25% for trim and smart cut, slower on jobs under 100 ms by a fixed 60 to 125 ms (two `ffprobe` runs, one before and one after the job), 2.2x for compress (the verify loop), 3.5x for HDR to SDR against a plain encode that does not tone-map |
| Output quality, equal or better than the named competitor | against hardware encoders at a naive rate: far better (VMAF 73 against 33 and 88 against 70 on the hard clips); against x264 at equal size: 2.5 to 17 points lower with the default hardware HEVC, level or better with `--codec av1`. The gap is the price of hardware first; see the results file |

Module times (p50 / p95, ms): info 44 / 120, convert video 80 / 586, convert audio 171 / 1031, extract audio 82 / 102,
trim video 357 / 686, trim audio 64 / 122, normalize 389 / 6054, to mono 225 / 2707, repair 232 / 883, HDR to SDR
47 / 2016 (most corpus files are SDR), compress to 2 MB 1122 / 5008, streams 44 / 63.

## Not done, and known limits

- **macOS engine**: the build recipe is written, not run (an Actions job). Until then macOS uses an ffmpeg on `PATH`.
- **Hardware encoders other than VideoToolbox are untested**: the Media Foundation, NVENC, QSV, AMF and VA-API
  argument sets follow ffmpeg's documentation and are unit tested as text; none ran on this Mac.
- **Windows and Linux were not run.** The code has no OS-specific part except the engines folder, a `/dev/null`
  versus `NUL` choice for a two-pass log and `curl`/`tar` for the fetch; nothing was cross-compiled or executed there.
- **Software AV1 compress uses 1.3 GB at 1080p** (SVT-AV1 lookahead and threads, bounded by resolution, not duration);
  `lookahead=8` with `lp=4` measured 822 MB at the same speed and is the first tuning to try.
- **Hardware encoders lose to x264 at tight sizes** (see the results file); `--codec av1` closes the gap at ten times the CPU.
- **OpenH264 from Cisco is not fetched** (see above), so a machine with no hardware H.264 encoder and no ffmpeg of
  its own cannot make H.264.
- **Repair cannot rebuild sound** from a file with no index (AAC frames have no length). It rebuilds the picture
  from a reference, and keeps sound when only the end of the file is missing. VFR timing of a rebuilt file is the
  reference's frame rate. Zeroed bytes in the middle of an index-complete file are copied, not dropped (that needs
  a decode).
- **HDR to SDR is a static mapping** (peak from the file's metadata, not scene by scene) and refuses Dolby Vision
  profile 5 (needs libplacebo and Vulkan, not available through every LGPL build).
- **Smart cut** works for H.264 and HEVC in MP4 and MOV at IDR (or BLA) pictures; open groups of pictures have no
  safe join, so a range with none inside is re-encoded whole when it is short and snapped to a keyframe when it is
  long. Dolby Vision, spatial video and other codecs snap to a keyframe, as the spec says.
- **The loudness meter is integrated BS.1770**, not dialogue-gated (ATSC A/85 and Netflix define dialogue loudness;
  the result card says so).
- Not in wave 1: Compress audio's speech and music classifier, Transcribe, Text to speech. Remove noise is in (`media.denoise`, spec 5.5) without hum notch filters, a spectrogram or an A/B preview.

## Licenses

MIT OR Apache-2.0. Dependencies are the workspace's own (serde, toml, sha2, thiserror). The helper is
LGPL ffmpeg run as a separate program, fetched and not distributed with Panoply; the libraries in it (libvpx, SVT-AV1,
libopus, libmp3lame, libvmaf and the rest) are permissive or LGPL, listed in `engine/README.md`.
