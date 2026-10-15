# Changelog

All notable changes to Panoply are written here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow semantic versioning, with `-beta`
marking releases that are still finding their edges.

## v0.1.2-beta (2026-10-15)

The first release checked on real Windows and Linux desktops before shipping.

### Fixed

- **Linux: the app now opens a window.** The v0.1.0 and v0.1.1 Linux builds started without X11 or Wayland
  support, so nothing appeared on screen. The command bar now stays hidden until the hotkey shows it,
  Escape hides it, and the app follows system appearance changes instead of polling them.
- **Undo never deletes for good.** It moves the result to the Recycle Bin on Windows, the trash on Linux and
  the Trash on macOS. If a drive has no bin, the file stays where it is and the result card says so.
- **Windows: a tray icon while the app runs in the background.** Closing the window used to leave the
  process running with nothing to see. The fan now sits in the notification area with Open Panoply, Open
  command bar and Quit.
- **Windows: the command bar opens centred in the upper part of the display under the pointer**, not at a
  fixed spot near the top left.
- **Small screens:** the first window fits displays such as 1024 by 768 instead of running off the edge.

### Changed

- **Icon:** the fan opens narrower, so all three tool cards read at every size.

### Known

- On Linux with a compositing window manager, pressing the hotkey to hide the command bar can bring it
  straight back. Press Escape to hide it instead. Fixed in the next release.

## v0.1.1-beta (2026-10-14)

A new look, and fixes for Windows and Linux.

### Changed

- **New icon.** Three tool cards fanned from one pivot, drawn on the macOS 26 icon grid with hand-tuned small
  sizes, plus Windows and Linux sets.
- **Desktop app redesign.** One design system across every screen: a single type ramp, an 8 pt spacing
  grid, quieter selection, visible focus rings, a full menu bar with standard shortcuts, a settings window,
  and a window that remembers its size and place. Choices that do not fit one row open as a pop-up menu, and
  every hint fits on one line.

### Fixed

- **Windows.** Reading text from images no longer crashes when worker threads come and go. A QR share no
  longer ends before the other device has the whole file.
- **Passport photo.** Head tilt is measured on a level, face-sized crop, so turned heads straighten properly
  on Windows and Linux too, within 2 degrees.
- **Tool names.** Summaries that the command bar and panel header cut off are shorter, and the AI image
  tools say up front that they download a model once, after you say yes.
- **Security.** rustls updated to clear four advisories.

## v0.1.0-beta (2026-10-14)

The first public beta. 31 tools, one desktop app for macOS, Windows and Linux, and the same tools as a
command line. Builds are not code signed. See [KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md) for the
first-open steps and for what is not here yet.

### Added

- **Desktop app.** One GPUI app on the Rust core. A command bar opens from a global hotkey (Option+Space on
  macOS, Ctrl+Alt+Space on Windows and Linux). It searches the tools, takes dropped files and understands
  plain requests such as "make this under 50 kb". The app window has categories, recent jobs and history.
  Tool panels are drawn from each tool's manifest, run on a worker thread, show progress, cancel at once,
  and end in a result card with before and after numbers, Show in folder, Open and Undo.
- **PDF.** Merge, split, organize pages, compress to a quality or to a size, images to PDF, PDF to images,
  OCR to a searchable PDF. Large files stream: a 300 MB file compresses in about 5 seconds and 11 MB of
  memory.
- **ID and forms.** Passport and visa photo, print sheet, signature cleaner, and resize to an exact size in KB
  with 15 exam presets that each carry their source link and date.
- **Images.** Convert, compress to a quality or a size, resize, strip metadata, and image to text.
  Conversions keep orientation and color profiles. Nothing is overwritten.
- **Image AI.** Remove background (Apple Vision on macOS, IS-Net elsewhere), erase an object (MI-GAN), remove
  a watermark from your own pictures, upscale 4x (Real-ESRGAN). Models download once, on first use, and
  unload after 60 seconds idle.
- **OCR.** Apple Vision on macOS, PP-OCRv5 models for English, Latin and Devanagari scripts, Tesseract for
  Bengali where it is installed.
- **Video and audio.** Convert, trim, compress to a size or an upload limit, extract audio, normalize
  loudness, stereo to mono, HDR to SDR, repair, inspect and edit streams. Stream copy is used whenever it can
  be, so most trims and conversions do not re-encode.
- **Sharing.** `panoply share` sends and receives files with LocalSend apps over your own network, and makes a
  QR link for devices without an app.
- **Command line.** `panoply` runs every tool: `panoply run image.compress photo.jpg`. `panoply modules`
  lists them. `panoply route docx jpg` shows how one format reaches another through the conversion graph.
- **Community modules.** A module is a manifest and a WebAssembly file that runs in a sandbox. Two examples
  are in `modules/examples-wasm`.
- **Third-party notices.** `THIRD-PARTY-NOTICES.md` lists every Rust dependency with its license text, the
  vendored GPUI input code, and each engine and model that is downloaded at run time with its source and
  license. It ships inside the app and the CLI archives, and CI fails when it is out of date.
- **Downloads are pinned.** Every engine and model download names a release or commit and is checked against a
  SHA-256 before it is used.

### Changed

- Dependency advisories: `instant` and `rustls-pemfile` are gone from the build, and the two advisories that
  come from GPUI (`rustybuzz`, `ttf-parser`) are documented in `deny.toml` with the reason and the date to
  look again. The advisory check now runs as its own CI job.

### Known issues

The full list is in [docs/KNOWN-LIMITATIONS.md](docs/KNOWN-LIMITATIONS.md). The short version:

- The macOS and Windows builds are unsigned. You confirm the first start once.
- HEIC and several camera formats open on macOS only for now.
- Video tools need an `ffmpeg` on macOS. Windows and Linux fetch one on first use.
- The MCP server is not shipped yet. The tool definitions exist, the transport does not.
- Windows and Linux have had less hands-on testing than macOS.
