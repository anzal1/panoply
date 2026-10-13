# Kill test 2: do AI agents need Panoply's file tools?

Gate from `docs/PLAN.md` Phase 0 gate 2, context in `docs/adr/0003`. Run on 2026-10-13.

## The bar (stated before any run, never changed)

An agent with only standard command-line tools fails at least 3 of 10 everyday file tasks on the first try, judged by automatic checkers.

- Fails 3 or more: an MCP file toolbox is a real pillar.
- Passes 8 or more: MCP is a bonus.

## Setup

- 10 tasks, each with `prompt.md` (a normal user ask), generated fixtures, and a strict `check.py`. No downloads, fixtures come from `gen_fixtures.py` (python3, Pillow, numpy, sips, textutil, ffmpeg).
- One fresh Sonnet subagent per task, told: "You are a capable coding agent with a shell. Do this task using any standard command-line tools available on this Mac. Work in <dir>/work. When done, say DONE." No hints beyond the user prompt. No retries after checking. Checked only after the agent said DONE.
- Machine tooling the agents could reach: ffmpeg and ffprobe, poppler (`pdfunite`, `pdftoppm`, `pdftotext`), sips, textutil, cupsfilter, python3 with Pillow and numpy, Swift (Vision), Google Chrome. Not present: ghostscript, qpdf, exiftool, ImageMagick, LibreOffice, pandoc, tesseract, pikepdf, PyMuPDF.
- Checkers were validated before the run: a reference solution passes each one, and the obvious naive route fails where it should (sips only for task 3, `-c copy` for task 5, plain resize for tasks 4 and 9).

### Run A was thrown out

The first pass ran with `check.py` inside the task folder. Two agents (tasks 1 and 9) said in their reports that `check.py` passed, so they had seen it. Run A went 9 of 9 (task 10 was not launched), but it is invalid. Verdicts are kept in `results/run-A-checker-visible.txt` for transparency only.

### Run B is the official run

All checkers, generators and answers were moved out of reach. Each agent got a clean arena with only `fixtures/` and an empty `work/`. Same prompts, only the paths changed. Verdicts in `results/run-B-clean.txt`, agent outputs and scripts in `results/run-B-work/`.

One checker fix was made after the agents ran but before the verdict was read: task 3 first scanned the first 64 KB of each JPEG for the bytes "GPS", which can false-positive on pixel data (an agent reported such random hits). It now scans only the metadata segments before the image data. The negative control (sips-only output) still fails, and no verdict changed.

## Results (run B, first try, automatic checkers)

| # | Task | Result | Time | Tool calls | How the agent got there, and the failure risk on the way |
|---|---|---|---|---|---|
| 1 | Compress 3.7 MB image-heavy PDF under 1 MB, keep text selectable | PASS (381 KB) | 28.0 s | 6 | No ghostscript or qpdf. Wrote a Python script that rewrites PDF objects and recompresses the embedded JPEGs with Pillow. Works only because the fixture is a plain-structure PDF |
| 2 | Merge 3 PDFs in a given order | PASS (9 pages, right order) | 9.9 s | 2 | `pdfunite`, then checked page text order |
| 3 | HEIC to JPG for 5 photos, strip GPS | PASS | 28.6 s | 7 | `sips` converts but keeps the GPS block. Agent noticed this on its own check and stripped the APP1 and APP13 segments by hand. Stopping at the obvious command would have failed |
| 4 | Resize photo to 200x230 JPG, 20 to 50 KB | PASS (27.7 KB) | 18.8 s | 6 | Crop to aspect, resize, check size. Quality 85 lands near 16 KB on this fixture, below the floor |
| 5 | Trim 00:05 to 00:12, correct start, no glitches | PASS (7.00 s, starts at 5.00 s) | 18.1 s | 4 | Spotted that the source has keyframes only at 0 s and 10 s, so re-encoded instead of `-c copy`. `-c copy` fails the checker (7.17 s, wrong start) |
| 6 | Compress video under 8 MB, keep 720p | PASS (6.6 MB, 1280x720) | 13.4 s | 2 | ffmpeg at a target bitrate. Default CRF 23 gives about 12 MB on this fixture |
| 7 | Extract audio as 128 kbps MP3 | PASS (CBR 128 kbps, tones intact) | 9.3 s | 2 | ffmpeg with libmp3lame |
| 8 | DOCX to PDF, keep headings, bold, table, bullets | PASS (2 pages, table row intact, bold present) | 32.9 s | 7 | No converter installed. Wrote a script that parses the DOCX XML to HTML and printed it with headless Chrome. `textutil` and `cupsfilter` do not get there |
| 9 | Passport 600x600 JPG, white background from a blue one | PASS | 58.1 s | 16 | numpy keying on blueness plus background unmixing, then square crop. Longest and most iterative task |
| 10 | Find which of 30 screenshots contains "ORDER-4471" | PASS | 31.4 s | 5 | No tesseract. Agent wrote a Swift script calling macOS Vision OCR |

