# Panoply build plan

> **Compressed 2026-10-14: ship fast.** Public beta on macOS, Windows and Linux
> around 28 October 2026, built in parallel: PDF and image engines (done),
> media engine, ID and forms, background removal and erase, OCR, LocalSend
> sharing, the GPUI desktop app and the release pipeline. The rest of the top
> 30 (PDF to Word, edit text, redact, transcribe, text to speech, find,
> clipboard) follows in weekly updates. The phases below are the full scope,
> no longer a serial schedule.

Built from VISION.md, FLOWS.md, MODULES.md, DECISIONS.md and specs/. Weeks are
rough. Every phase ends on a gate: a measured check that must pass before the
next phase starts. Philosophy gates apply throughout: blazing fast, bang on
utility, lowest memory, butter smooth.

## Phase 0: Foundations and kill tests (weeks 1 to 3)

**Build**

- Rust workspace:
  - `panoply-core`: file sniffing (bytes, not extensions), job pipeline,
    streaming I/O, budgets enforcement, cancellation, history.
  - `panoply-host`: module manifest v0 (inputs, options, outputs), WASM
    sandbox (wasmtime on desktop and Android, interpreter on iOS),
    capability grants (files you choose, no network by default).
  - `panoply-convert`: the conversion graph, generated from
    `conversions/formats.py` into a Rust table, shortest-route planner.
  - `panoply-cli` and `panoply-mcp`: every module as a command and an MCP tool
    from day one, generated from the manifest.
  - `panoply-ffi`: UniFFI bindings to Swift and Kotlin.
- CI from the first commit:
  - **Budget harness:** idle CPU, module start, first preview, UI-thread time,
    peak memory per job, measured on every module.
  - **Quality corpora:** 100 real PDFs, 200 images (HEIC with depth, CMYK,
    16-bit, panoramas), 50 videos (VFR, HDR, rotated, 10-bit), exam photo
    samples. Round-trip tests for every pair in CONVERSIONS.md.
  - **Fuzzing** for every file parser.
  - **License gate:** `cargo-deny` blocks GPL and AGPL crates; a model gate
    blocks weights without a clean license.

**Gate (all must pass)**

1. Module authoring: a new developer, or an agent with only the template,
   ships a working module in under 15 minutes and under 50 lines.
2. Agent value: an agent given 10 everyday file tasks with raw CLI tools fails
   at least 3 on the first try (if it does not, MCP is a bonus, not a pillar).
3. Compress PDF beats iLovePDF's recommended setting on file size at equal
   visual quality across the 100-PDF corpus. If it does not, run REA on the
   benchmark tools before lowering the bar.

## Phase 1: Engines and wave 1 (weeks 3 to 7)

**Build**

- Engines, OS first: pdfium; image pipeline with the pipeline contract
  (orientation, color profiles, HDR gain maps, alpha); media through OS
  hardware encoders plus an LGPL ffmpeg build for demux and stream copy; OCR
  ladder (Apple Vision, then PP-OCRv5 fallback).
- Shared engines built once: OCR, GIF encoder with our own quantizer, SSIM,
  print layout, Office conversion (LibreOffice core as a downloaded helper).
- Wave 1 modules: universal converter, PDF merge, split, organize, rotate,
  compress, images to PDF, PDF to images; image convert, compress, resize;
  video and audio stream-copy tools (convert, trim, extract audio, repair);
  archives; metadata view and strip.

**Release: Panoply CLI and MCP, public alpha (week 6)**

The first public drop is for developers and AI agents: one binary, every wave
1 module, local, MCP server included. It builds the GitHub audience before the
app launches, in the fastest-growing open-source category (agent file tools).

**Gate:** every wave 1 module passes its budget and quality bar in CI on Mac,
Windows and Linux (CLI).

## Phase 2: The desktop app for macOS, Windows and Linux (weeks 5 to 10, overlapping)

**Build**

- GPUI app grown from `prototypes/shell-test/gpui/kit-variant` (measured: 33 MB
  idle, 152 to 190 ms cold start), with the GPUI Kit input plus our grapheme
  patch (upstreamed). One app for all three desktops from day one. Platform
  entry points: Finder right-click and share extension (small Swift targets)
  on macOS, Explorer context menu and Send To on Windows, file manager actions
  (Nautilus, Dolphin) and a .desktop handler on Linux.
- Every module and the app are tested on all three OSes before launch: Linux
  in Docker locally, Windows and Linux on free CI runners.
- Command bar, app window (home, find, history), result cards, chains,
  presets.
- Native renderer that draws any module's declared UI from the shared
  component set, so community modules look first-party.
- Entry points: Finder right-click actions, Shift-drag to convert, notch drop
  tray, drag onto bar or Dock icon.
- Accessibility, Reduce Motion, light and dark, 3x zoom review of every state.

**Gate:** shell budgets hold with all wave 1 modules installed (cold start
under 300 ms, idle under 100 MB, 120 fps).

## Phase 3: Wave 2, the demand-ranked top 30 (weeks 8 to 12)

- ID and forms: passport photo, resize to exact KB with exam presets
  (SSC, UPSC, RRB, Passport Seva, PAN, Bangladesh), signature cleaner,
  document scanner.
- Images: background removal, erase and watermark removal, upscale.
- PDF: OCR, PDF to Word, sign, fill forms, redact.
- Audio and video: compress to a size, transcribe and subtitles, noise
  removal, text to speech.
- Find: one search module (names, content, metadata, screenshots by text).
- Share: LocalSend-compatible send and receive, QR to browser.
- Everyday: QR codes, calculators in the bar, clipboard history and snippets.
- REA studies where the bar is hard: Acrobat edit text and PDF to Word,
  remove.bg edges, Everything search speed.

**Gate:** each top-30 module beats its named competitor on its quality bar,
measured and published.

## Phase 4: Desktop launch on macOS, Windows and Linux (around week 12)

- Desktop app plus CLI and MCP on macOS, Windows and Linux. Channels: GitHub
  releases, Homebrew cask, winget and Scoop, Flatpak (Flathub) and AppImage.
- Signing: Apple Developer account ($99 a year) to notarize on macOS,
  otherwise macOS warns users away. Windows SmartScreen warns on unsigned
  apps until reputation builds; winget and Scoop installs avoid most of it,
  and free signing for open source (SignPath Foundation) can be applied for.
  Linux needs no signing. Everything else stays free.
- Launch format, from the X research:
  - one gesture demo (drop a file on the bar, Shift-drag to convert)
  - "apps you can delete once Panoply is on your Mac", with measured numbers
  - generic upload-distrust pain, no competitor named in our copy
  - endorsements from a few known people over our own posts
  - Show HN with the benchmark tables
- Module registry opens for community pull requests on launch day.

## Phase 5: Every other platform (after launch)

The three desktops launch together (Phase 4). Then mobile, ordered by where
the users are: iLovePDF's biggest audiences are Bangladesh and India, mostly
Android.

1. **Android** (Compose): share sheet, passport and exam photos, scanner,
   LocalSend sharing, Hindi and Bengali.
2. **iOS** (SwiftUI): share sheet, camera flows.

## Running costs

Zero apart from the Apple Developer account at launch. CI must be free:
public repository runners or a free self-hosted runner, never paid minutes. No servers, no hosted
models, models downloaded per module from GitHub releases.
