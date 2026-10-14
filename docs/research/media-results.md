# Video and audio engine results, wave 1

Measured on 2026-10-14 with `crates/panoply-media` (uncommitted working tree), the `panoply` command line and
`panoply-bench`. Nothing was uploaded and no competitor figure is claimed. The comparison is against the ffmpeg
command line on the same machine and the same files, plus x264 as a yardstick (it is GPL and is never shipped).

## Setup

- Machine: Apple M5 Max (18 cores), macOS, release build without LTO. The Mac was busy with other work
  the whole time (load average 12 to 14), so absolute times are pessimistic and small ones are noisy. Every
  comparison below ran back to back on the same machine, so ratios are fair, but a quiet machine would show
  smaller absolute numbers. Everything ran under `nice -n 10`, nothing was played.
- Helper engine: the ffmpeg 9.0.1 and ffprobe on `PATH` (Homebrew build). This is the macOS case until the
  Actions recipe in `crates/panoply-media/engine/` is run and pinned. Panoply only asks it for LGPL-set encoders:
  VideoToolbox, SVT-AV1, libvpx, native AAC, libopus, libmp3lame. libvmaf measures quality.
- Corpus: `tests/corpus`, default tier of video (19 files) and audio (16 files), fetched with
  `python3 -I tests/corpus/fetch.py --kind video` and `--kind audio`, generated files from `generate.py`.
  Large files for the constant-memory rows were made on the spot: a 4.6 GB, 1 hour 13 minute copy of the
  1080p clip (`ffmpeg -stream_loop`), the corpus 3 hour FLAC (246 MB) and the corpus 1 hour video.
- Reproduce: `cargo build --release -p panoply-cli`, then
  `cargo run --release -p panoply-media --example <name>` with `speed_report`, `quality_report`,
  `compress_sweep`, `cut_report`, `repair_report`; budgets with `panoply-bench run --module <id> --runner cli`.

## What shipped

Ten modules, each with a manifest in `modules/builtin/media-*`, registered in `panoply-host` and in the CLI:
`media.convert`, `media.extract_audio`, `media.trim`, `media.compress`, `media.info`, `media.repair`,
`media.normalize`, `media.to_mono`, `media.hdr_to_sdr`, `media.streams` (Streams is on the wave 1 list of the spec,
so it is a tenth module).

Tests: 114 unit tests, 14 + 10 + 5 + 2 + 5 + 11 + 4 + 5 + 14 + 9 integration tests in `panoply-media`
(convert, trim, trim splice, mp4 index, info, audio ops, hdr, streams, compress, repair), 5 tests in
`panoply-host` that run jobs through the real capability guard and cancel one mid-run, 5 end to end tests of the
CLI binary. Independent checks do the judging: ffprobe packet hashes for bit-identical copies, a clean decode of every
output, frame hashes, SSIM and VMAF, the EBU meter on the decoded result, a pixel comparison for HDR.

## Engine decisions

| decision | choice | why |
|---|---|---|
| How ffmpeg is used | LGPL build as a separate helper process, not dynamically loaded | `unsafe_code = "forbid"` and ADR 0002: a process boundary is the clearest license line, a damaged file cannot crash the app, cancel is a kill, memory is the helper's own streaming |
| Where it comes from | Windows and Linux, x64 and arm64: BtbN `lgpl` build, tag `autobuild-2026-09-30-13-08`, FFmpeg n9.0.2, fetched on first use with a pinned SHA-256 (https only, checked before unpacking, never committed). macOS: no LGPL prebuilt exists, so a build recipe plus Actions workflow is written (not run), until then `PANOPLY_FFMPEG` or `PATH` | the only public macOS builds are GPL |
| Hardware encoders | VideoToolbox (macOS); Media Foundation forced to hardware, then NVENC, QSV, AMF (Windows); VA-API, then NVENC, QSV, AMF (Linux). Each is proven by a two-frame test encode and cached | spec 0.1 HWL |
| Software fallback | SVT-AV1 (BSD), libvpx (BSD), OpenH264 only inside an ffmpeg the user supplies. No x264, no x265 (a test asserts no candidate names them) | OpenH264's patent grant covers Cisco's own binary, so Panoply should fetch that binary from Cisco, which is not built yet |
| MP3 | encoded by libmp3lame in the helper process only (LGPL) | it is never linked |
| Own code | MP4 index and cut planner, repair scanner, EBU R128 meter, HDR lookup table | the helper cannot do a multi-description smart cut, a rebuild from a reference, or a one pass tone map with our own curve |
| Default codec for Compress | hardware HEVC where the receiver is the user's own device, H.264 for app presets | speed first, see "Compress quality at equal size" for what that costs |

