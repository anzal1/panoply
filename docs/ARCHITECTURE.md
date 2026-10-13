# Panoply architecture

Panoply is one Rust core with a native UI on each platform. Every tool is a
module that declares what it needs, and the core runs it. This document is
the map. The why behind each big choice is in [adr/](adr/), and the
per-module behavior is in [specs/](specs/).

The four rules every part is judged by (see [VISION.md](VISION.md)):
blazing fast, bang on utility, lowest possible memory, butter smooth.

## 1. The shape

```mermaid
flowchart TB
  subgraph Surfaces
    MAC["Mac and iPhone app<br/>SwiftUI"]
    AND["Android app<br/>Jetpack Compose"]
    WIN["Windows and Linux app<br/>renderer chosen by kill test"]
    EXT["Finder, Explorer and share-sheet extensions"]
    CLI["panoply CLI"]
    MCP["MCP server for AI agents"]
  end
  FFI["panoply-ffi<br/>UniFFI bindings (Swift, Kotlin)"]
  subgraph Core["Rust core"]
    JOB["Job engine<br/>sniff, defaults, estimate, run, history"]
    HOST["Module host<br/>manifests, sandbox, registry"]
    ROUTE["Conversion router<br/>graph.json, shortest chain"]
    LADDER["Engine ladder<br/>OS engine first, own model second"]
    MODELS["Model manager<br/>download on first use, unload when idle"]
    INDEX["Index<br/>names, content, metadata, images"]
    SHARE["Share<br/>LocalSend v2, QR, borrow the desktop"]
  end
  subgraph Engines
    PDF["pdfium and own PDF code"]
    IMG["image pipeline"]
    MEDIA["OS hardware encoders<br/>plus LGPL ffmpeg for demux"]
    ML["ONNX Runtime for fallback models"]
    OS["OS engines: Vision, Speech, ML Kit,<br/>Windows AI, Translation"]
  end
  MAC & AND & WIN & EXT --> FFI --> Core
  CLI & MCP --> Core
  JOB --> HOST --> LADDER
  JOB --> ROUTE
  LADDER --> OS & ML & PDF & IMG & MEDIA
  LADDER --> MODELS
```

Every surface reaches the same core, so a module written once appears in the
bar, the app window, the right-click menu, the share sheet, the CLI and MCP.

## 2. Repository layout

| Path | What lives there |
|---|---|
| `crates/panoply-core` | Job model, file sniffing, output naming, cancellation, budgets |
| `crates/panoply-manifest` | `module.toml` schema and validation |
| `crates/panoply-host` | Module trait, registry, capability guard, built-in modules |
| `crates/panoply-convert` | Conversion graph loader and router |
| `crates/panoply-cli` | The `panoply` command |
| `crates/panoply-mcp` | MCP tool definitions generated from manifests |
| `crates/panoply-ffi` | Bindings for the native apps (Phase 2) |
| `conversions/formats.py` | Source of truth for formats and converters; generates `docs/CONVERSIONS.md` and `crates/panoply-convert/data/graph.json` |
| `modules/` | Module manifests, built-in and examples |
| `apps/` | Native apps per platform (Mac first) |
| `docs/` | Vision, architecture, flows, modules, decisions, plan, specs, research |
| `prototypes/` | Kill-test code that informed decisions (not shipped) |

## 3. Modules

A module is a manifest plus code.

- **Manifest (`module.toml`).** Declares the flow pattern (A to I, see
  [FLOWS.md](FLOWS.md)), accepted inputs, outputs, typed options with
  defaults, capabilities (files, network, models) and surfaces. The host
  validates it. A model with an unclean license or network access without a
  stated reason fails validation.
- **Built-in modules** are native Rust in `panoply-host` for full speed.
- **Community modules** compile to WebAssembly and run sandboxed: wasmtime on
  desktop and Android, an interpreter on iOS (no JIT allowed there). Heavy
  work never happens inside the sandbox; modules call host-provided engines
  (PDF, image, media, OCR) through imports, so a ten-line module runs at full
  native speed.
- **Capabilities.** A module sees only the files the user handed it and the
  output folder. Network is off unless the manifest asks with a reason and the
  user allows it. This makes "nothing leaves your device" provable for
  community code.
- **No module draws its own UI.** Each platform has one native component set
  (file input, number, choice, toggle, size, text, before and after preview
  for images, PDF pages, video frames and waveforms, result card). The app
  renders any manifest with it, so every module looks first-party and
  quality cannot drift.

## 4. Job lifecycle