**Total: 10 of 10 pass, 0 fail.** Total agent time 248.5 s, 57 tool calls, median task 18 to 28 s.

## Verdict

Bar: 3 or more failures makes MCP a pillar. Result: 0 failures and 10 passes, which is above the "8 or more" line. **MCP is a bonus, not a pillar**, as `docs/PLAN.md` gate 2 says to record when the bar is not met.

## Read this before leaning on the verdict

The pass rate is real but the conditions were generous. These are the reasons the result is an upper bound on what raw CLI tools do for agents, not a floor:

1. **This is a developer Mac.** ffmpeg, poppler, Pillow, Chrome and the Swift toolchain are installed. Every pass leans on one such tool: ffmpeg for tasks 5, 6 and 7, `pdfunite` for 2, Chrome for 8, Swift Vision for 10, Pillow for 1, 4 and 9. On a stock Mac, or on Windows, most of these tools are absent.
2. **Fixtures are synthetic and clean.** The task 1 PDF has plain objects, so a regex rewriter works. Real PDFs use object streams, compressed xref tables and subset fonts and would break that approach. The HEICs have no depth map or HDR gain map. The video is constant frame rate and unrotated. The passport subject has no blue and a hard edge.
3. **Checkers are thresholds.** They check size, dimensions, tags, text, duration and pixels, not visual quality. A visually poor but in-spec output passes.
4. **One run per task, one model (Sonnet).** No repeats. Agents self-verified, and several fixed their own first attempt before saying DONE. "First try" here means the final output when the agent said DONE, not its first command.
5. **n is 10.** The bar is a coarse test and 0 of 10 does not say how it would go on 100 tasks.

## Near misses (what the agents had to work around)

None ended in a failed check, but these are the places where the obvious command was wrong or missing. They show which Panoply tools remove real risk, ranked by how much agent effort and error risk they carried:

1. **Missing converter or engine, agent writes its own** (tasks 1, 8, 10: PDF recompression, DOCX to PDF, OCR). Each worked only because a capable agent found Chrome headless, Swift Vision or raw PDF surgery. This is the biggest hole in raw CLI tooling.
2. **Plausible default silently does the wrong thing** (tasks 3, 4, 5, 6): `sips` keeps GPS, default JPEG quality undershoots a size floor, `-c copy` trims on the wrong keyframe, default CRF overshoots a size cap. Each passed only because the agent verified the output. The correct tool encodes the right default and refuses to leave GPS behind.
3. **Needs a perceptual primitive, no CLI exists** (task 9: background removal, 16 calls, 58 s). Worked on a clean synthetic photo. A real photo with hair and similar colors is where this would fail.
4. **Verification burden sits on the agent.** Every pass needed `ffprobe`, `pdftotext` or a look at the result. A tool that returns a verified result (size, tags, page count) saves those calls.

## What this means for Panoply

- Gate 2 as written: not met, so MCP is a bonus. Do not make `panoply-mcp` a launch pillar on this evidence alone.
- The generated CLI and MCP surface costs little, so keep it. If MCP tools get prioritised, the order that follows from the near misses is: strip metadata (GPS), PDF compress, DOCX/Office to PDF, OCR search across a folder, trim with correct start, background cutout, then the rest.
- The result is most fragile on point 1 above. A cheap follow-up that would test it without changing the bar: rerun the same 10 with a stripped PATH (no Homebrew, no Chrome), and with Haiku. Not done here.

## Files

- `tasks/NN-*/` prompt.md, fixtures/, check.py, work/ (empty after the runs)
- `gen_fixtures.py` regenerates all fixtures and prompts (deterministic)
- `checklib.py`, `run_checks.py` shared checker helpers; `python3 run_checks.py <arena-dir>` re-checks an arena
- `_private/` answers used by checkers 3 and 10 (keep out of agent reach when running)
- `_reference/` the Swift reference solutions used to validate checkers 8 and 10
- `results/` run A (invalid) and run B (official) verdicts, plus run B outputs and agent scripts
