# Panoply build plan

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

## Phase 2: The desktop app, Mac first (weeks 5 to 10, overlapping)

**Build**

- GPUI app grown from `prototypes/shell-test/gpui/kit-variant` (measured: 33 MB
  idle, 152 to 190 ms cold start), with the GPUI Kit input plus our grapheme
  patch (upstreamed). The same app becomes Windows and Linux later. Small
  native Swift targets for Finder right-click and the share extension.
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

## Phase 4: Mac launch (around week 12)

- Mac app plus CLI and MCP on all desktops, GitHub releases, Homebrew cask.
- Needs before launch: Apple Developer account ($99 a year) to sign and
  notarize, otherwise macOS warns users away. Everything else stays free.
- Launch format, from the X research:
  - one gesture demo (drop a file on the bar, Shift-drag to convert)
  - "apps you can delete once Panoply is on your Mac", with measured numbers
  - generic upload-distrust pain, no competitor named in our copy
  - endorsements from a few known people over our own posts
  - Show HN with the benchmark tables
- Module registry opens for community pull requests on launch day.

## Phase 5: Every other platform (after launch)

Ordered by where the users are. iLovePDF's biggest audiences are Bangladesh
and India, which are mostly Android and Windows.

1. **Android** (Compose): share sheet, passport and exam photos, scanner,
   LocalSend sharing, Hindi and Bengali.
2. **Windows and Linux**: the same GPUI desktop app, plus Explorer right-click.
   Confirm the budgets on Windows hardware first.
3. **iOS** (SwiftUI): share sheet, camera flows.

## Running costs

Zero apart from the Apple Developer account at launch. No servers, no hosted
models, models downloaded per module from GitHub releases.
