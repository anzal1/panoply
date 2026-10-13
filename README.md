# Panoply

Every utility you have ever paid for, downloaded once, or uploaded a file to a
stranger's website for, in one open-source app. It runs on your own device,
opens from a shortcut, shares files with your other devices over your own
network, and anyone can add a new tool to it.

Built by the people, for the people.

> **Status: pre-alpha.** The product is fully specced (about 270 modules, 9,214
> conversions, budgets and quality bars for each) and the Rust core is
> scaffolded. Nothing is released yet. See [the plan](docs/PLAN.md).

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

Nothing leaves your device. No accounts, no limits, no watermarks.

## What it does

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
| [research/](docs/research/) | Market, launch and model research behind the decisions |

## Repository layout

```
crates/        Rust core: core, manifest, host, convert, cli, mcp, ffi
conversions/   formats.py, the source of truth for the conversion graph
modules/       module manifests (examples to start)
apps/          native apps per platform (Mac first)
docs/          everything above
prototypes/    kill-test code behind the decisions, not shipped
```

## Try the core

```bash
cargo build
cargo run -p panoply-cli -- route docx jpg
cargo run -p panoply-cli -- formats --family Audio
cargo test
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
