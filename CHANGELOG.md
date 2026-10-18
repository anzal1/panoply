# Changelog

All notable changes to Panoply are written here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow semantic versioning, with `-beta`
marking releases that are still finding their edges.

## Unreleased

### Added

- **Remove noise.** Takes background noise out of a speech recording or the sound of a video
  (`panoply audio denoise in.wav --strength normal`). It runs RNNoise on your computer, with the model inside
  the program and no download. Strength is light, normal or strong. The picture of a video is copied as it is
  and the sound is encoded once. A 60 second clip takes about half a second. Built for speech: steady noise with
  no voice in it may barely change, and music can be dulled.
- **MCP server.** `panoply mcp` gives AI agents (Claude Code, Claude Desktop, Cursor) every tool, built-in and
  community, over stdio. Files go in as absolute paths, results are new files, and long jobs report progress. Setup
  in docs/MCP.md.

## v0.2.0-beta (2026-10-16)

Find any file as you type, see what is filling your disk, and three community plugins in the box.

### Added

- **Find.** Every file on your disk by name, as you type: a few milliseconds a keystroke on a million files. The
  index builds once in the background and stays current as files change, with no work while idle. Files
  also show up in the command bar under the tools. Enter opens, Cmd+Enter shows it in its folder. Names
  only, and nothing leaves your computer.
- **Disk map.** See what is filling your disk as a map you can click into, coloured by file type. Pick
  files or folders, review them with the total, and move them to the Trash with Undo. System folders such as
  Library, Applications and Windows are locked so they cannot be picked. Copies and hard links are counted
  once. Also on the command line as `panoply disk <path>`.
- **Community plugins.** Markdown to HTML, Format JSON, YAML and TOML, and QR code maker ship in every
  download. Each runs sandboxed with only the files you choose and no network, and shows "Community,
  sandboxed" on the Modules page. Writing your own starts from `modules/template-rust`.
- **Result card.** A thumbnail of what you made, with a Before and After toggle for pictures.

### Changed

- **Honest labels.** Tools that fetch a model or an engine once now say so ("Downloads a model once",
  "Downloads an engine once") with a neutral badge. Your files never leave the computer.
- **Quality floor.** Compressing a JPEG never goes below quality 40 by default, so a smaller file never
  comes at a visibly worse picture.

### Fixed

- **Remove watermark never touches faces,** and no longer mistakes labels or scene text for a watermark.
- **Fixes from testing every tool** the way a person would, on macOS, Windows and Linux.

## v0.1.3-beta (2026-10-16)

Fixes found by running the app on real Windows and Linux desktops.

### Added

- **Three community plugins, built from the module template.** They run in the sandbox, work offline, and
  show up in the app (Modules page, "Community, sandboxed"), in the command bar and in `panoply run`:
  `doc.markdown_to_html` (Markdown to one HTML page that prints to PDF), `data.format` (tidy, convert or
  check JSON, YAML and TOML, with line and column for mistakes) and `image.qr_code` (links or text in a file
  to PNG or SVG QR codes). They live in `modules/examples-wasm`, each with its own README.
- **The plugins ship in the app.** The release workflow builds exactly the modules in `modules/shipped.txt` and
  puts them in the macOS app (`Resources/modules`), the Windows MSI and zip, and the Linux AppImage and
  tarball, in the folder the app's catalog loads from. The learning examples stay out of releases.
  `panoply modules` lists them from a release build too: the command line and the app now share one
  module search path (`panoply_core::modules`).
- `tools/check-modules.sh` and a `modules` CI job: fmt, clippy, unit tests and the WebAssembly build for every
  community module.

### Fixed

- **Windows and Linux: type straight after the hotkey.** The command bar opened without the cursor in the
  search box, so typing did nothing until you clicked it.
- **"Under 50 kb" on a photo saved as PNG.** It used to promise the size and then fail. When PNG cannot get
  that small, Panoply now tries JPG, then WebP, and tells you which it used. If nothing fits, it says so and
  shows the smallest size it reached.
- **Windows at 150% scaling.** The first window now fits the screen, with the close button in view.
- **Linux: the hotkey always hides the command bar,** also with a compositing window manager.

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
- **Community modules.** A module is a manifest and a WebAssembly file that runs in a sandbox. Examples
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
