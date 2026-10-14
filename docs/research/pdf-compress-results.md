# PDF compress results, Phase 1

Measured on 2026-10-14 with the engine in `crates/panoply-pdf` (commit state: uncommitted working tree).
Nothing here was uploaded anywhere, and no competitor figure is claimed. A comparison with iLovePDF is
left to a separate run (candidates are listed at the end).

## Setup

- Machine: Apple M5 Max, macOS, release build without LTO (`CARGO_PROFILE_RELEASE_LTO=false`).
- Corpus: `tests/corpus/pdf.toml`, default tier plus the generated 300 MB file. 44 default-tier files, 35 of
  which the engine opens and compresses. The other 9 are skipped on purpose: 8 are encrypted (Phase 1 does not
  decrypt, it says so) and 1 is not a PDF (5 bytes).
- Setting: "smallest that still looks good" (the default). Pictures are shrunk to at most 150 dpi where the page
  shows them smaller, and recoded as JPEG at the lowest quality whose SSIM against the original stays at or
  above 0.97, per picture. A picture that does not get smaller by at least 5% is left alone.
- Never inflate: a result that saves under 2% is not written. The module keeps the original and says
  "Already small". Those files are marked "(original kept)" below.
- SSIM columns. "Lowest image SSIM" is the lowest score among recoded pictures (luma, 8x8 windows, against the
  original brought to the same pixel size). "Lowest page SSIM" and "mean page SSIM" draw the original and the
  result with pdfium at 100 dpi and compare up to 6 pages spread evenly through the file. `-` means nothing was
  recoded, or the file could not be drawn (the two damaged generated files).
- Reproduce: `cargo run --release -p panoply-pdf --example compress_report` (add `-- --under-mb 1` for the
  size-target run, `-- --tier large` for the 300 MB file).

## Per file

