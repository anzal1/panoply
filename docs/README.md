# Panoply documentation

Everything about the project, in the order most people need it. The front page is the
[README](../README.md).

## Using Panoply

| Page | What it covers |
|---|---|
| [KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md) | First-open steps for an unsigned build, what the beta does not do yet, and what differs between macOS, Windows and Linux |
| [PRIVACY.md](PRIVACY.md) | What stays on your device, the three things that can touch the network, and every download with its size, source and license |
| [../CHANGELOG.md](../CHANGELOG.md) | What changed in each release |
| [../SECURITY.md](../SECURITY.md) | How to report a security problem |

## Understanding Panoply

| Page | What it covers |
|---|---|
| [VISION.md](VISION.md) | What Panoply is, the four rules every tool is judged by, and why it exists |
| [MODULES.md](MODULES.md) | Every planned tool, ranked by demand, with its status |
| [CONVERSIONS.md](CONVERSIONS.md) | The conversion graph: 214 formats and 9,214 routes (generated) |
| [WIREFRAMES.md](WIREFRAMES.md) | The nine key screens, light mode, with their source |
| [design/UI-AUDIT.md](design/UI-AUDIT.md) | The desktop UI audited against the Apple, Fluent 2 and GNOME guidelines: what was fixed, what is open, the contrast and frame-time numbers, and before and after renders in [design/review/](design/review/) |
| [PLAN.md](PLAN.md) | Build phases and the gates between them |

## Building on Panoply

| Page | What it covers |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | How it is built. Start here as a contributor |
| [FLOWS.md](FLOWS.md) | The nine module flow patterns, the shared capabilities, the budgets every tool meets and the spec template |
| [specs/](specs/) | The full spec of every tool: [PDF](specs/pdf.md), [ID and images](specs/id-and-image.md), [video and audio](specs/video-and-audio.md), [files, find, share and capture](specs/docs-find-files-share-capture.md), [text, calculators, developer tools and privacy](specs/text-calc-dev-privacy-misc.md) |
| [DECISIONS.md](DECISIONS.md) | Merges, splits, cuts, deferrals and the budget exceptions, with the reason for each |
| [adr/](adr/) | The ten architecture decision records |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | The rules a change follows before it merges |
| [../apps/desktop/README.md](../apps/desktop/README.md) | The desktop app: layout, checks and how the screenshots are rendered offscreen |
| [../apps/desktop/packaging/README.md](../apps/desktop/packaging/README.md) | Bundling and the Linux build libraries |

## Measured results

The numbers on the front page come from these reports. Each says how to reproduce its results.

| Page | What it measured |
|---|---|
| [research/beta-smoke.md](research/beta-smoke.md) | All 33 registered modules over real files, through the path the app takes: 498 jobs, 0 failures |
| [research/pdf-compress-results.md](research/pdf-compress-results.md) | PDF compress on 35 files and a generated 300 MB file |
| [research/image-compress-results.md](research/image-compress-results.md) | Image compress at equal quality, resize to an exact size in KB, memory and speed |
| [research/media-results.md](research/media-results.md) | Video and audio: speed, quality at equal size, memory on multi-hour files |
| [research/ocr-results.md](research/ocr-results.md) | Text recognition accuracy by engine, language and scan quality |
| [research/vision-results.md](research/vision-results.md) | Background removal, erase, watermark removal and upscale: models, timings, memory, edge quality |
| [research/tiny-models.md](research/tiny-models.md) | The small, clean-license models surveyed, with sizes and download totals |

## Research behind the decisions

| Page | What it holds |
|---|---|
| [research/tool-harvest.md](research/tool-harvest.md) | The everyday file and utility tasks people look for, collected by demand |
| [research/launches-pdf-docs.md](research/launches-pdf-docs.md), [launches-media.md](research/launches-media.md), [launches-allinone-misc.md](research/launches-allinone-misc.md) | Recent launches in each area, and what they show |
| [research/x-launches.md](research/x-launches.md) | Indie file-tool launches on X, with their reach |

## Licenses and credits

[THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) lists everything Panoply is built on, including each
engine and model that is downloaded at run time. Panoply itself is dual licensed under
[MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE).
