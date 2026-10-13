# Panoply

> A panoply is a complete set of everything, originally a warrior's full suit
> of armour. Name chosen 2026-10-13. getpanoply.app was free at that date; the
> GitHub handle `panoply` is taken, so use a suffixed org. SQream sells a data
> warehouse product called Panoply, so run a trademark search before launch.

## The vision

Every utility you have ever paid for, downloaded once, or uploaded a file to a
stranger's website for, in one open-source app. It runs on your own machine,
opens from a shortcut, sends files to any device on your network, and anyone
can add a new tool to it in an afternoon.

Built by the people, for the people.

## Philosophy

Four rules every module, screen and line of code is judged by. When they
conflict with a feature, the feature changes.

1. **Blazing fast.** Native code, hardware encoders, streaming. Every
   interaction lands within one frame, every job beats the tool it replaces.
2. **Bang on utility.** Each tool does its job better than the best paid or
   web version, measured, with defaults so good most people never open the
   options.
3. **Lowest possible memory.** Idle app under 100 MB with nothing running in
   the background. Models and engines load only while a job runs and unload
   after. Files stream instead of loading whole.
4. **Butter smooth.** 120 fps everywhere, nothing on the UI thread that can
   stutter, every state animated calmly, nothing ever snaps or freezes.

## The problem

There are thousands of PDF editors, video clippers, image compressors and file
converters. Free ones, paid ones, subscriptions, lifetime deals. Each does one
thing, each has its own limits, and most of the web ones make you upload your
files to someone else's server.

- iLovePDF alone gets about 325M visits a month. Smallpdf, FreeConvert,
  CloudConvert and Sejda each get 37M to 57M.
- Their paywalls are artificial. Smallpdf allows about 2 tasks a day, Sejda 3,
  CloudConvert 10, plus file size caps and watermarks. A laptop does this work
  for free.
- On desktop, people buy the same utilities one by one for $15 to $39 each, or
  rent a bundle like Setapp for $15 to $23 a month.
- remove.bg's standalone site shuts down on 1 Dec 2026 and folds into Canva.
  Services disappear. Your tools should not.

## What it is

One app, five pillars.

1. **Every utility.** PDF, image, video, audio, documents, OCR, clipboard,
   snippets, screenshots, and whatever the community adds next.
2. **The Raycast of utilities.** A global shortcut opens a command bar. Type
   what you want ("compress pdf", "heic to jpg", "trim video"), drop a file,
   done. The same tools also appear in the right-click menu in Finder and
   Explorer, and in a full app window for longer jobs.
3. **Local sharing built in.** Send files between your phone, laptop and desktop
   over your own network. No cloud, no account.
4. **Built by the people.** Every tool is a module. Adding one is a small folder
   and a pull request. Install only what you use.
5. **Agent ready (a bonus).** Every module is also a command-line tool and an
   MCP tool. Kill test 2 showed capable agents can already do everyday file
   tasks with raw tools, so this makes them faster and more reliable rather
   than enabling them.

## Platforms

| Platform | Role | Entry points |
|---|---|---|
| macOS, Windows, Linux | The full app | Global shortcut command bar, right-click menu, app window, CLI, MCP |
| Android | Full toolbox and sharing | App, share sheet, quick settings tile |
| iOS | Toolbox and sharing within Apple's rules | App, share sheet, Files app |

All six ship together. One Rust core, one module format, a native UI on each platform.

**The phone can borrow the desktop.** When a job is too heavy for the phone (a
long 4K video, a 500-page OCR), the phone sends it to your own desktop over the
local network, the desktop does the work, and the result comes back. Still no
cloud. Nobody else does this.

## The quality bar

Studio-grade UI, blazing speed and tools better than the paid ones. These are
budgets, checked in CI, not adjectives.

**Speed budgets**

| Moment | Budget |
|---|---|
| Shortcut to command bar visible | under 50 ms |
| Keystroke to updated results | under 16 ms (one frame) |
| App cold start | under 300 ms |
| Idle memory | under 100 MB |
| Installer | under 30 MB, engines and models download on first use |
| Heavy work (video, PDF, images) | within 1.5x of native ffmpeg and qpdf |

**Studio-grade UI, everywhere, including community modules**

- Modules do not ship their own UI. They declare inputs, options and outputs,
  and the host renders them with one design system. Raycast does the same with
  its component API, which is why thousands of community extensions all look
  like Raycast. Every module looks first-party by default.
- One design system: type scale, spacing, motion curves, light and dark,
  keyboard-first everywhere, every state designed (empty, loading, progress,
  error, done). 3x zoom review on every screen before release.