| file | class | license | before | after | saved | images | recoded | images left alone | lowest image SSIM | lowest page SSIM | mean page SSIM | ms | target met |
|---|---|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-digitally-signed | forms | Apache-2.0 | 68846 | 54112 | 21.4% | 0 | 0 | - | - | 1.000 | 1.000 | 3 | - |
| pdf-qpdf-form-filled-acrobat | forms | Apache-2.0 | 61841 | 29987 | 51.5% | 1 | 0 | Undecodable 1 | - | 1.000 | 1.000 | 3 | - |
| pdf-qpdf-damaged-stream | text-heavy | Apache-2.0 | 77635 | 77536 | 0.1% (original kept) | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-qpdf-bad-xref | text-heavy | Apache-2.0 | 819 | 691 | 15.6% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-qpdf-linearized | text-heavy | Apache-2.0 | 11325 | 4417 | 61.0% | 0 | 0 | - | - | 1.000 | 1.000 | 1 | - |
| pdf-qpdf-image-streams | image-heavy | Apache-2.0 | 2970696 | 1436694 | 51.6% | 9 | 3 | Kind 4, NoGain 2 | - | 1.000 | 1.000 | 36 | - |
| pdf-qpdf-fax-ccitt | scanned | Apache-2.0 | 1565966 | 218281 | 86.1% | 8 | 3 | Graphic 1, Kind 2, Tiny 2 | 0.970 | 0.994 | 0.998 | 97 | - |
| pdf-qpdf-inherited-rotate | text-heavy | Apache-2.0 | 978 | 701 | 28.3% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-pdfjs-arabic-cid-truetype | text-heavy | Apache-2.0 | 39370 | 34564 | 12.2% | 0 | 0 | - | - | 1.000 | 1.000 | 2 | - |
| pdf-pdfjs-cmyk-jpeg | text-heavy | Apache-2.0 | 374080 | 340499 | 9.0% | 1 | 0 | Kind 1 | - | 1.000 | 1.000 | 31 | - |
| pdf-pdfjs-xfa | forms | Apache-2.0 | 11568 | 10223 | 11.6% | 0 | 0 | - | - | 1.000 | 1.000 | 1 | - |
| pdf-pdfjs-jpx | text-heavy | Apache-2.0 | 5451 | 4506 | 17.3% | 1 | 0 | Tiny 1 | - | 1.000 | 1.000 | 0 | - |
| pdf-pdfjs-attachment | text-heavy | Apache-2.0 | 15299 | 14216 | 7.1% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-pdfjs-scan-bad | scanned | Apache-2.0 | 29832 | 18326 | 38.6% | 1 | 0 | Kind 1 | - | 1.000 | 1.000 | 0 | - |
| pdf-pdfjs-annotation-highlight | text-heavy | Apache-2.0 | 92397 | 92130 | 0.3% (original kept) | 0 | 0 | - | - | 1.000 | 1.000 | 9 | - |
| pdf-pdfjs-bomb-giant | scanned | Apache-2.0 | 123242 | 41552 | 66.3% | 0 | 0 | - | - | 1.000 | 1.000 | 1 | - |
| pdf-pyhanko-signed-twice | text-heavy | MIT | 107981 | 60200 | 44.2% | 0 | 0 | - | - | 1.000 | 1.000 | 5 | - |
| pdf-pyhanko-certified | text-heavy | MIT | 13211 | 4025 | 69.5% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-pyhanko-pades-lta | text-heavy | MIT | 46884 | 13748 | 70.7% | 0 | 0 | - | - | 1.000 | 1.000 | 1 | - |
| pdf-pdfbox-acroform-rotation | forms | Apache-2.0 | 42637 | 17584 | 58.8% | 0 | 0 | - | - | 1.000 | 1.000 | 3 | - |
| pdf-pdfassoc-simple-20 | text-heavy | CC-BY-SA-4.0 | 5211 | 3936 | 24.5% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-irs-f1040-2022 | forms | Public-Domain | 156045 | 123855 | 20.6% | 0 | 0 | - | - | 1.000 | 1.000 | 28 | - |
| pdf-irs-p17-2022 | text-heavy | Public-Domain | 2921289 | 2796638 | 4.3% | 1 | 0 | Tiny 1 | - | 1.000 | 1.000 | 1009 | - |
| pdf-nist-sp800-53r5 | text-heavy | Public-Domain | 6073678 | 5159830 | 15.0% | 14 | 0 | Graphic 2, Tiny 12 | - | 1.000 | 1.000 | 1096 | - |
| pdf-commons-ia-scan-public-lands | scanned | Public-Domain | 3880663 | 3827070 | 1.4% (original kept) | 228 | 0 | Kind 223, Tiny 5 | - | 1.000 | 1.000 | 102 | - |
| pdf-commons-ia-scan-mining-claims | scanned | Public-Domain | 6526211 | 6516205 | 0.2% (original kept) | 96 | 0 | Kind 95, Tiny 1 | - | 1.000 | 1.000 | 38 | - |
| pdf-commons-arabic-book | scanned | Public-Domain | 1197732 | 1171225 | 2.2% | 63 | 0 | Kind 62, Tiny 1 | - | 1.000 | 1.000 | 7 | - |
| pdf-commons-arabic-primer-wdl | scanned | Public-Domain | 5206646 | 5195075 | 0.2% (original kept) | 169 | 0 | Kind 120, Tiny 49 | - | 1.000 | 1.000 | 25 | - |
| pdf-commons-hebrew-parable | image-heavy | CC-BY-SA-4.0 | 467005 | 445381 | 4.6% | 6 | 3 | NoGain 3 | 0.972 | 0.998 | 0.999 | 110 | - |
| pdf-gen-mixed-sizes-rotations | text-heavy | MIT OR Apache-2.0 | 2949 | 2059 | 30.2% | 0 | 0 | - | - | 1.000 | 1.000 | 0 | - |
| pdf-gen-3000-pages | text-heavy | MIT OR Apache-2.0 | 1074148 | 558405 | 48.0% | 0 | 0 | - | - | 1.000 | 1.000 | 93 | - |
| pdf-gen-truncated | text-heavy | MIT OR Apache-2.0 | 21337 | 18693 | 12.4% | 0 | 0 | - | - | - | - | 3 | - |
| pdf-gen-no-xref | text-heavy | MIT OR Apache-2.0 | 37016 | 32203 | 13.0% | 0 | 0 | - | - | - | - | 4 | - |
| pdf-gen-junk-prefix-bitflip | text-heavy | MIT OR Apache-2.0 | 39084 | 32283 | 17.4% | 0 | 0 | - | - | 1.000 | 1.000 | 4 | - |
| pdf-gen-scanned-skewed | scanned | MIT OR Apache-2.0 | 280391 | 279904 | 0.2% (original kept) | 4 | 0 | NoGain 4 | - | 1.000 | 1.000 | 54 | - |

