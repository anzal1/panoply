# Panoply vs iLovePDF, PDF compression

Status: NOT RUN. The iLovePDF side is blocked by the built-in browser's policy, so there is no iLovePDF figure
in this file and none is claimed. The Phase 0 gate (beat iLovePDF's "Recommended" size at equal visual quality)
is undecided.

Date: 2026-10-14.

## What was prepared

- Candidate set: 19 files from the table in `docs/research/pdf-compress-results.md` (public domain, Apache-2.0,
  MIT, CC-BY-SA, plus generated files). Left out: the 315 MB generated file. The two signed files
  (`pdf-qpdf-digitally-signed`, `pdf-pyhanko-signed-twice`) are in the set but a web tool will break their
  signatures, so their rows need that note.
- `helper.py`: a local helper (127.0.0.1:8765, read-only allowlist of the 19 files, POST saves into
  `results/`, CORS for https://www.ilovepdf.com including Private-Network preflight). Not needed once a file
  picker route works.
- `crates/panoply-pdf/examples/compare_pdfs.rs`: SSIM of the first, middle and last page of two PDFs, drawn
  with pdfium at 100 dpi, the same measure as `compress_report`. Built and ready for the iLovePDF outputs.
- Panoply outputs for all 19 files are in `results/panoply/` (default "smallest that still looks good").
  Sizes below.

## Why it stopped

The page at https://www.ilovepdf.com/compress_pdf cannot reach the helper. From the page, every fetch to
`http://127.0.0.1:8765` and `http://localhost:8765` fails, including `mode: 'no-cors'`, and the helper log shows
that not even a preflight arrived. In the pane, `navigator.permissions.query` reports the local network access
permission (`local-network-access`, `loopback-network`) as `denied`, while a plain fetch to a public site
works. This is a host policy of the browser pane, not something the page does. I did not change that
setting, and I did not try a LAN address (same permission), a native file dialog (it would pop up over
the user's work), or pasting file bytes through the tool channel (only the sub-160 KB files would fit, and they
are the least informative). No file was uploaded to iLovePDF. No account was made. No CAPTCHA or limit message
was seen, because no upload started.

To unblock, either of these is enough:
1. Allow local network access for the Browser pane on https://www.ilovepdf.com, then re-run (the helper and
   method are ready).
2. Upload the 19 files by hand through the iLovePDF page (Recommended level, then Extreme), save the outputs as
   `results/<id>_rec.pdf` and `results/<id>_ext.pdf`, and run the scoring step below.

Scoring step once outputs exist: `target/release/examples/compare_pdfs original.pdf result.pdf` for page SSIM,
`pdftotext` on each output for text selectability, sizes from `stat`.

## Panoply side, ready for the comparison

Sizes in bytes. "iLovePDF Recommended" and "Extreme" are empty on purpose.

| id | class | original | Panoply | Panoply saved | iLovePDF Recommended | iLovePDF Extreme |
|---|---|---:|---:|---:|---|---|
| ia-scan-public-lands | scanned | 3880663 | 3827070 | 1.4% (kept) | not run | not run |
| ia-scan-mining-claims | scanned | 6526211 | 6516205 | 0.2% (kept) | not run | not run |
| arabic-primer-wdl | scanned | 5206646 | 5195075 | 0.2% (kept) | not run | not run |
| arabic-book | scanned | 1197732 | 1171225 | 2.2% | not run | not run |
| qpdf-fax-ccitt | scanned | 1565966 | 218281 | 86.1% | not run | not run |
| gen-scanned-skewed | scanned | 280391 | 279904 | 0.2% (kept) | not run | not run |
| qpdf-image-streams | image-heavy | 2970696 | 1436694 | 51.6% | not run | not run |
| hebrew-parable | image-heavy | 467005 | 445381 | 4.6% | not run | not run |
| pdfjs-cmyk-jpeg | image-heavy | 374080 | 340499 | 9.0% | not run | not run |
| nist-sp800-53r5 | text-heavy | 6073678 | 5159830 | 15.0% | not run | not run |
| irs-p17-2022 | text-heavy | 2921289 | 2796638 | 4.3% | not run | not run |
| gen-3000-pages | text-heavy | 1074148 | 558405 | 48.0% | not run | not run |
| pdfjs-arabic-cid-truetype | text-heavy | 39370 | 34564 | 12.2% | not run | not run |
| pdfjs-annotation-highlight | text-heavy | 92397 | 92130 | 0.3% (kept) | not run | not run |
| irs-f1040-2022 | forms | 156045 | 123855 | 20.6% | not run | not run |
| qpdf-form-filled-acrobat | forms | 61841 | 29987 | 51.5% | not run | not run |
| pdfbox-acroform-rotation | forms | 42637 | 17584 | 58.8% | not run | not run |
| qpdf-digitally-signed (signature breaks) | mixed | 68846 | 54112 | 21.4% | not run | not run |
| pyhanko-signed-twice (signature breaks) | mixed | 107981 | 60200 | 44.2% | not run | not run |

These Panoply sizes reproduce the Phase 1 results table exactly, so the engine side is stable.

## Verdict

Undecided. Reading the Phase 1 numbers alone, the likeliest outcome is a split: Panoply is strong on structure-only
wins and CCITT scans, and weak on JPEG 2000 and JBIG2 scans (6 of 19 files gain under 2%), where a tool that
re-encodes pictures will likely win on size. That is a prediction, not a measurement.