## Budgets (`panoply-bench`, CLI runner, release build)

The `panoply` process only (the helper's memory is listed separately below). 19 video or 16 audio default-tier files
per row, 2 to 3 runs each.

| module | files | wall p50 ms | wall p95 ms | peak MB (p50 / max) | errors | the errors are |
|---|---:|---:|---:|---:|---:|---|
| media.info, video | 19 | 44 | 120 | 3.8 / 3.9 | 0 | |
| media.info, audio | 16 | 26 | 46 | 3.8 / 3.8 | 0 | |
| media.streams | 19 | 44 | 63 | 3.8 / 3.8 | 1 | the broken file, "no index, use Repair" |
| media.convert, video to MP4 | 19 | 80 | 586 | 4.0 / 5.7 | 1 | the broken file |
| media.convert, audio to M4A | 16 | 171 | 1031 | 3.9 / 3.9 | 0 | |
| media.extract_audio | 19 | 82 | 102 | 3.8 / 3.9 | 6 | videos with no sound (plain error) and the broken file |
| media.trim, video (copy and smart cut) | 19 | 357 | 686 | 5.1 / 9.9 | 1 | the broken file |
| media.trim, audio | 16 | 64 | 122 | 3.9 / 3.9 | 0 | |
| media.normalize | 16 | 389 | 6054 | 5.4 / 6.9 | 0 | |
| media.to_mono | 16 | 225 | 2707 | 5.6 / 6.6 | 0 | |
| media.repair (no reference given) | 19 | 232 | 883 | 3.8 / 8.6 | 1 | the broken file, "give a reference" |
| media.hdr_to_sdr | 19 | 47 | 2016 | 3.7 / 14.9 | 1 | the broken file (2 files are HDR and are converted, 16 are SDR and are left alone, which is why p50 is 47 ms and p95 is 2 s) |
| media.compress to 2 MB | 19 | 1122 | 5008 | 3.9 / 4.0 | 1 | the broken file |

FLOWS budgets the harness can check: First preview PASS for `media.info` (median 27 ms, slowest 46 ms over 18 files)
and `media.streams`; peak memory PASS on every row (constant, under 300 MB); idle cost is zero (nothing runs between
jobs, the helper exits with the job). "Module start", "UI thread" and "Frame rate" belong to the desktop app
and are not measurable from the CLI runner.

### Constant memory

Peak resident set of the whole process tree (`/usr/bin/time -l`, which includes the ffmpeg helper), measured on the
same operation at two sizes:

| operation | 10 s clip | big file | big file is |
|---|---:|---:|---|
| info | 21 MB | 46 MB | 4.6 GB, 1 h 13 min, 265,000 pictures |
| convert, copy to MKV | 26 MB (0.17 s) | 48 MB (2.1 s) | the same 4.6 GB file |
| trim to keyframes (own writer, offsets above 4 GB, `co64`) | 21 MB | 73 MB (0.96 s) | the same file, 4.40 GB result |
| smart cut, edges re-encoded | 239 MB | 263 MB (1.6 s) | the same file; the helper's hardware encoder is most of it |
| normalize | 17 MB (1 min MP3) | 25 MB (23 s) | the 3 hour FLAC, 246 MB |
| info | 13 MB | 13 MB | the 3 hour FLAC |

What grows is the sample table kept in memory for own MP4 code, about 200 bytes per picture, so a 4.6 GB file with 265,000
pictures costs 73 MB while a 10 s clip costs 21 MB. Copy, convert and normalize do not grow with length. The result of the
big trim was checked: `co64` is present, ffprobe reads all 251,750 pictures, a decode of the last 5 seconds is clean.

Encodes keep their frame buffers in the helper, so they are bounded by resolution, not duration:

| operation | peak |
|---|---:|
| compress, 1080p, hardware HEVC | 214 MB |
| compress, 1080p, `--codec av1` (SVT-AV1 preset 8, two-pass) | 1.3 GB |
| HDR to SDR, 1080p-class, 5 s | 298 MB |

The AV1 figure is above the 300 MB line of the FLOWS table, for the "documents and images" row; the rule for video
is "constant regardless of length", which holds, but 1.3 GB is a lot. It is SVT-AV1's lookahead and thread pools:
`lookahead=8` with `lp=4` measured 822 MB at the same speed, a candidate for a later tuning pass.

## Speed against the ffmpeg command line

Median of 3 runs, both writing a real file, Panoply through the `panoply` binary (process start included). The exact
ffmpeg command of every row is printed by `speed_report` and listed here.

| operation | file | Panoply ms | ffmpeg CLI ms | ratio |
|---|---|---:|---:|---:|
| Convert, container change (copy) | bbb 1080p H.264, 10 s | 175 | 50 | 3.5 |
| Convert, container change (copy), 4K | 4K HEVC, 5 s | 81 | 24 | 3.4 |
| Extract audio (copy) | sintel trailer 480p | 80 | 21 | 3.9 |
| Trim to keyframes (copy) | bbb 1080p | 63 | 51 | 1.2 |
| Trim to the frame (smart cut), against a full re-encode of the clip | bbb 1080p | 1416 | 1135 | 1.25 |
| Compress to 5 MB, against one encode at the same rate | bbb 1080p | 2907 | 1320 | 2.2 |
| Normalize to -16 LUFS, against two-pass `loudnorm` | MP3, 24 s | 352 | 600 | 0.6 |
| Stereo to mono, against `-ac 1` | OGG speech | 521 | 222 | 2.35 |
| HDR to SDR, against a plain 8-bit H.264 encode | HDR10 HEVC | 1440 | 395 | 3.65 |
| Info, 1 hour video | 1 h low-res | 60 | 19 | 3.2 |

```
ffmpeg -y -i bbb-1080p-h264-10s.mp4 -map 0 -c copy ff0.mkv
ffmpeg -y -i 4k-hevc-5s.mp4 -map 0 -c copy -tag:v hvc1 -movflags +faststart ff0.mov
ffmpeg -y -i sintel-trailer-480p.mp4 -vn -c:a copy ff0.m4a
ffmpeg -y -ss 1 -to 9 -i bbb-1080p-h264-10s.mp4 -c copy -movflags +faststart ff0.mp4
ffmpeg -y -ss 1 -to 9 -i bbb-1080p-h264-10s.mp4 -c:v h264_videotoolbox -b:v 8400k -c:a copy -movflags +faststart ff0.mp4
ffmpeg -y -i bbb-1080p-h264-10s.mp4 -c:v h264_videotoolbox -b:v 3900k -an -movflags +faststart ff0.mp4
ffmpeg -y -i recorder-improvisation.mp3 -af loudnorm=I=-16:TP=-1:LRA=11 -c:a libmp3lame -q:a 2 ff0.mp3
ffmpeg -y -i agnew-speech.ogg -ac 1 -c:a libopus -b:a 64k ff0.ogg
ffmpeg -y -i hdr10-hevc-10bit.mp4 -c:v h264_videotoolbox -b:v 4000k -pix_fmt yuv420p -c:a copy ff0.mp4
ffprobe -show_format -show_streams   (1 hour file)
```

How to read it, honestly: the spec's rule is "at least as fast as the native CLI". Panoply meets it where it does real work
(normalize is 1.7 times faster, because the meter is our own at 1,300 times real time and the gain pass is one
encode where `loudnorm` needs two) and is within 25% for trim and smart cut, but it misses on jobs that finish in 20 to
50 ms. The miss is a roughly fixed 60 to 125 ms and not a multiple: Panoply runs a probe before the job (one `ffprobe`
process) and verifies the result after it (a second `ffprobe` process, so that "never reports success on a file that
does not play" is true), and each process start costs 20 to 40 ms on a busy machine. The fix is to read MP4, MOV and
M4A headers with our own box walker for both probes, which the engine already contains for editing; not done in wave 1.
The three other rows differ in kind of work, not in overhead: Compress runs the verify loop (two attempts here), Stereo to
mono listens to the channels before it decides, HDR to SDR does a 65 cube lookup on 16-bit RGB where the baseline is a
plain encode.

## Compress quality at equal size

Method: Panoply `media.compress` is given a target size and its result is scored with VMAF (libvmaf 3, whole clip, against
the source). Every other encoder is then driven to the same output size (within 3%, up to five tries) and scored the same way.
The x264 column is only a yardstick (GPL, not shipped). Panoply may scale the picture down (it is scored at source
resolution, which charges it for the lost pixels); the others keep the source size.

| clip | target | Panoply default | Panoply `--codec av1` | h264_videotoolbox, naive rate | hevc_videotoolbox | SVT-AV1 preset 8 | x264 medium |
|---|---:|---|---|---|---|---|---|
| bbb 1080p H.264 | 3.0 MB | 73.4 HEVC 1280x720 (2.92 MB) | 88.2 AV1 1920x1080 (2.82 MB) | 32.7 (2.99 MB) | 50.8 (3.03 MB) | 89.4 (2.93 MB) | 76.7 (2.93 MB) |
| sintel 720p H.264 | 1.5 MB | 98.6 HEVC 1280x720 (1.46 MB) | 99.1 AV1 1280x720 (1.50 MB) | 98.8 (1.49 MB) | 98.6 (1.49 MB) | 98.9 (1.50 MB) | 97.1 (1.46 MB) |
| bbb 720p HEVC | 2.5 MB | 88.3 HEVC 1280x720 (2.42 MB) | 93.1 AV1 1280x720 (2.20 MB) | 69.8 (2.49 MB) | 77.6 (2.48 MB) | 93.9 (2.42 MB) | 90.8 (2.42 MB) |
| sintel trailer 480p | 1.0 MB | 64.0 HEVC 428x240 (0.99 MB) | 79.9 AV1 854x480 (0.91 MB) | 72.5 (1.01 MB) | 75.9 (1.18 MB) | 89.9 (0.99 MB) | 81.3 (0.98 MB) |

Reading it:

- Against the hardware encoder given a naive bit rate (what a plain ffmpeg or HandBrake hardware preset does), the default
  path is far better on the two hard clips (73.4 against 32.7 and 88.3 against 69.8), because the size formula, the
  floors and the verify loop keep the encoder out of the range where it falls apart. It is level on the easy clip and
  lower on the 1 MB trailer (64.0 against 72.5): at 0.10 Mb/s the floor says 854x480 would look like mush, the module goes to
  428x240, and a full-resolution VMAF punishes the missing pixels more than the mush. Whether a person prefers
  it is not measured here.
- Against software x264 medium at the same size the default hardware path is below on three clips by 2.5 to 17 points
  and above on one by 1.5. This misses the spec's quality bar ("no lower than HandBrake's software equivalents")
  on tight budgets when the hardware encoder is used. That is the price of the hardware ladder: speed over bits.
- `--codec av1` (SVT-AV1 through the same size formula, two-pass) is level with or above x264 on three clips (88.2 against
  76.7, 99.1 against 97.1, 93.1 against 90.8) and below on the trailer (79.9, but it stopped at 0.91 MB, 9% under the
  target; raw SVT-AV1 given the whole 1.0 MB scores 89.9). It costs time: on the 10 s 1080p clip 3.9 s of wall and 39 s of CPU
  against 2.1 s of wall and 6 s of CPU for the hardware path, and 1.3 GB of memory.
- Decision for the wave: the default stays hardware HEVC or H.264, as the spec says ("hardware first"), and the module
  says which encoder ran. The result card should offer "Smaller or sharper, slower" for AV1 when the size target is under the
  hardware knee. The data to decide this automatically is in the tables here and in `floors.toml`.

### Floors (`modules/builtin/media-compress/floors.toml`)

Bits per pixel per frame at which a clip's VMAF crosses 80 (floor) and 90 (knee), median over the four clips, measured on 6 seconds
of each clip with `compress_sweep` (libvmaf `n_subsample=2`). Cells are VMAF with the bits per pixel the encoder actually produced in brackets.

| encoder | floor (VMAF 80) | knee (VMAF 90) |
|---|---:|---:|
| H.264, VideoToolbox | 0.031 | 0.068 |
| HEVC, VideoToolbox | 0.022 | 0.044 |
| AV1, SVT-AV1 preset 8 | 0.008 | 0.020 |
| VP9, libvpx good cpu-used 4 | 0.0125 | 0.030 |
| H.264 software, AV1 hardware | not measured, estimated (H.264 software = H.264 hardware; AV1 hardware = 1.5 times AV1 software) | |

H.264, VideoToolbox:

| clip | 0.008 | 0.012 | 0.018 | 0.027 | 0.04 | 0.06 | 0.09 | 0.135 | 0.2 |
|---|---|---|---|---|---|---|---|---|---|
| bbb 1080p | 40.4 (0.011) | 54.4 (0.013) | 65.4 (0.020) | 73.6 (0.029) | 79.7 (0.042) | 84.7 (0.062) | 88.7 (0.093) | 91.7 (0.139) | 93.8 (0.206) |
| sintel 720p | 79.9 (0.018) | 82.8 (0.018) | 89.3 (0.021) | 95.1 (0.029) | 97.2 (0.042) | 98.1 (0.063) | 98.3 (0.092) | 98.5 (0.137) | 98.6 (0.200) |
| bbb 720p HEVC source | 34.9 (0.014) | 43.3 (0.015) | 55.7 (0.020) | 66.2 (0.029) | 73.9 (0.043) | 80.3 (0.064) | 85.2 (0.095) | 89.0 (0.140) | 91.9 (0.208) |
| sintel trailer 480p | 71.6 (0.009) | 80.7 (0.013) | 85.8 (0.016) | 90.6 (0.026) | 93.1 (0.039) | 94.9 (0.057) | 95.8 (0.085) | 96.4 (0.123) | 96.8 (0.170) |

HEVC, VideoToolbox:

| clip | 0.008 | 0.012 | 0.018 | 0.027 | 0.04 | 0.06 | 0.09 | 0.135 | 0.2 |
|---|---|---|---|---|---|---|---|---|---|
| bbb 1080p | 61.5 (0.009) | 71.1 (0.013) | 78.7 (0.019) | 83.9 (0.029) | 87.3 (0.042) | 90.4 (0.062) | 92.8 (0.093) | 94.8 (0.138) | 95.8 (0.205) |
| sintel 720p | 80.1 (0.022) | 80.1 (0.022) | 84.2 (0.024) | 91.5 (0.030) | 96.8 (0.042) | 97.9 (0.061) | 98.1 (0.089) | 98.3 (0.127) | 98.4 (0.188) |
| bbb 720p HEVC source | 49.3 (0.010) | 60.0 (0.013) | 70.2 (0.020) | 76.8 (0.029) | 82.6 (0.042) | 87.0 (0.063) | 90.4 (0.094) | 92.6 (0.138) | 94.3 (0.201) |
| sintel trailer 480p | 72.6 (0.016) | 75.4 (0.016) | 82.6 (0.019) | 89.8 (0.026) | 92.7 (0.037) | 94.6 (0.052) | 95.9 (0.076) | 96.5 (0.113) | 96.9 (0.162) |

AV1, SVT-AV1 preset 8:

| clip | 0.008 | 0.012 | 0.018 | 0.027 | 0.04 | 0.06 | 0.09 | 0.135 | 0.2 |
|---|---|---|---|---|---|---|---|---|---|
| bbb 1080p | 79.6 (0.008) | 84.3 (0.011) | 87.4 (0.017) | 89.8 (0.025) | 91.4 (0.036) | 92.7 (0.053) | 93.7 (0.077) | 94.6 (0.120) | 95.5 (0.188) |
| sintel 720p | 77.5 (0.007) | 84.4 (0.010) | 91.5 (0.014) | 95.6 (0.022) | 97.1 (0.030) | 97.8 (0.044) | 98.1 (0.068) | 98.4 (0.101) | 98.6 (0.153) |
| bbb 720p HEVC source | 69.9 (0.008) | 73.2 (0.009) | 82.4 (0.017) | 87.1 (0.026) | 89.8 (0.038) | 92.1 (0.054) | 93.9 (0.079) | 95.1 (0.121) | 95.9 (0.175) |
| sintel trailer 480p | 81.0 (0.004) | 85.1 (0.006) | 87.2 (0.008) | 89.7 (0.011) | 92.5 (0.018) | 93.7 (0.030) | 95.0 (0.053) | 95.3 (0.075) | 95.9 (0.095) |

VP9, libvpx good cpu-used 4:

| clip | 0.008 | 0.012 | 0.018 | 0.027 | 0.04 | 0.06 | 0.09 | 0.135 | 0.2 |
|---|---|---|---|---|---|---|---|---|---|
| bbb 1080p | 75.2 (0.011) | 81.1 (0.017) | 85.1 (0.024) | 88.3 (0.034) | 90.8 (0.049) | 92.7 (0.071) | 94.1 (0.102) | 95.2 (0.148) | 96.0 (0.214) |
| sintel 720p | 75.3 (0.008) | 81.8 (0.010) | 88.7 (0.015) | 94.1 (0.021) | 97.0 (0.031) | 98.0 (0.045) | 98.3 (0.066) | 98.4 (0.098) | 98.5 (0.141) |
| bbb 720p HEVC source | 68.4 (0.013) | 76.4 (0.019) | 82.6 (0.028) | 87.5 (0.043) | 90.8 (0.066) | 93.2 (0.101) | 94.6 (0.149) | 95.7 (0.206) | 96.4 (0.274) |
| sintel trailer 480p | 81.0 (0.008) | 85.6 (0.011) | 89.5 (0.016) | 92.1 (0.023) | 93.8 (0.033) | 95.2 (0.047) | 96.1 (0.069) | 96.7 (0.099) | 97.2 (0.141) |

Hardware encoders miss their asked rate at the low end (the VideoToolbox H.264 encoder returned 0.011 when 0.008 was
asked, and sintel 720p H.264 shows 0.018 for both 0.008 and 0.012). This is why Compress verifies and retries, and why it
gives the encoder a smaller picture when asking for less changes nothing.

### Size targets met

Compress to 2 MB over the 19 default-tier video files: 15 outputs written, every one at or under the limit (1.7 to 2.0 MB), 3 inputs
were already under it and were left alone (said so on the card), and the broken file errored with the Repair pointer. The rotated-90 phone clip used to
fail (the hardware HEVC encoder will not go below about 2.3 MB for it at 1080p); the loop now notices that asking for
less does not make the file smaller, scales the picture down one rung and lands at 1.97 MB. The 1 MB target for the 52 s trailer needs
the same step and ends under the limit in 3 attempts (before the loop was widened to 6 attempts with a harder second
correction it ended 1% over and refused).

## Smart cut

`cut_report` cuts clips on frames that are not keyframes and measures what the output kept. A keyframe cut is a pure copy, so it
is not listed (it is bit-identical, checked by packet hash in `tests/trim.rs`). SSIM is against the same range of the source.

| clip | range (s) | wall ms | video packets copied untouched | SSIM mean | SSIM worst frame |
|---|---|---:|---:|---:|---:|
| bbb 1080p H.264, 60 fps | 1 to 9 | 1697 | 249 of 480 (52%) | 0.9806 | 0.9468 |
| bbb 1080p H.264, 60 fps | 2.5 to 3.3 | 1023 | 0 of 48 (0%) | 0.9591 | 0.9482 |
| sintel 720p H.264 | 1.3 to 8.7 | 747 | 0 of 177 (0%) | 0.9948 | 0.8206 |
| bbb 720p HEVC, B-frames | 1.2 to 8.8 | 1273 | 0 of 228 (0%) | 0.9705 | 0.9420 |
| sintel trailer 480p, with sound | 5 to 45 | 1034 | 797 of 960 (83%) | 0.9993 | 0.9882 |
| moov at the end, with sound | 3 to 17 | 1494 | 249 of 420 (59%) | 0.9974 | 0.9875 |

How much is copied depends on the source's group of pictures. The 1080p clip has a keyframe every 4.2 s, so a cut from 1 to 9 copies the
middle group and re-encodes the 3.2 s before it and the 0.7 s after. The 720p clips have one keyframe in 10 s, so a cut inside
that group re-encodes the whole range (no way around that: a picture needs its predecessors). The audio is always copied, frame exact. Edge pictures are
re-encoded by the hardware encoder at the source's bit rate; the worst frame of 0.82 on sintel is a fade where the encoder at the source rate cannot keep up.
Each splice is decode-checked before it is accepted and falls back to a keyframe snap (said on the card) if it fails; none fell back here.

## HDR to SDR

One pass: a 65-cube lookup table made by our own code (PQ or HLG decode, BT.2390 tone mapping, BT.2020 to BT.709 with a soft knee,
BT.709 transfer), applied in ffmpeg's `lut3d`, tagged BT.709 on output. `tests/hdr.rs` builds clips of 12 known color patches in RGB,
encodes them as PQ and as HLG HEVC 10-bit, runs the module and compares each decoded patch pixel to our own mapping function:

| transfer | worst error over 12 patches, 3 channels |
|---|---|
| PQ (smpte2084) | 3 code values of 255 |
| HLG (arib-std-b67) | 4 code values of 255 |

That checks the pipeline (the lookup is applied as designed through 4:2:0, a lossy encode and 8-bit rounding); it does not say the curve is the one
a reviewer would pick. Dolby Vision profile 8.4 is treated as HLG, profile 5 is refused in plain words, and the sound and the rotation flag are kept.
The mapping is static (peak from the file's metadata), not scene by scene.

## Loudness

Our own BS.1770-4 meter against ffmpeg's `ebur128` filter as an independent reader, after `media.normalize` to -16 LUFS (the module measures its own result
and corrects once more if it is more than 0.2 LU away):

| file | before | after |
|---|---|---|
| recorder-improvisation.mp3 | -9.8 LUFS, -1.8 dBTP | -16.3 LUFS, -8.3 dBTP |
| stay-down-here-1915.mp3 | -18.0 LUFS, 0.1 dBTP | -16.0 LUFS, -0.9 dBTP |
| agnew-speech.ogg | -7.6 LUFS, 1.5 dBTP | -16.0 LUFS, -7.2 dBTP |
| netaji-1943.ogg | -17.9 LUFS, -3.2 dBTP | -16.0 LUFS, -1.2 dBTP |
| glockenweihe.flac | -22.1 LUFS, -5.5 dBTP | -16.1 LUFS, -1.0 dBTP |
| lachmoeweisma.flac | -18.8 LUFS, -1.0 dBTP | -16.1 LUFS, -1.0 dBTP |
| aac-lc-stereo.m4a | -8.7 LUFS, -3.0 dBTP | -16.0 LUFS, -10.4 dBTP |
| clipped-overdriven.wav | -1.9 LUFS, 0.1 dBTP | -16.0 LUFS, -14.0 dBTP |
| stereo-lr-tones.flac | -6.5 LUFS, -6.0 dBTP | -16.0 LUFS, -15.6 dBTP |

Every result is within 0.3 LU of the target (the MP3 is the worst, from re-encoding) and under the true-peak ceiling of -1 dBTP; where gain would have pushed peaks over the
ceiling (the 1915 recording, the two flac files) a limiter held them at -0.9 to -1.0. The meter runs at 1,318 times real time
(a 3 hour file in 8 s) and `tests/audio_ops.rs` asserts the half-LU and ceiling guarantees, the EBU preset (-23 LUFS, -1 dBTP) and that
ReplayGain tags change no sample. The meter is integrated BS.1770 loudness, not dialogue gated (see open items).

## Repair

`repair_report` breaks corpus files four ways and counts the pictures that come out against the pictures that were really in the broken file's bytes
(counted from the intact file's index). A result counts only if it decodes without errors. The baseline is `ffmpeg -i broken -c copy`;
`untrunc` is not installed here, so it is not compared. "Reference" is a working file from the same camera or app, which is what Repair asks for.

| file | damage | pictures in the file | Panoply | ffmpeg `-c copy` |
|---|---|---:|---|---|
| sintel trailer 480p (video + audio) | cut off at 10%, 50%, 90% (index kept) | 133, 574, 940 | 132, 573, 938 (99 to 100%) | 133, 574, 940 |
| | index stripped | 1253 | 1253 (100%) | cannot open |
| | recorder crashed at 70%, no index | 724 | 723 (100%) | cannot open |
| phone portrait VFR (video + audio) | cut off at 10%, 50%, 90% | 35, 168, 288 | 34, 166, 287 (97 to 100%) | 35, 168, 288 |
| | index stripped | 319 | 318 (100%) | cannot open |
| | crashed at 70% | 226 | 222 (98%) | cannot open |
| bbb 720p HEVC (video only, B-frames) | cut off at 10%, 50%, 90% | 19, 150, 259 | 19, 150, 259 (100%) | 19, 150, 259 |
| | index stripped | 300 | 300 (100%, bit-exact frames in display order) | cannot open |
| | crashed at 70% | 210 | 210 (100%) | cannot open |
| moov at end, 20 s (video + audio) | cut off at 10%, 50%, 90% | 59, 299, 542 | 59, 299, 541 (100%) | 59, 299, 542 |
| | index stripped | 600 | 599 (100%) | cannot open |
| | crashed at 70% | 424 | 423 (100%) | cannot open |

Where the index is intact ffmpeg is as good (it reads the index), so the point of the module is the other rows: a file with no index at all, which no
player opens, comes back with every picture. A file cut off with its index keeps everything that arrived, and Panoply tells the person which part is
missing; the last picture is dropped when it is cut in half (one fewer than ffmpeg on three rows), because a half picture does not decode cleanly. Sound
cannot be rebuilt from a file with no index (AAC frames carry no length), so a rebuilt file has the picture only and the card says why there is no audio.

## Findings and fixes from this round

- **Repair swallowed pictures in files that interleave sound** (found by `repair_report`: sintel trailer, index stripped, recovered 538 of 1253 with
  decode errors). AAC frames of a few hundred bytes happen to look like a NAL unit with a plausible length, and the scanner accepted a
  "picture" of 27 KB that started inside a sound frame and swallowed the real pictures behind it. Fix: for H.264 the first slice's
  `frame_num` must follow the previous picture's (`frame_num` is `prev_ref + 1` or the same as the last, 0 at an IDR picture; streams that
  allow gaps are exempt), checked for each candidate and for each hop of the confirmation chain, with a fallback search without the check for streams
  that reset their numbering. Result: 1253 of 1253 and 723 of 724, and a regression test (`pictures_between_stretches_of_sound_are_not_confused_with_sound`).
- **Compress gave up 1% over the limit** (sintel trailer to 1 MB). The loop now has 6 attempts, a second consecutive miss leaves extra
  room, and the smaller picture step happens as the third attempt from the end.
- **AAC at 96 kHz failed** in the AudioToolbox encoder: it is resampled to 48 kHz with a note on the card; a test covers it.
- **The size parser treats a bare number as megabytes** (`--under 3000000` is 3,000,000 MB and says "already under the 3000.00 GB", correctly but
  unhelpfully). `quality_report` now passes `3000000b`. Worth a guard in the manifest ("did you mean bytes?").
- Earlier in the build (all fixed, all with tests): a junction decode check that failed on open groups of pictures (splice points are now restricted to
  IDR and BLA pictures), a `moov` stsd with unused entries that broke HEVC decode, a false "spatial video" flag found by a byte search, a host guard that blocked reading
  the module's own output (engine reads of own output use `probe_trusted`), `has_filter` that was always false because filter flags are two
  characters, a libvmaf JSON parse that picked the per-frame score instead of the pooled one, an ffmpeg raw H.264 remux that dropped the first
  frame and B-frame timing (now an own picture-order-count rebuild), libvorbis missing in the Homebrew build (Ogg targets use Opus with a note).

## Open items and limits

- **The macOS LGPL helper is not built.** The recipe (`engine/build-macos.sh`, `deps.sh`, `macos-ffmpeg.yml.example`) is not run; until it is, macOS
  uses an ffmpeg on `PATH`. Needs one Actions run and a pinned checksum.
- **Windows and Linux ran nothing.** `cargo check --all-targets` passes for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`; the Media Foundation,
  NVENC, QSV, AMF and VA-API argument sets are unit tested as text only. The BtbN pins and their checksums are recorded, the fetch itself is untested.
- **Fixed cost of about 60 to 125 ms per job** against the ffmpeg CLI on tiny jobs (two `ffprobe` processes). Native MP4 probing for the common case would remove most of it.
- **Hardware H.264 and HEVC lose to x264 at tight sizes** (table above). A tight target could pick SVT-AV1 automatically for a person's own device and say why;
  not done because it costs 10 times the CPU and 1.3 GB, and the spec says hardware first.
- **SVT-AV1 memory** is 1.3 GB at 1080p. `lookahead=8:lp=4` measured 822 MB; not adopted without a speed and VMAF run of its own.
- **OpenH264 from Cisco** is not fetched, so a machine with no hardware H.264 encoder and no ffmpeg of its own cannot make H.264.
- **Estimated floors**: `h264_software` and `av1_hardware` have no measurement (no way to run them on this Mac).
- **Loudness is integrated, not dialogue gated**: ATSC A/85 and Netflix presets say so on the card.
- **Smart cut** covers H.264 and HEVC in MP4 and MOV at IDR or BLA splice points; open groups of pictures, Dolby Vision, spatial video and other codecs snap to a keyframe.
- **HDR mapping is static**; profile 5 Dolby Vision is refused (needs libplacebo with Vulkan).
- **Repair of sound**: no index means no sound rebuild; zeroed bytes in the middle of an index-complete file are copied, not dropped.
- `cargo deny check` over the whole workspace fails on advisories that come from the desktop app's GPUI dependencies (not from this crate); `cargo deny` run on `panoply-media` and `panoply-cli` passes all four checks.