## Summary

- 35 files, 33,549,463 bytes in, 28,636,754 bytes out (counting kept originals at their own size): 14.6% smaller
  overall. Median reduction 17.3%, mean 26.2%.
- The median is flattered by tiny test files, where most of the saving is object-stream packing and a better
  Flate level on a few KB. For the 15 files of 100 KB or more the median is 9.0%.
- By class (median reduction): forms 21.4% (5 files), text-heavy 16.4% (20), image-heavy 28.1% (2), scanned
  1.8% (8).
- No output is larger than its input. The page SSIM floor across all drawn comparisons is 0.994.
- The 300 MB generated file (300 pages of photographic page images): 314,706,042 to 136,866,468 bytes (56.5%
  smaller) in 5.1 s, all 300 pictures recoded, lowest image SSIM 0.971, lowest page SSIM 0.993. Peak memory
  11 MB.

## Files where we lose

None gets bigger, but these gain under 2% and are kept as they were:

| file | why |
|---|---|
| pdf-commons-ia-scan-public-lands | pictures are JPEG 2000 (background) and JBIG2 (text mask). The engine does not decode either, so it cannot recode them. |
| pdf-commons-ia-scan-mining-claims | same family (JPEG 2000 and JBIG2) |
| pdf-commons-arabic-primer-wdl | pictures of kinds the engine does not recode, plus 49 tiny ones |
| pdf-gen-scanned-skewed | the 4 pictures are already JPEG, and no setting both holds SSIM 0.97 and gets smaller |
| pdf-qpdf-damaged-stream | no pictures, little structure to compact |
| pdf-pdfjs-annotation-highlight | no pictures, little structure to compact |

Update 2026-10-14: this gap is closed for the three scans, see "JPEG 2000 and JBIG2 scans" below. The table above
is the Phase 1 state.

This is the real gap of Phase 1. Scanned PDFs made of JPEG 2000 or JBIG2 are the most common "please make
this smaller" input, and a tool that re-encodes those pictures will beat us on them. Closing it needs a JPEG 2000
decoder and a JBIG2 generic-region encoder (spec section 10 allows generic-region JBIG2 only, never symbol
matching). CCITT and 1-bit pictures are also left alone. The fax sample still went down 86% because three of its
pictures are recoded.

## JPEG 2000 and JBIG2 scans

Measured on 2026-10-14, same machine, same setting ("smallest that still looks good"). "Before" is the same build
with pdfium made unavailable (`PANOPLY_PDFIUM_LIB` pointing nowhere), which leaves JPEG 2000 and JBIG2 pictures
alone exactly as Phase 1 did and reproduces its sizes byte for byte.

### What changed

- pdfium decodes the pictures. A one-page PDF that holds only the stream is opened and the bitmap copied out
  (`compress/engine_decode.rs`). No decoder was added, no OpenJPEG, nothing LGPL or GPL. pdfium is downloaded once,
  as for drawing, and without it the pictures are left alone and counted as `NoEngine`.
- A JPEG 2000 picture shown much smaller than it is stored is decoded at a reduced size. pdfium skips wavelet levels
  when a page is drawn small: decoding 2728 x 3564 takes 99 ms whole, 28 ms at a half or a third, 8 ms at a quarter.
  That is also what keeps memory flat (decoding the 7.7 million pixel scans of mining-claims whole peaked at 395 MB in a first try).
- Gray and colour pictures become JPEG as before (SSIM 0.97, at most 150 dpi).
- Scans are split into layers: a low-resolution background, a colour layer, and a 1-bit stencil mask of the glyphs
  that paints the colour layer. The colour layer is only seen through the mask, so it is scored there: stored at 1/8
  to 1/2 of the page resolution (smallest that holds), stretched back bilinearly, laid over a plain sheet through the
  mask, SSIM 0.97 on each colour channel in the windows the mask touches. Scoring the bare layer was tried first and
  fails every scale (SSIM 0.80 to 0.95), because it scores smear that never reaches the page. The masks are never touched.
