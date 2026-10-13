# Kill test 1: adding a module is a breeze

Date: 2026-10-13. Phase 0 gate 1 in `docs/PLAN.md`.

## The bar (stated before the test)

A fresh author, given only `modules/template-rust/README.md`, ships a working module:

- in under 15 minutes of wall clock, and
- in under 50 lines of module code (`src/lib.rs`, excluding `Cargo.toml`, `module.toml` and
  generated code).

Task given to the author: "Create a Panoply module at modules/killtest/csv-to-json that converts a
CSV file to a JSON array of objects (first row is headers), pattern A, following the template.
Build it and run it on a small CSV you create. Report the exact commands you ran."

Author: a fresh Haiku subagent each time, with no knowledge of this work. The only additions to the
prompt were an instruction to keep cargo cool (`nice -n 15`, `-j3`) and to log friction.

## Result: PASS (on the second author, after one README improvement)

| | Run 1 | Run 2 |
|---|---|---|
| Wall clock, start of task to finished | 120 s | 104 s |
| Module code, `src/lib.rs` total lines | 115 (98 without blanks and comments) | 45 (39 without blanks and comments) |
| Time bar (under 15 min) | pass | pass |
| Line bar (under 50) | **fail** | pass |
| Output correct | yes | yes |
| README version | first draft | after the fixes below |

Output verified by me, not by the author: each run's JSON was loaded and compared with Python's
`csv.DictReader` on the same file. Run 1 (quoted commas, doubled quotes, a newline inside a quoted
field, a non-ASCII name, an empty last field): 4 rows, identical, keys in header order. Run 2 (quoted
comma, empty field): 3 rows, identical, keys in header order.

Code: run 1 is in `prototypes/module-killtest/run1-csv-to-json/`, run 2 is in
`modules/killtest/csv-to-json/` (with its sample `people.csv` and output).

### Why run 1 failed the line bar

Nothing in the first README said that crates from crates.io work. The author hand-wrote an RFC 4180
CSV parser (about 70 lines) plus validation. The template was not the obstacle, the missing sentence
was. After the README gained a "Use crates, keep it short" section, run 2 used `cargo add csv` and
wrote 45 lines, including the progress and cancel calls copied from the template. A reference
solution I wrote in the same style needs about 22 lines.

Per the plan, the README was improved once after the first run, then rerun with another fresh
author. Both authors needed no help from me and read only the template folder.

## Friction points the authors hit

Run 1 (README fixes applied before run 2 are marked "fixed"):

1. README said "change two files" but a third change was needed: `serde_json` key order is
   alphabetical without the `preserve_order` feature. Fixed: a tip, with the `cargo add -F` form.
2. `cp -r modules/template-rust ...` copied the template's 143 MB `target/` folder. Fixed: the
   template test now builds in a scratch copy, the README copies only the five files that matter.
3. The target folder `modules/killtest` did not exist, so `cp` failed. Fixed (run 2 hit it again
   because the fix was only a note): the README now has `mkdir -p` in the command.
4. `outputs` in `module.toml` was never explained. Fixed.
5. Only patterns A and I were described. Fixed: one line each for B, E and F, and a pointer to
   `docs/FLOWS.md`.
6. Unclear where to run `panoply run` and what the paths are relative to. Fixed in run 2's README,
   but run 2 still found it ambiguous (item 9 below), so it is reworded again.
7. The package name in `Cargo.toml` looked like something that had to change. Fixed: stated that
   it does not matter.
8. The author did not know third-party crates are allowed. Fixed (this was the line-bar failure).

Run 2 (fixes applied after the run, not re-tested by a third author, only by me following the
README literally in a scratch folder: copy, build, run all worked):

9. `cp -r` into `modules/killtest/csv-to-json` failed because `modules/killtest` did not exist.
   README now says `mkdir -p` and uses `modules/mine/my-module`.
10. The copied folder contains the template's own `README.md`. The copy command now lists the five
    files to copy and leaves the README behind.
11. How `accepts` matches (type from bytes or extension, or either) was not explained. Added: one
    matching entry is enough, a `.csv` has type `text/csv`.
12. The README's `serde_json` tip would duplicate the template's line if pasted. Now says to run
    `cargo add serde_json -F preserve_order`.
13. A module with no options still receives `options`. Now says to name it `_options` and ignore it.
14. A `json` label gives `people (json).json`. Now explained, with an `as json` suggestion.
15. `cargo run -p panoply-cli` must run from the clone's root, so file paths are relative to that.
    README now shows the `cd` and the full paths.
16. Excel CSVs start with a byte order mark that the module did not strip. Tip added.

Items not fixed, and why:

- `cargo add` prints index and feature noise. Cargo's, not ours.
- Each module project has its own 140 MB `target/` folder after a release build. A shared
  `CARGO_TARGET_DIR` would fix it but breaks the "built file is found by itself" rule. Left as is.
- The first `cargo run -p panoply-cli` for a fresh author builds wasmtime and takes minutes. The
  README says so. A prebuilt `panoply` binary removes it, which is a distribution task.

## What was measured alongside (host, dev build with optimized dependencies, Apple Silicon)

- `panoply run` on a small file, whole process including compile: 13 ms median (word-count),
  16 ms (slugify-lines). With Cranelift instead of Winch the same run was 43 to 55 ms.
- Compile step alone (88 KB component): Winch 11 ms, Cranelift opt level none 38 ms.
- Peak memory: 18 MB resident for a small job. A 30 MB text file through word-count: 205 ms,
  80 MB resident.
- Cancel of a module that never polls (an infinite loop): cancel fired at 300 ms, the job returned
  at 317 ms, about 16 ms later. No files left behind.

## Verdict

The bar holds: a fresh author goes from the README to a working, correct module in under two
minutes and under 50 lines, provided the README says crates work. That sentence is now in it.
The honest caveat is sample size: two Haiku authors on one task. The task is a typical Pattern A
module. A harder module (one that needs a host engine, like PDF) is outside this test because the
engine imports do not exist yet.