- Live previews before you commit: see the compressed PDF size, the cropped
  frame, the converted image side by side, then save.

**Utility quality**

- Every module beats the tool it replaces on output, measured. PDF compression
  is judged on size at equal visual quality (SSIM) against the big web tools.
  Conversions are checked against a golden-file corpus of real-world ugly files.
- File parsers are fuzzed. Opening a hostile PDF must never crash the app or
  run code.
- The registry CI runs the corpus, the benchmarks and the fuzzers on every
  community module before merge. Quality is enforced by the pipeline, not by
  hoping contributors are careful.

## Local sharing

- **Default protocol: LocalSend's open v2 protocol.** Implementing it means we
  can send to and receive from LocalSend's existing user base (94k stars, apps on
  every platform) on day one, instead of starting a new network from zero.
- **No AirDrop needed.** When both devices run the app, they find each other on
  the local network and transfer directly at full Wi-Fi speed.
- **Device without the app.** The sender shows a QR code. The other device
  scans it and opens a page served by the sender over the local network, then
  downloads or uploads in its browser. Nothing to install, still no cloud.
- **Network blocks device-to-device traffic** (guest, hotel, office Wi-Fi often
  do). Fall back to the phone's hotspot, or a direct Wi-Fi link where the OS
  allows it.
- AirDrop interop is out of scope. It only matters for devices without the app,
  and the QR path already covers those.

## How modules work

The module system is the product. Raycast wins because of its extension store.
Gauntlet, an open-source Rust launcher with the same pitch, stalled at about
800 stars because it never got one.

- **One folder, one manifest, a few dozen lines.** Written in Rust, TypeScript,
  Python or Go, compiled to WebAssembly. Zed (a Rust editor, 92k stars) runs its
  extensions the same way.
- **The core does the heavy lifting.** Hardware video encoding, PDF rendering,
  image processing and OCR live in native Rust in the core. Modules call them.
  A ten-line module runs at full speed.
- **One module, every surface.** A module appears in the command bar, the
  right-click menu, the app window, the CLI and MCP without extra work. That is
  the pitch to contributors: write one small thing, reach humans and agents on
  every platform.
- **Wrap what exists.** Many great tools are already open-source command-line
  programs. A module can wrap one with a manifest and a form. tinycast reached
  8.4k stars partly by running existing Raycast extensions, so compatibility is
  a proven shortcut to a full catalogue.
- **Chains.** Modules compose. "HEIC to JPG, resize to 1600px, strip location,
  zip" is a saved pipeline you can bind to a shortcut or a watched folder.
- **Sandboxed by default.** A module only sees the files you hand it and has no
  network unless its manifest asks and you approve. That makes "nothing leaves
  your machine" provable even for community modules.
- **Registry is a public GitHub repo.** Adding a module is a pull request.
  Installing is one click. It costs nothing to run.

## Architecture sketch

- **Rust core crate:** file detection, the job pipeline, the module host, native
  engines, and the sharing stack. Shared by every platform.