- 1-bit pictures stored raw or with Flate become CCITT Group 4 (the MIT `fax` crate, already in the tree through
  `tiff`; no encoder had to be written) when 5% smaller, and every stream is decoded again and compared row by row.
- JBIG2 pictures are kept. Group 4 was larger on all 139 JBIG2 pictures in the three scans: 6.1 times larger
  than the stored bytes on public-lands (3,917,892 against 641,461), 3.9 times on mining-claims (1,578,071 against
  405,729), 1.2 times on the primer (1,431,619 against 1,194,627). JBIG2 is not even decoded for its own sake, only
  when it is the mask of a colour layer.
- Never inflate, the 2% rule, cancellation and page-by-page streaming are unchanged. Explicit `/Mask` stencils no longer
  block lossy recoding of the picture under them (colour key masks still do).

### The three losing files

| file | before | after | saved | recoded | lowest image SSIM | lowest page SSIM (100 dpi) | mean page SSIM |
|---|---:|---:|---:|---:|---:|---:|---:|
| pdf-commons-ia-scan-public-lands | 3,880,663 | 1,466,950 | 62.2% (was 1.4%, kept) | 74 of 228 | 0.974 | 0.999 | 0.999 |
| pdf-commons-ia-scan-mining-claims | 6,526,211 | 2,788,376 | 57.3% (was 0.2%, kept) | 64 of 96 | 0.970 | 0.993 | 0.995 |
| pdf-commons-arabic-primer-wdl | 5,206,646 | 4,782,062 | 8.2% (was 0.2%, kept) | 42 of 169 | 0.971 | 0.981 | 0.990 |

Page SSIM is up to 6 pages spread through the file, pdfium at 100 dpi. Mean colour shift of a drawn page (poppler, 50
dpi, pages 1, 5, 10, 20): at most 0.7 levels of 255 on public-lands, 3.1 on mining-claims, 4.3 on the primer. The
SSIM gate looks at luma only, so the colour shift is checked separately and is small.

Time and memory, `compress_cli`, peak is the process maximum resident size (it includes the pdfium library):

| file | ms before | ms after (quiet machine) | ms after (machine under load) | peak before | peak after |
|---|---:|---:|---:|---:|---:|
| public-lands (76 pages) | 33 | 6,800 | 22,700 | 9 MB | 128 MB |
| mining-claims (32 pages) | 10 | 3,300 | 4,700 | 9 MB | 130 MB |
| arabic-primer (42 pages) | 5 | 1,700 | 1,600 | 9 MB | 70 MB |

The machine was busy with other jobs during the final run (load average 4 to 6), so the second time column is the
honest worst case and the first was taken when it was quiet. About 90 ms per page goes to decoding the mask and the colour
layer in pdfium and scoring the candidates.

### Why the primer gains only 8%

Most of its bytes are the page pictures, 675 x 954 JPEG 2000 at about 1.6 bits per pixel on a paper texture. JPEG needs
quality 85 to 92 to reach SSIM 0.97 against them and comes out 25 to 100% larger, so they stay. Its 100 dpi colour layers
under 1-bit soft masks are recoded (42 pictures). Smooth backgrounds of the two Internet Archive scans stay too: JPEG
needs 0.25 bits per pixel or more for them at any quality, JPEG 2000 stores them in 0.03 to 0.07, so a bits-per-pixel
check skips them without decoding. Beating those needs a JPEG 2000 or JBIG2 encoder, not a better recoder.

### Whole corpus, nothing regresses

Same 35 files as above, same setting.

- 32 of 35 files are byte for byte the same as before. The three that changed are the three above.
- No output is larger than its input. The same three files are kept at under 2%: qpdf-damaged-stream,
  pdfjs-annotation-highlight, gen-scanned-skewed.
