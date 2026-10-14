# panoply-smoke

The smoke matrix: every module over real corpus files, through the same path the desktop app uses, with the budgets checked.

Each job runs in its own process, so the kernel reports that job's peak memory. A module may decline a file it cannot
handle, and a decline counts as clean only when a rule in `src/expect.rs` explains it, so a polite refusal is told
apart from a bug. What the matrix found and fixed is in [findings.md](src/findings.md).

```sh
python3 -I tests/corpus/fetch.py --kind pdf,image   # once, from the repository root
cargo run -p panoply-smoke --release -- --help
```

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
