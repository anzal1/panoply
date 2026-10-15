# Instant search index: design and kill test (phase 1)

Crate: `crates/panoply-find`. Budgets come from `docs/MODULES.md` section 7, the Instant file search spec and
`docs/ARCHITECTURE.md` section 9: under 10 ms per keystroke on 1 million files, index memory well under 100 MB,
initial index in seconds, near-zero idle CPU, permissive licenses only.

## Design

- **Name table (`segment.rs`).** A struct of arrays, about 57 bytes per entry: a folded (lowercase,
  accent-free) copy and an original copy of every name in two contiguous buffers, offsets, an interned parent id
  (a folder is stored once, paths are rebuilt only for rows on screen), mtime, a 32-bit character mask, the first
  folded byte, depth and flags. Safe Rust, no mmap yet (the workspace forbids `unsafe`); a saved index loads with
  one read.
- **Query (`query.rs`).** Pass 1: prefix and exact from the one-byte-per-file `first` column through `memchr`.
  When it fills the pool nothing else can outrank it, so one-letter queries end here. Pass 2: SIMD substring
  search (`memchr::memmem`) over the whole folded buffer, cut into slices and scanned by a parked 8-thread pool
  on big indexes. Pass 3: fuzzy, only when the page is still short, behind a one-AND-per-file mask prefilter.
  Ranking is match tier (exact, prefix, word start, substring, fuzzy) plus recency, shallowness and name coverage;
  a caller-supplied boost adds frecency to the best few hundred candidates. Smart case, several tokens (the
  longest drives the scan, the rest are checked in the name or the folders above it) and `a/b` path tokens work.
- **Walker (`walk.rs`).** One task per folder on a rayon pool, per-thread arenas, one ordered merge that fills
  parent, depth and hidden without any path-keyed lookup. Exclusions: `node_modules`, Rust `target` next to a
  `Cargo.toml`, `.git`, `*.noindex`, app bundles, system folders, `.metadata_never_index`, network mounts (parsed
  from `mount` or `/proc/self/mountinfo`), other file systems.
- **Freshness (phase 2).** FSEvents, USN and inotify or fanotify events become small delta segments plus
  tombstones next to the big segment (`Index` is already a list of segments; `F_DEAD` is the tombstone), merged
  in the background.

## Measurements

Synthetic tree from `examples/gen_tree.rs`: 1,000,000 files in 83,000 folders (1,083,006 entries), names in
realistic styles, mtimes spread over eight years. Apple M5 Max, APFS, run at `nice -n 15` with 8 threads while
other agents were building on the same Mac (load average 6 to 11), so every figure is pessimistic.

| | result | budget |
|---|---|---|
| Index build, 1.08 M entries, no mtime | 1.2 to 1.7 s | seconds |
| Index build with mtime (one lstat per entry) | 4.9 to 5.2 s | seconds |
| Same build with the `ignore` crate's WalkParallel | 1.20 s without mtime, 5.4 s with | |
| Merge into final columns | 25 ms | |
| Columns | 59.0 MB, 57.1 bytes per entry | under 100 MB per 1 M |
| Resident memory of a fresh process holding the index | 66 MB | |
| Cold load from the saved file | 7 ms (61.9 MB file) | under 300 ms |

Per-keystroke latency, limit 50, 2,000 sampled queries per class (first run of each query; best-of-3 in
brackets). Index loaded from the saved file:

| class | p50 | p99 | max |
|---|---|---|---|
| 1 char (prefix, substring, random) | 0.6 to 0.7 ms | 1.2 to 2.7 ms | 3.1 ms |
| 2 char (prefix, substring, fuzzy, random) | 0.9 to 1.1 ms | 2.0 to 3.0 ms | 7.5 ms |
| 3 char (prefix, substring, fuzzy, random) | 1.3 to 2.0 ms | 2.4 to 6.8 ms | 20 ms |
| 8 char (substring, fuzzy, no match) | 0.9 to 1.7 ms | 1.7 to 10.7 ms | 63 ms |
| every 1 and 2 character query (1,638) | 0.75 ms | 2.3 ms | 4.2 ms |

All figures above are from the build before the multi-token fix. The only queries over 10 ms were multi-token
ones with a one-letter token ("tosoka s"), where the scan was driven by the shortest token; the driver is now the
longest token and a later driver only runs for a short page.

## Decisions and what did not work

- The mtime is the cost, not the walk: reading names takes 1.2 s for 1 M entries, the per-entry `lstat` adds about
  4 s. `fstatat` relative to the open folder (rustix) did not help. Next: `getattrlistbulk` on macOS (needs a
  small `unsafe` crate like `panoply-platform-macos`), the NTFS MFT on Windows, `statx` on Linux, or read mtimes
  lazily and let the watcher fill them.
- The `ignore` crate's walker matches ours, so the stock walker is fine as a fallback; ours avoids per-entry path
  hashing and has no gitignore machinery.
- A trigram posting list (as sketched in the spec) would double the memory; a linear SIMD scan over 18 MB across
  8 threads is already around 1 ms.
- No reverse engineering was needed: the bar is reachable, so ADR 0005 was not triggered.

## Phase 2 measurements (macOS)

Same 1,083,006 entry tree and the same Mac under the same shared load, release build, 8 walker threads.

| first scan, with modified times | best of 3 builds |
|---|---|
| `getattrlistbulk` (name, type, mtime, device in one call per few hundred entries) | 2.99 to 3.78 s |
| `readdir` plus one `lstat` per entry (`bench --lstat`) | 4.65 to 5.00 s |
| names only, no mtime (`bench --no-mtime`, the floor) | 1.75 s |

The bulk read removes about 40 percent of the lstat cost; what is left over the floor is the kernel filling in the
attributes. A volume without bulk support falls back to the old path by itself (`WalkOptions::bulk_reader`
turns it off on purpose).

Live updates, from `examples/watch_demo.rs` on a 1 M entry index with FSEvents (0.1 s stream latency, 200 ms
settle window):

| change | searchable after |
|---|---|
| new file | 217 ms |
| rename | 219 ms |
| delete | 221 ms |
| 2,000 file burst across 20 new folders | 316 ms |
| removal of that burst | 309 ms |
| idle for 60 s | 0.00 s of CPU, rss steady at 74.0 MB |