- Median saving 17.3% to 20.6%, mean 26.2% to 29.8%, 33,549,463 bytes in to 22,135,792 out (34.0% smaller, was 14.6%).
  For the 15 files of 100 KB or more the median goes from 9.0% to 20.6%. Scanned class median 1.8% to 48.0%.
- Page SSIM floor across the corpus is now 0.981 (primer), was 0.994.
- Peak memory across the corpus 130 MB, was 31 MB. Under the 300 MB cap. The 300 MB generated file is unchanged:
  314,706,042 to 136,866,468 bytes, 5.4 s, lowest page SSIM 0.993.

### Under a size

Target "under 1 MB", the three scans: public-lands 1,852,702 (not reached, 46 s on the loaded machine), mining-claims
651,297 (90.0% smaller, 10 s, lowest page SSIM 0.981), primer 1,478,565 (71.6% smaller, lowest page SSIM 0.924,
not reached). The harsher rungs go below the SSIM floor by design. Two known rough edges: size mode decodes each
picture again on every pass, which is why public-lands takes 46 s there, and the report says "not reached" for
mining-claims although the file ends under 1 MB, because the confirming passes did not land under it before the final rung
was chosen (this logic is unchanged from Phase 1).

### Limits that remain

- CCITT pictures and JBIG2 pictures are not recoded. A JPEG 2000 picture that is neither oversampled nor under a mask stays
  JPEG 2000 when JPEG would be bigger.
- ICC-based, palette, CMYK and `/Decode` JPEG 2000 pictures, and ones with an alpha channel, are left alone.
- JPEG 2000 pictures that decode to more than 5 million pixels at the size asked for, and JBIG2 pictures over 48 million
  pixels, are left alone (memory).
- A colour layer is scored over a white sheet for dark ink and a black one for light ink, which is a stand-in for the
  real background layer, not the page itself.

## Under a size

Same corpus, target "under 1 MB". Files already under 1 MB are written with pictures untouched and the
structure compacted (no pixel changes). Rung 0 of the size ladder is the SSIM 0.97 setting; harsher rungs
lower resolution and JPEG quality to reach the target and can go below that floor (image-streams reaches a
lowest image SSIM of 0.955). The size mode trades quality for size because the person asked it to, and the job notes
name the setting used. Files over 1 MB:

| file | class | before | after | saved | images | recoded | images left alone | lowest image SSIM | lowest page SSIM | mean page SSIM | ms | target met |
|---|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-image-streams | image-heavy | 2970696 | 1433023 | 51.8% | 9 | 5 | Kind 4 | 0.955 | 0.988 | 0.998 | 68 | no |
| pdf-qpdf-fax-ccitt | scanned | 1565966 | 218281 | 86.1% | 8 | 3 | Graphic 1, Kind 2, Tiny 2 | 0.970 | 0.994 | 0.998 | 471 | yes |
| pdf-irs-p17-2022 | text-heavy | 2921289 | 2796638 | 4.3% | 1 | 0 | Tiny 1 | - | 1.000 | 1.000 | 2675 | no |
| pdf-nist-sp800-53r5 | text-heavy | 6073678 | 5159830 | 15.0% | 14 | 0 | Graphic 2, Tiny 12 | - | 1.000 | 1.000 | 3089 | no |
| pdf-commons-ia-scan-public-lands | scanned | 3880663 | 3827070 | 1.4% (original kept) | 228 | 0 | Kind 223, Tiny 5 | - | 1.000 | 1.000 | 133 | no |
| pdf-commons-ia-scan-mining-claims | scanned | 6526211 | 6516205 | 0.2% (original kept) | 96 | 0 | Kind 95, Tiny 1 | - | 1.000 | 1.000 | 73 | no |
| pdf-commons-arabic-book | scanned | 1197732 | 1171225 | 2.2% | 63 | 0 | Kind 62, Tiny 1 | - | 1.000 | 1.000 | 13 | no |
| pdf-commons-arabic-primer-wdl | scanned | 5206646 | 5195075 | 0.2% (original kept) | 169 | 0 | Kind 120, Tiny 49 | - | 1.000 | 1.000 | 41 | no |
| pdf-gen-3000-pages | text-heavy | 1074148 | 558405 | 48.0% | 0 | 0 | - | - | 1.000 | 1.000 | 154 | yes |