```mermaid
sequenceDiagram
  participant U as Surface
  participant J as Job engine
  participant M as Module
  participant E as Engines
  U->>J: drop files or command
  J->>J: sniff bytes, build file profile
  J->>M: defaults for this profile
  M-->>U: options and estimate (size, time, route)
  U->>J: run
  J->>M: run on worker pool (never the UI thread)
  M->>E: streamed work, OS engine or fallback
  M-->>U: progress events
  M-->>J: outputs
  J-->>U: result card, history entry
```

- **Sniffing** reads magic bytes, never trusts extensions.
- **Profile** (pages, faces, GPS, codec, resolution, loudness) drives smart
  defaults per file.
- **Estimate before run**: size, dimensions, duration, route.
- **Outputs never overwrite.** `name (compressed).pdf` next to the original,
  or a chosen folder. Overwrite is explicit and sends the original to Trash.
- **Cancellation** is instant and leaves no partial files.
- **Errors** are plain language with a next step.

## 5. Conversion router

`conversions/formats.py` defines 214 formats in 20 families and the bridges
between families. It generates `graph.json`, which `panoply-convert` loads.
A request is routed by breadth-first search over plain converter steps, at
most four steps. Steps that need a model (transcription, OCR, read aloud) are
only taken as a single direct step, never chained. Only sensible family pairs
count, which yields 9,214 conversions. See [CONVERSIONS.md](CONVERSIONS.md).

## 6. Engine ladder and models

For every job that needs acceleration or ML, the ladder is:

1. **The OS engine** if present and it passes the module's quality bar:
   Apple Vision, Speech, Translation and NaturalLanguage; Android ML Kit;
   Windows AI APIs; VideoToolbox, MediaCodec, Media Foundation, VA-API.
2. **Our own small model** where the OS has nothing or falls short. Picked
   for size and a clean license ([research/tiny-models.md](research/tiny-models.md)),
   run with ONNX Runtime, downloaded only when the module is first used,
   checksum verified, size shown before download, loaded per job and unloaded
   after 60 seconds idle.
3. **No bundled language model.** Text-smart features use the OS model
   (Apple Foundation Models, Gemini Nano, Phi Silica), else the user's own
   local model or key, else they stay hidden. See ADR 0003.

The choice is per platform and decided in CI by the quality bar test, not by
opinion.

## 7. Memory and speed by design

- **Streaming everywhere.** PDFs page by page, images in bands or tiles, media
  through hardware pipelines with constant memory regardless of length.
- **Idle means idle.** No background CPU when nothing runs; the window is
  pre-created hidden so showing it is instant.
- **UI thread rule.** No task over 4 ms on it. All work runs on a worker pool
  with a CPU budget.
- **Budgets are tests.** The CI harness measures idle CPU, module start
  (under 50 ms), first preview (under 100 ms), frame rate (120 fps), and peak
  memory per job (under 300 MB for documents and images, flat for media).
  A module that misses one does not ship. Numbers are in
  [FLOWS.md](FLOWS.md#budgets-every-module-must-meet).
- **Measured baseline.** The SwiftUI shell idled at 30 MB with a 182 ms cold
  start in the shell kill test (`prototypes/shell-test`).

## 8. Sharing between devices

- **Protocol:** LocalSend v2, so Panoply interoperates with LocalSend's
  existing apps on every platform. Discovery over multicast and mDNS,
  transfers over HTTPS with certificate fingerprints.
- **Devices without Panoply:** a QR code opens a page served by the sender on
  the local network with a one-time token.
- **Borrow the desktop:** a phone sends a job spec and inputs to the user's own
  computer, which runs it and returns the result. Same channel, same trust.
- **Clipboard sync** over the same paired, encrypted link.
- **No servers.** Far-away transfer is a community module, not core.

## 9. Find (the index)

- File system events keep the index fresh: FSEvents on macOS, the USN journal
  on Windows, fanotify or inotify on Linux.
- Names in a memory-mapped table, full text in tantivy, metadata in SQLite.
- OCR text and image embeddings are computed in the background only within a
  CPU and battery budget, and only for folders the user includes.
- Plain-words queries ("photos taken in Goa in December") parse into filters.

## 10. Security and privacy

- Every parser is fuzzed; hostile files must never crash the app or run code.
- Modules are sandboxed with explicit capabilities.
- Model and engine downloads are pinned by checksum.
- Anything that touches the network is labelled before it runs.
- Reverse engineering of other tools is clean-room only (ADR 0005).

## 11. Platforms

| Platform | UI | Status |
|---|---|---|
| macOS | SwiftUI, Liquid Glass | Phase 2 |
| Android | Jetpack Compose | after Mac launch |
| Windows | GPUI, Slint or Tauri, by kill test | after Android |
| iOS | SwiftUI shared with macOS | after Windows |
| Linux | same as Windows | last |
| CLI and MCP | Rust, all desktops | first public drop (Phase 1) |
