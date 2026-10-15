# panoply-find

Instant file-name search for Panoply: "find anything on your desktop", results on every keystroke.
Specs: `docs/MODULES.md` section 7, `docs/specs/docs-find-files-share-capture.md` section 7 (Instant file
search), `docs/ARCHITECTURE.md` section 9. Design and measurements: `docs/research/instant-search-index.md`.

Safe Rust only (the workspace forbids `unsafe`). Dependencies: `memchr` (SIMD substring and byte search)
and `rayon` (the walker's work-stealing pool), both MIT or Apache. The `ignore-walker` feature adds the
`ignore` crate as a benchmark baseline; it is off by default.

## What is in it

| module | job |
|---|---|
| `segment` | The compact name table: a struct of arrays holding a folded and an original copy of every name, interned parent ids, modified time, a character mask, depth and flags. About 60 bytes per file. Save and load as a flat little-endian file. |
| `walk` | The parallel walker: one task per folder on a work-stealing pool, per-thread arenas, no per-entry lookup tables, one ordered merge. Honors the exclusion policy, `.metadata_never_index`, network mounts and one-file-system. |
| `query` | The per-keystroke engine: prefix and exact from a one-byte-per-file column, substring from a SIMD search over one buffer, fuzzy only on names the character mask lets through, smart case, several tokens, path tokens, ranking by match quality then recency and shallowness. |
| `fuzzy` | Subsequence scoring with word-start and run bonuses (fzf-style window). |
| `fold` | Case and accent folding so NFD and NFC names, `Café` and `cafe` all match. |
| `exclude`, `mounts` | What a scan never enters: `node_modules`, Rust `target`, `.git`, `*.noindex`, app bundles, system folders, network mounts. |

## Run the benchmark

```
cargo run --release -p panoply-find --example gen_tree -- ~/personal/panoply-find-bench.noindex 1000000 8
cargo run --release -p panoply-find --example bench -- ~/personal/panoply-find-bench.noindex --threads 8 --compare
```

Run both through `tools/lib/heavy-lock.sh` at `nice -n 15`. Never point `bench` at a real home folder at
full speed; give it `--threads 8` and run it at nice 15.