- **Native UI per platform, one shared core.** Decided 2026-10-13 after the
  shell kill test (`prototypes/shell-test/`). Performance and low memory beat
  "one UI codebase".
  - macOS, Windows and Linux: one GPUI desktop app (Zed's Rust UI framework),
    calling the core directly. Amended after GPUI matched SwiftUI on memory
    and startup and the user could not tell them apart (ADR 0001 amendment).
  - iOS: SwiftUI. Android: Jetpack Compose. Both reach the core through
    UniFFI bindings.
  - This stays sustainable because modules never ship UI. They declare inputs,
    options and outputs, and each platform renders that small component set
    natively. Modules are written once, the renderer is written three times.
    Raycast gets its feel the same way.
- **Engines:** OS hardware encoders for video (VideoToolbox, Media Foundation,
  VA-API, MediaCodec), pdfium for PDF, native image libraries, OS OCR (Apple
  Vision, Windows OCR) with an open model as fallback, a permissively licensed
  local model for background removal.
- **Module runtime:** wasmtime on desktop and Android. iOS has no JIT, so an
  interpreter there (slower, but modules mostly call into the native core).

Speed evidence so far: a 1080p 20s clip to 720p took 1.07s with the native
hardware encoder on an M5 Max. Even the browser's hardware path (WebCodecs) took
1.42s. The usual ffmpeg-in-WebAssembly approach took over 45s. The native core
is where "blazing fast" comes from.

Shell kill test (2026-10-13, same command bar built three ways, macOS 26,
M5 Max, real Liquid Glass in all three):

| | SwiftUI | Tauri 2 | Flutter | Budget |
|---|---|---|---|---|
| Cold start | 182 ms | 397 ms | 328 ms | under 300 |
| Hotkey to visible (p50) | 24 ms | 12 ms | 7 ms | under 50 |
| Keystroke to frame (p50) | 9 ms | 16 ms (2-frame measuring floor) | 7.6 ms | under 16 |
| Idle memory (footprint) | 30 MB | about 91 MB with WebKit helpers | about 297 MB | under 100 |
| Bundle | 0.8 MB | 5 MB | 21 MB | under 30 |

All three are fast enough to feel instant. Memory and cold start separate them.
Only native met every budget. Tauri also needed private WebKit calls to reach
120 Hz and instant show, which rules it out of the App Store, iOS included.

## v1 modules (ranked by evidence)

1. **PDF suite.** Merge, split, compress, rotate, reorder, edit text, sign, fill
   forms, OCR, to and from images and Office. Largest traffic by far.
2. **Universal converter.** Image, audio, video and document formats.
3. **Image tools.** Compress, resize, HEIC, strip metadata, background removal
   (timed with remove.bg's shutdown).
4. **Video tools.** Trim, compress, to GIF, extract audio, change resolution.
5. **Local sharing.** LocalSend-compatible send and receive across devices.
6. **Clipboard and snippets.** History, pinned items, text expansion.
7. **Screenshot and OCR.** Capture a region, copy the text out of it.

**Later, as modules:** local dictation (the biggest viral cluster of 2025-26),
screen recording, system utilities (window snapping, per-app volume), file
search, downloader.

## Why now, and the evidence

- **The bundle idea works.** Vorssaint, "one menu bar icon doing the job of a
  dozen paid Mac apps", went from zero to 26.5k stars in four months and is
  still accelerating (+8.4k in the last 30 days). It is Mac only, has no file
  tools, and has 1,005 open issues. One team cannot keep up with demand.
- **Local file tools get attention.** BreezePDF scored 791 points on Hacker
  News, VERT 601. Both are browser only.
- **Agent file tools are the steadiest growth on GitHub.** markitdown added 26k
  stars in 90 days, OfficeCLI reached 32k in seven months, anydoc (Rust) 23k in
  two months.
- **Nobody has the whole thing.** No cross-platform open-source app combines
  converters, PDF, image, video, OCR, clipboard, local sharing and a module
  system at scale.

## Risks

- **Too broad means mediocre.** v1 ships a small set that is better than the
  paid versions, not two hundred rough tools.
- **Vorssaint adds file tools.** Our edge is every platform, phones included,
  the module system, local sharing and agent access.
- **StoryTold** launched Rust PDF and video editors two weeks ago (about 11k
  stars each, likely promoted). Standalone apps, not a platform, but watch them.
- **iOS rules.** Apple restricts apps that download executable code, so
  community modules on iOS may need to ship bundled with releases. Check
  before promising "install any module" on iPhone.
- **Licenses.** Avoid AGPL engines in the core. Keep ffmpeg use within LGPL or
  use OS encoders. Decide the project license up front.
- **Signing costs.** Apple developer account ($99 a year) and Windows code
  signing. Small but not zero.
- **Launch copy.** The common viral line is "free alternative to [paid app]".
  We lead with "the dozen paid apps you buy one by one" and keep competitor
  names out.

## Kill tests before the full build

Each has a bar set in advance.

1. **Module authoring is a breeze.** Rough host plus template. Pass: a new
   developer, or an agent working from the template alone, ships a working
   module in under 15 minutes and under 50 lines.
2. **Agents need it.** Give an agent 10 everyday file tasks with only standard
   command-line tools. Pass: it fails 3 or more on the first try, so a clean
   engine for agents has real value. Otherwise agents are a bonus, not a pillar.
3. **Sharing interop.** Pass: our client sends to and receives from stock
   LocalSend on Mac, Windows, Android and iOS.
4. **Speed.** Pass: within 1.5x of native ffmpeg and qpdf on five benchmark
   operations, and faster than the paid desktop apps we replace.
5. **UI shell.** Done on macOS, see the table above. Native won. Still to run
   on Windows and Linux to pick their renderer.

## Open decisions

- Windows and Linux renderer: GPUI, Slint or Tauri, by rerunning the shell
  kill test on those platforms.
- Project license (MIT or Apache-2.0 for adoption, GPL to keep forks open).
