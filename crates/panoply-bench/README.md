# panoply-bench

The budget harness. It runs a module over a slice of the test corpus and checks the numbers in [docs/FLOWS.md](../../docs/FLOWS.md#budgets-every-module-must-meet): start time, first preview, CPU, peak memory.

Every job runs in a child process, so each one gets its own peak-memory reading. Results print as plain text
tables, with the same data available as JSON. The exit status is 0 when no budget failed, 1 when one did.

```sh
cargo run -p panoply-bench --release -- check              # one budget table
cargo run -p panoply-bench --release -- run --kind pdf     # a module over a corpus slice
cargo run -p panoply-bench --release -- --help
```

Fetch the corpus first (see [CONTRIBUTING.md](../../CONTRIBUTING.md#the-quick-dev-loop)). The bench measures with
`/usr/bin/time`, so it needs macOS or Linux.

Part of [Panoply](../../README.md). Licensed under MIT OR Apache-2.0.
