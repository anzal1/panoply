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
5. Run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`,
   `cargo deny check`.

## Adding a format or conversion

Edit `conversions/formats.py`, run `python3 conversions/formats.py`, commit the
regenerated `docs/CONVERSIONS.md` and `crates/panoply-convert/data/graph.json`.

## Decisions

Big choices get an ADR in [docs/adr/](docs/adr/) with context, decision and
consequences. Propose one in a pull request.

## Conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
