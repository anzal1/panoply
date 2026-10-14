# Panoply

Every utility you have ever paid for, downloaded once, or uploaded a file to a
stranger's website for, in one open-source app. It runs on your own device,
opens from a shortcut, shares files with your other devices over your own
network, and anyone can add a new tool to it.

Built by the people, for the people.

> **Status: v0.1.0 beta.** 30 tools ship for macOS, Windows and Linux: PDF, ID and exam photos, images,
> image AI, OCR, video and audio. macOS is the most tested system. The rest of the plan (about 270 modules) is specced and comes in later
> releases. Read [Known limitations](docs/KNOWN-LIMITATIONS.md) before you start and the
> [changelog](CHANGELOG.md) for what is new.

## Quickstart

### Install

Download the file for your system from the [releases page](https://github.com/anzal1/panoply/releases).
The beta is not code signed, so your system asks you to confirm the first start. The steps are below.
Check a download with `SHA256SUMS` from the same page.

**macOS (Apple silicon and Intel).** Open `Panoply-<version>-macos-universal.dmg` and drag Panoply to
Applications. Then:

1. Double-click Panoply. macOS says it cannot check the app for malware. Click Done.
2. Open System Settings, then Privacy and Security. Scroll down to the Security section.
3. Next to "Panoply was blocked to protect your Mac", click Open Anyway. Confirm with your password or
   Touch ID, then click Open Anyway once more.

You do this once. On macOS 14 and earlier you can also right-click Panoply and choose Open. Videos need
`ffmpeg` on your Mac: `brew install ffmpeg`.

**Windows 10 and 11 (x64 and arm64).** Run `Panoply-<version>-windows-<arch>.msi`, or unzip the `.zip` and run
`panoply-desktop.exe`. Windows SmartScreen may show "Windows protected your PC". Click More info, then Run
anyway. The installer offers an Explorer right-click entry, "Open with Panoply", and puts `panoply` on your PATH.

**Linux (x64 and arm64).** Download the AppImage, then:

```bash
chmod +x Panoply-<version>-linux-x64.AppImage
./Panoply-<version>-linux-x64.AppImage
```

If it does not start, run it with `--appimage-extract-and-run`. The `.tar.gz` holds the same files and an
`install.sh`: run it for `~/.local`, or `sudo ./install.sh /usr/local` for everyone.

### First run

1. Start Panoply. The app window opens. A small command bar is created hidden in the background.
2. Press the hotkey from anywhere: **Option+Space** on macOS, **Ctrl+Alt+Space** on Windows and Linux. Set your
   own with `PANOPLY_HOTKEY="ctrl+alt+p"`. On Wayland there is no global hotkey yet: use the app window or bind
   a key in your desktop settings to `panoply-desktop`.
3. Drop a file on the bar, or type what you want: "compress pdf", "make this under 50 kb", "remove background".
4. Pick a tool, set the one or two options it shows, press Return. You get a new file next to the original.
   Originals are never changed. Undo removes the new file again.

The first time a tool needs an engine or a model it downloads it once, shows the size first, checks a pinned
SHA-256 and keeps it in your cache folder. After that it works offline. Details:
[docs/PRIVACY.md](docs/PRIVACY.md).

### What works in the beta

- **PDF:** merge, split, organize pages (reorder, delete, rotate, extract), compress to a quality or to a
  size, images to PDF, PDF to images, OCR to a searchable PDF.
- **ID and forms:** passport and visa photos, print sheets, signature cleaner, resize a photo to an exact
  size in KB with 15 exam presets.
- **Images:** convert (JPG, PNG, WebP, AVIF, TIFF, BMP, GIF, and HEIC on macOS), compress, resize, strip
  metadata, image to text.
- **Image AI:** remove background, erase an object, remove a watermark from your own pictures, upscale.
- **Video and audio:** convert, trim, compress to a size or an upload limit, extract audio, normalize
  loudness, stereo to mono, HDR to SDR, repair, inspect, edit streams.
- **Command line:** every tool is a `panoply` command, for example `panoply run image.compress photo.jpg`.
  `panoply modules` lists them all. `panoply share` sends and receives files with LocalSend apps on your
  network.
- **Community modules:** a module is a manifest plus a WebAssembly file. Drop one in the modules folder of
  the app data directory and it shows up in the bar.

What is not in the beta yet, and what differs per system, is in
[docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md).

## Where the project is going

The beta ships the tools listed above. The sections below are the plan, with the rules every tool is built to.

## The four rules

1. **Blazing fast.** Native code, hardware encoders, streaming. Every
   interaction lands within one frame.
2. **Bang on utility.** Each tool beats the best paid or web version on a
   measured quality bar, with defaults good enough that most people never open
   the options.
3. **Lowest possible memory.** Under 100 MB idle, nothing running in the
   background, models loaded only during a job.
4. **Butter smooth.** 120 fps everywhere, nothing on the UI thread that can
   stutter.

Your files stay on your device. No accounts, no limits, no watermarks.

## The full plan

- **PDF:** merge, split, compress to a size, edit, sign, fill, OCR, redact,
  convert to and from Office and images.
- **ID and forms:** passport and visa photos for any country, exam photo and
  signature sizing to exact KB, document scanning.
- **Images, video and audio:** convert, compress, resize, remove background,
  erase objects, upscale, trim, transcribe, subtitles, noise removal.
- **Universal converter:** 214 formats, 9,214 conversions, routed automatically.
- **Find anything:** instant file search, text inside screenshots, photos by
  meaning, search by metadata.
- **Share:** send files between your phone and computers on your network,
  compatible with LocalSend.
- **Everyday:** clipboard history, snippets, calculators, developer tools,
  privacy tools.
- **For AI agents:** every module is also a CLI command and an MCP tool.

Full catalogue: [docs/MODULES.md](docs/MODULES.md).

## Documentation

| Read this | For |
|---|---|
| [VISION.md](docs/VISION.md) | What Panoply is and why |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | How it is built, start here as a contributor |
| [FLOWS.md](docs/FLOWS.md) | The nine module flow patterns, shared capabilities, budgets, spec template |
| [MODULES.md](docs/MODULES.md) | Every module, ranked top 30 by demand |
| [CONVERSIONS.md](docs/CONVERSIONS.md) | All formats and conversion bridges (generated) |
| [specs/](docs/specs/) | Full spec for every module |
| [DECISIONS.md](docs/DECISIONS.md) and [adr/](docs/adr/) | Decisions and their reasons |
| [PLAN.md](docs/PLAN.md) | Build phases and gates |
| [WIREFRAMES.md](docs/WIREFRAMES.md) | The nine key screens |
| [KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md) | What the beta does not do yet, and platform gaps |
| [PRIVACY.md](docs/PRIVACY.md) | What stays on your device and what is downloaded |
| [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) | Licenses and credits for everything Panoply is built on |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each release |
| [research/](docs/research/) | Market, launch and model research behind the decisions |

## Repository layout

```
crates/        Rust core: engines (pdf, image, id, ocr, media, vision, share), host, convert, cli, mcp, ffi
conversions/   formats.py, the source of truth for the conversion graph
modules/       module manifests (built-in tools, examples to start)
apps/desktop/  the GPUI desktop app for macOS, Windows and Linux
packaging/     release scripts and package manifests
tools/         maintenance scripts (third-party notices)
docs/          everything above
prototypes/    kill-test code behind the decisions, not shipped
```

## Build from source

```bash
cargo build --release -p panoply-cli -p panoply-desktop
cargo run -p panoply-cli -- modules
cargo run -p panoply-cli -- route docx jpg
cargo test --workspace
```

Linux needs the system libraries listed in [apps/desktop/packaging/README.md](apps/desktop/packaging/README.md).
Regenerate the third-party notices after any `Cargo.lock` change:

```bash
cargo fetch --locked && python3 -I tools/notices/generate.py
```

Regenerate the conversion graph after editing `conversions/formats.py`:

```bash
python3 conversions/formats.py
```

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md). The short version: every module
must meet the budgets, beat its quality bar, use only permissive licenses, and
never copy code from tools we study.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option.
