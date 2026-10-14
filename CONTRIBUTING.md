# Contributing to Panoply

Thanks for helping. Panoply only works if every tool in it is better than the
paid one it replaces, so the bar for merging is high and written down.

## Before you start

1. Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and
   [docs/FLOWS.md](docs/FLOWS.md).
2. Find the module's spec in [docs/specs/](docs/specs/). If it has none, open a
   module proposal issue first.
3. Check [docs/DECISIONS.md](docs/DECISIONS.md) so you do not rebuild something
   already merged into a shared engine.

## The quick dev loop

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it for you) and Python 3.

```sh
# Before every push: format, lint and test what you changed (and the crates that depend on it).
tools/preflight.sh
```

`tools/preflight.sh` is a local replica of the CI checks (see [docs/CI.md](docs/CI.md)): `cargo fmt --check`,
clippy with `-D warnings` and the tests through cargo-nextest, limited to the crates changed since
`origin/main` plus every crate that depends on them, and the whole workspace when `Cargo.lock`, a root
`Cargo.toml` or a workflow changed. It runs at low priority, builds into a `.noindex` folder (see the macOS
tip below) and prints the time it took, usually 1 to 3 minutes on a warm build folder. `--all` runs the
whole workspace, `--heavy` adds the slowest corpus and model tests that CI runs on Linux, `--plan` only
prints what would run. It needs [cargo-nextest](https://nexte.st) (`cargo install cargo-nextest --locked`).

The same checks by hand:

```sh
cargo fmt --all --check
cargo clippy --workspace --exclude gpui-base --all-targets -- -D warnings
cargo nextest run --workspace --exclude gpui-base --no-fail-fast

# Test corpus: files are fetched or generated on demand, and only the manifests are committed.
pip install pillow
python3 -I tests/corpus/fetch.py --kind pdf,image
python3 tests/corpus/generate.py --kind pdf,image
```

Notes:

- `gpui-base` is a patched copy of a UI library, kept in the workspace tree but excluded from lint and tests.
- Tests that need a model or an engine download it. Panoply asks before any download, so tests set
  `PANOPLY_YES=1` (the same as `--yes`). Point `PANOPLY_DATA_DIR` at a throwaway folder so test runs do not
  touch your own data folder.
- Tests that use a corpus file skip when it is not on disk. Run the fetch step above to run them all.
- The first build compiles the PDF, image and media engines and takes a while. Later builds are incremental.
- Run `cargo nextest run -p <crate>` while you work and `tools/preflight.sh` before you push.
- A change to `Cargo.lock` needs `THIRD-PARTY-NOTICES.md` regenerated (see Notices below).

**macOS tip: keep Spotlight out of your build folder.** Spotlight indexes a `target` folder and burns CPU
during big builds. A folder whose name ends in `.noindex` is skipped, so point cargo at one:

```sh
export CARGO_TARGET_DIR="$HOME/panoply-target.noindex"
```

Put the line in your shell profile. The repository ignores `*.noindex/` already.

## The rules every change follows

**Budgets.** Every module meets the table in
[FLOWS.md](docs/FLOWS.md#budgets-every-module-must-meet): no idle CPU, module
start under 50 ms, first preview under 100 ms, no UI-thread task over 4 ms,
120 fps, peak memory caps, streamed media. CI measures them. An exception needs
a stated reason in the spec.

**Quality bar.** Every module names the tool it must beat and how that is
measured. A pull request that adds or changes a module includes the benchmark
result.

**Engine ladder.** Use the OS engine first. Add a model only where the OS has
none or fails the bar, pick the smallest one with a clean license, and download
it on first use. No bundled language models.

**Licenses.** Code is MIT OR Apache-2.0. No GPL, LGPL-linked or AGPL
dependencies; `cargo deny check` must pass. Model weights need MIT, Apache-2.0,
BSD or a similar clean license, verified on the model card.

**Notices.** `THIRD-PARTY-NOTICES.md` is generated and ships in every download. When you change
`Cargo.lock`, run `cargo fetch --locked && python3 -I tools/notices/generate.py` and commit the result, or
CI fails. When you add or change a download (a model, an engine, a helper), update
`tools/notices/runtime.md` with its source URL and the license you read there. The generator fails if a
pinned version or commit in the source is missing from that file.

**Clean room.** We study how great tools work (with
[REA](https://github.com/morluto/rea) when a bar is hard to reach) and write
down the technique with evidence in `docs/research/`. Code is written from
those notes only. Never paste or translate decompiled or proprietary code.

**Privacy.** Modules see only the files the user handed them. Network access
needs a reason in the manifest and the user's consent, and is labelled in the
UI.

**Writing.** Plain, short sentences. No em dashes. Errors tell the user what
happened and what to do next.

## Adding a module

1. Copy a manifest from [modules/examples/](modules/examples/) and fill in the
   pattern, inputs, options with defaults, capabilities and surfaces.
2. Built-in modules are Rust in `crates/panoply-host`. Community modules
   compile to WebAssembly and call host engines for heavy work.
3. Do not build UI. The app renders your manifest with native components.
4. Add tests on the shared corpora and a benchmark against the named
   competitor.
5. Run the quick dev loop above, then `cargo deny check`.

## Adding a format or conversion

Edit `conversions/formats.py`, run `python3 conversions/formats.py`, commit the
regenerated `docs/CONVERSIONS.md` and `crates/panoply-convert/data/graph.json`.

## Pull requests

Keep a pull request to one change. Fill in the template, link the issue it closes, and say how you checked it.
CI must be green on macOS, Windows and Linux. Use plain sentences for commit messages.

## Decisions

Big choices get an ADR in [docs/adr/](docs/adr/) with context, decision and
consequences. Propose one in a pull request.

## Conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
