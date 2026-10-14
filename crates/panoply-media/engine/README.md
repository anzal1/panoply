# The helper engine builds

Panoply runs an LGPL build of ffmpeg and ffprobe as a separate process (why: see the module doc of
`src/engine/mod.rs` and `../README.md`). Nothing in this repository is a binary.

| Platform | Where the build comes from | Pinned in |
|---|---|---|
| Windows x64, arm64 | BtbN FFmpeg-Builds, `lgpl` variant, tag `autobuild-2026-09-30-13-08`, FFmpeg n9.0.2 | `src/engine/pins.rs` |
| Linux x64, arm64 | the same tag, `linux64-lgpl` and `linuxarm64-lgpl` | `src/engine/pins.rs` |
| macOS arm64, x64 | **none exists**: every prebuilt macOS ffmpeg that was found is `--enable-gpl`. Built from source by `macos-ffmpeg.yml.example` and published as a Panoply release, then pinned | not yet (see below) |

BtbN keeps the last build of each month for two years, which is why a month-end tag can be pinned and a daily
tag cannot. That is a README promise, not a contract, so the four archives should also be mirrored into a Panoply
release; the pin table then points at the mirror and keeps the BtbN URL as the documented origin.

Until the macOS archives are pinned, a Mac uses the `ffmpeg` and `ffprobe` it finds on `PATH` (or
`PANOPLY_FFMPEG`). Panoply only ever asks such a build for encoders that are in the LGPL set
(VideoToolbox, SVT-AV1, libvpx, libopus, libmp3lame, native AAC, AudioToolbox), so a GPL ffmpeg on the user's
own machine gives the same results, but Panoply never fetches or ships one.

## Building the macOS engine

`build-macos.sh` and `deps.sh` (the static dependencies) are a written, **not yet executed** recipe: a build like this is an Actions job and was not run on a developer's Mac. The first Actions run will say which version numbers need adjusting, and the checksums go into `SHA256SUMS` after review. `build-macos.sh` configures ffmpeg with `--disable-gpl --disable-nonfree --enable-version3` and the libraries
listed in its header (all permissive or LGPL), strips and ad-hoc signs the result and writes
`ffmpeg-<version>-macos-<arch>-lgpl.tar.xz` and its `sha256`. `macos-ffmpeg.yml.example` runs it on GitHub
Actions for `macos-15` (arm64) and `macos-15-intel` (x86_64), uploads the archives, the source tarballs, the
build script and the configure line to a release (LGPL section 6: static linking makes the relink material an
obligation), and prints the pin to paste into `pins.rs`.

`--disable-autodetect` also turns off VideoToolbox and AudioToolbox, so both are enabled explicitly.

## OpenH264

OpenH264 (BSD-2-Clause) is the permissive software H.264 encoder, and Cisco's patent grant applies only to
the binary Cisco publishes, downloaded by the end user onto their own device (never mirrored, with the line
"OpenH264 Video Codec provided by Cisco Systems, Inc." where the user controls it). BtbN's builds compile their
own copy and link it in, which carries no such grant, so Panoply does not use a pinned build's OpenH264. An H.264
encode therefore needs a hardware encoder (VideoToolbox, Media Foundation, NVENC, QSV, AMF, VA-API), or an
ffmpeg of the user's own that has OpenH264, or the output goes to AV1 or VP9. Fetching Cisco's binary needs an
ffmpeg variant linked against its shared library; that variant is not built yet (see `../README.md`, "Not done").