The target was met on 28 of 35 files. It was not met where the bytes are pictures the engine cannot recode
(the scans above), or text and fonts with nothing left to shrink (the IRS and NIST documents). In those cases the
module says so and writes the smallest file it reached: "It could not reach the size you asked for without
removing pictures."

## qpdf baseline

qpdf is not installed on this machine, so no baseline was run. It is not a dependency. To add one, install it
and compare against `qpdf --object-streams=generate --recompress-flate --compression-level=9 in.pdf out.pdf`,
which is the structural part of what we do without the picture work.

## Candidates for an iLovePDF comparison

Twenty corpus files, varied, all with a clear license in `tests/corpus/pdf.toml` (public domain, Apache-2.0,
MIT, or CC-BY-SA noted). Pick the set as it is, or drop the signed ones, since a web tool will break the
signature.

| class | file | size | note |
|---|---|---|---|
| scanned | pdf-commons-ia-scan-public-lands | 3.9 MB | JPEG 2000 plus JBIG2, 228 pictures. The case we lose |
| scanned | pdf-commons-ia-scan-mining-claims | 6.5 MB | same family, 96 pictures |
| scanned | pdf-commons-arabic-primer-wdl | 5.2 MB | scan, 169 pictures, RTL |
| scanned | pdf-commons-arabic-book | 1.2 MB | scan, 63 pictures, RTL |
| scanned | pdf-qpdf-fax-ccitt | 1.6 MB | CCITT fax pages. We win 86% here |
| scanned | pdf-gen-scanned-skewed | 280 KB | synthetic skewed scan |
| image-heavy | pdf-qpdf-image-streams | 3.0 MB | mixed image encodings. We win 52% |
| image-heavy | pdf-commons-hebrew-parable | 467 KB | picture pages, CC-BY-SA |
| image-heavy | pdf-pdfjs-cmyk-jpeg | 374 KB | CMYK JPEG, not recoded by us |
| image-heavy | pdf-gen-large-300mb | 315 MB | too big for most web tools, use as a speed point only |
| text-heavy | pdf-nist-sp800-53r5 | 6.1 MB | 492 pages, tagged, outline |
| text-heavy | pdf-irs-p17-2022 | 2.9 MB | 138 pages of text and tables |
| text-heavy | pdf-gen-3000-pages | 1.1 MB | 3,000 short pages |
| text-heavy | pdf-pdfjs-arabic-cid-truetype | 39 KB | embedded CID TrueType font |
| text-heavy | pdf-pdfjs-annotation-highlight | 92 KB | annotations |
| forms | pdf-irs-f1040-2022 | 156 KB | fillable government form |
| forms | pdf-qpdf-form-filled-acrobat | 62 KB | filled AcroForm |
| forms | pdf-pdfbox-acroform-rotation | 43 KB | rotated AcroForm |
| mixed | pdf-qpdf-digitally-signed | 69 KB | signed text and graphics (the signature breaks on any tool) |
| mixed | pdf-pyhanko-signed-twice | 108 KB | two signatures (same note) |

Gap to know about: the corpus has few photo-heavy documents of the kind people actually shrink (slide decks,
phone-camera scans exported as JPEG, brochures). Only `pdf-qpdf-image-streams`, `pdf-commons-hebrew-parable` and
the generated 300 MB file exercise the JPEG quality gate. A fair comparison on that class needs two or three more
such files added to `tests/corpus/pdf.toml` first.

## Versus iLovePDF

Not measured yet. The run on 2026-10-14 was blocked: the built-in browser pane denies local network access, so
the corpus PDFs could not be handed to the iLovePDF page from a local helper, and no file was uploaded. The
Panoply sizes for the 19 candidate files, the helper and an SSIM tool (`crates/panoply-pdf/examples/compare_pdfs.rs`)
are ready in `prototypes/ilovepdf-compare/`. See `prototypes/ilovepdf-compare/RESULT.md` for the blocker and the two
ways to unblock it. The Phase 0 gate (beat iLovePDF's Recommended setting on size at equal visual quality) is
undecided until that run completes.
