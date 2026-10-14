# Beta smoke matrix

Every registered module (built-in and the example WebAssembly modules) over real corpus files, through the path the desktop app takes: the catalog, the options form built from the manifest with its default values, the file sniffed from its bytes, `jobs::run_blocking`, and the result card text from `Done::headline`. No window is opened.

- Commit `071e6d8`, 2026-10-14, macos aarch64 with 18 CPUs (Apple M5 Max), debug build.
- 33 modules, 474 jobs: **297 passed, 53 passed with no change, 124 declined in plain words, 0 failed**, 31 undocumented budget violations.
- Pass: the job wrote what the manifest promises. Pass with no change: nothing needed doing and the result card says why (already the target format, a report). Declined: the module refused with a plain sentence and a next step, because the file needs a choice from the person or is a variant it cannot read yet. Those are listed under Known gaps. Fail: a crash, a hang, a wrong or empty output, or an error with no next step.
- Up to 2 real files per accepted file type, plus one batch job per module. Each job runs in its own process under `/usr/bin/time`, so peak memory is that job's own high-water mark (peak footprint on macOS, resident set on Linux), and it includes the registry the process builds.
- Refused on purpose and not in this matrix: encrypted, truncated and broken files. The host's corpus tests check that those fail in plain words.

## Summary

| Module | Pattern | Source | Jobs | Pass | No change | Declined | Fail | Budget flags | Median job ms | Worst job ms | Worst peak MB |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `files.hash` | I | built in | 49 | 49 | 0 | 0 | 0 | 0 | 1 | 8 | 6 |
| `id.passport_photo` | D | built in | 18 | 0 | 0 | 18 | 0 | 0 | 0 | 0 | 697 |
| `id.print_sheet` | B | built in | 18 | 14 | 0 | 4 | 0 | 3 | 173 | 3528 | 792 |
| `id.signature_cleaner` | D | built in | 17 | 13 | 0 | 4 | 0 | 2 | 225 | 1827 | 373 |
| `image.compress` | B | built in | 17 | 11 | 0 | 6 | 0 | 3 | 1400 | 16939 | 710 |
| `image.convert` | A | built in | 17 | 11 | 0 | 6 | 0 | 2 | 123 | 2134 | 763 |
| `image.erase` | C | built in | 18 | 11 | 0 | 7 | 0 | 6 | 573 | 11482 | 1300 |
| `image.remove_background` | B | built in | 17 | 11 | 0 | 6 | 0 | 0 | 915 | 4651 | 1222 |
| `image.remove_watermark` | C | built in | 18 | 3 | 0 | 15 | 0 | 3 | 2424 | 8188 | 1542 |
| `image.resize` | B | built in | 17 | 11 | 0 | 6 | 0 | 2 | 158 | 925 | 355 |
| `image.resize_to_kb` | B | built in | 17 | 11 | 0 | 6 | 0 | 2 | 311 | 5007 | 696 |
| `image.strip_metadata` | I | built in | 17 | 13 | 0 | 4 | 0 | 0 | 1 | 16 | 29 |
| `image.to_text` | A | built in | 17 | 0 | 14 | 3 | 0 | 3 | 375 | 3041 | 1101 |
| `image.upscale` | B | built in | 17 | 7 | 0 | 10 | 0 | 1 | 761 | 2531 | 710 |
| `media.compress` | B | built in | 16 | 16 | 0 | 0 | 0 | 0 | 1033 | 6471 | 6 |
| `media.convert` | A | built in | 16 | 12 | 4 | 0 | 0 | 0 | 131 | 821 | 5 |
| `media.extract_audio` | A | built in | 16 | 12 | 0 | 4 | 0 | 0 | 92 | 110 | 5 |
| `media.hdr_to_sdr` | A | built in | 7 | 0 | 7 | 0 | 0 | 0 | 36 | 130 | 5 |
| `media.info` | I | built in | 16 | 16 | 0 | 0 | 0 | 0 | 36 | 252 | 5 |
| `media.normalize` | A | built in | 16 | 11 | 0 | 5 | 0 | 0 | 565 | 6859 | 8 |
| `media.repair` | A | built in | 16 | 12 | 4 | 0 | 0 | 0 | 181 | 1493 | 9 |
| `media.streams` | C | built in | 16 | 0 | 16 | 0 | 0 | 0 | 38 | 168 | 5 |
| `media.to_mono` | A | built in | 16 | 7 | 4 | 5 | 0 | 0 | 332 | 1739 | 8 |
| `media.trim` | C | built in | 17 | 16 | 0 | 1 | 0 | 0 | 112 | 1356 | 11 |
| `pdf.compress` | B | built in | 3 | 3 | 0 | 0 | 0 | 0 | 5901 | 5970 | 126 |
| `pdf.images_to_pdf` | A | built in | 5 | 5 | 0 | 0 | 0 | 0 | 6 | 7 | 10 |
| `pdf.merge` | A | built in | 3 | 3 | 0 | 0 | 0 | 0 | 38 | 52 | 5 |
| `pdf.ocr` | B | built in | 19 | 2 | 4 | 13 | 0 | 4 | 482 | 2127 | 663 |
| `pdf.organize` | C | built in | 4 | 3 | 0 | 1 | 0 | 0 | 39 | 46 | 5 |
| `pdf.split` | A | built in | 3 | 3 | 0 | 0 | 0 | 0 | 176 | 188 | 6 |
| `pdf.to_images` | A | built in | 3 | 3 | 0 | 0 | 0 | 0 | 2232 | 3204 | 147 |
| `text.slugify_lines` | A | WASM example | 4 | 4 | 0 | 0 | 0 | 0 | 14 | 18 | 11 |
| `text.word_count` | I | WASM example | 4 | 4 | 0 | 0 | 0 | 0 | 10 | 11 | 10 |

## Failures and budget violations

| Module | Files | Problem |
|---|---|---|
| `id.print_sheet` | img-avif-gainmap-avif | peak memory 336 MB is over the 300 MB cap |
| `id.print_sheet` | img-heic-rotated-cw90 | peak memory 792 MB is over the 300 MB cap |
| `id.print_sheet` | img-raw-pixel-7-pro-dng | peak memory 360 MB is over the 300 MB cap |
| `id.signature_cleaner` | img-heic-rotated-cw90 | peak memory 373 MB is over the 300 MB cap |
| `id.signature_cleaner` | img-raw-pixel-7-pro-dng | peak memory 356 MB is over the 300 MB cap |
| `image.compress` | img-raw-canon-7d-sraw | peak memory 494 MB is over the 300 MB cap |
| `image.compress` | img-heic-rotated-cw90 | peak memory 710 MB is over the 300 MB cap |
| `image.compress` | img-raw-pixel-7-pro-dng | peak memory 649 MB is over the 300 MB cap |
| `image.convert` | img-heic-rotated-cw90 | peak memory 763 MB is over the 300 MB cap |
| `image.convert` | img-raw-pixel-7-pro-dng | peak memory 400 MB is over the 300 MB cap |
| `image.erase` | img-avif-gainmap-avif | peak memory 771 MB is over the 600 MB cap |
| `image.erase` | img-raw-canon-7d-sraw | peak memory 660 MB is over the 600 MB cap |
| `image.erase` | img-heic-rotated-cw90 | peak memory 1300 MB is over the 600 MB cap |
| `image.erase` | img-commons-wide-nasa | peak memory 1028 MB is over the 600 MB cap |
| `image.erase` | img-raw-pixel-7-pro-dng | peak memory 937 MB is over the 600 MB cap |
| `image.erase` | img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | peak memory 1158 MB is over the 600 MB cap |
| `image.remove_watermark` | img-raw-canon-7d-sraw | peak memory 787 MB is over the 300 MB cap |
| `image.remove_watermark` | img-heic-rotated-cw90 | peak memory 1542 MB is over the 300 MB cap |
| `image.remove_watermark` | img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | peak memory 909 MB is over the 300 MB cap |
| `image.resize` | img-heic-rotated-cw90 | peak memory 308 MB is over the 300 MB cap |
| `image.resize` | img-raw-pixel-7-pro-dng | peak memory 355 MB is over the 300 MB cap |
| `image.resize_to_kb` | img-heic-rotated-cw90 | peak memory 696 MB is over the 300 MB cap |
| `image.resize_to_kb` | img-raw-pixel-7-pro-dng | peak memory 359 MB is over the 300 MB cap |
| `image.to_text` | img-avif-gainmap-avif | peak memory 380 MB is over the 300 MB cap |
| `image.to_text` | img-heic-rotated-cw90 | peak memory 1101 MB is over the 300 MB cap |
| `image.to_text` | img-commons-wide-nasa | peak memory 704 MB is over the 300 MB cap |
| `image.upscale` | img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | peak memory 313 MB is over the 300 MB cap |
| `pdf.ocr` | img-commons-wide-nasa | peak memory 663 MB is over the 300 MB cap |
| `pdf.ocr` | pdf-qpdf-inherited-rotate | peak memory 385 MB is over the 300 MB cap |
| `pdf.ocr` | pdf-commons-ia-scan-mining-claims | peak memory 350 MB is over the 300 MB cap |
| `pdf.ocr` | pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | peak memory 566 MB is over the 300 MB cap |

## Known gaps (declined in plain words)

Each of these is a refusal the app shows on its result card, with the next step. They are not crashes, but they are places where a person's drop and Return does not finish.

| Module | What it says | Files |
|---|---|---|
| `id.passport_photo` | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. Check the file and the options, then try again. | img-avif-animated-8bpc, img-avif-gainmap-avif, img-raw-canon-7d-sraw, img-vips-gif-dispose-previous, img-commons-gif-horse, img-heic-unsupported-depth, img-heic-rotated-cw90, img-vips-ico, img-vips-jp2, img-commons-wide-nasa, img-pngsuite-odd-size-palette, img-avif-png-icc-exif-xmp, img-raw-pixel-7-pro-dng, img-commons-animated-webp |
| `id.passport_photo` | Say which country's rule to use, for example country=IN or country=US. Check the file and the options, then try again. | img-avif-animated-8bpc |
| `id.passport_photo` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic, img-turbo-arithmetic+img-exifori-landscape-8+img-commons-wide-nasa |
| `id.passport_photo` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `id.print_sheet` | Panoply does not know how big to print this photo, and it will not guess. Say the size with size=35x45 (millimetres), or take it from a country rule with rule=in-passport. Check the file and the options, then try again. | img-avif-animated-8bpc |
| `id.print_sheet` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic, img-turbo-arithmetic+img-exifori-landscape-8+img-commons-wide-nasa |
| `id.print_sheet` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `id.signature_cleaner` | No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame. Check the file and the options, then try again. | img-heic-unsupported-depth, img-pngsuite-odd-size-palette |
| `id.signature_cleaner` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `id.signature_cleaner` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.compress` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.compress` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.compress` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.convert` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.convert` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.convert` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.erase` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.erase` | Say what to erase: give a mask picture as the second file (white is erased), or set rect to a rectangle such as 100,200,300,80. Check the file and the options, then try again. | img-avif-animated-8bpc |
| `image.erase` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.erase` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.remove_background` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.remove_background` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.remove_background` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.remove_watermark` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.remove_watermark` | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). Check the file and the options, then try again. | img-avif-gainmap-avif, img-heic-unsupported-depth, img-vips-ico, img-vips-jp2, img-commons-wide-nasa, img-pngsuite-odd-size-palette, img-avif-png-icc-exif-xmp, img-raw-pixel-7-pro-dng |
| `image.remove_watermark` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.remove_watermark` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.remove_watermark` | This tool is for pictures you own or have the right to edit: your own photos with a date stamp, your own exports with a logo you added, your own AI images with a visible badge. Set own_content to true to confirm that, then run it again. Check the file and the options, then try again. | img-avif-animated-8bpc |
| `image.resize` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.resize` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.resize` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.resize_to_kb` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.resize_to_kb` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.resize_to_kb` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.strip_metadata` | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. Check the file and the options, then try again. | img-vips-gif-dispose-previous, img-commons-gif-horse, img-vips-ico, img-vips-jp2 |
| `image.to_text` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.to_text` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.to_text` | testimgari.jpg: This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic+img-exifori-landscape-8+img-commons-wide-nasa |
| `image.upscale` | ... is 4,672 x 7,008, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. Check the file and the options, then try again. | img-heic-rotated-cw90 |
| `image.upscale` | ... is 9,000 x 3,500, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. Check the file and the options, then try again. | img-commons-wide-nasa |
| `image.upscale` | ... is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. Check the file and the options, then try again. | img-avif-animated-8bpc, img-vips-gif-dispose-previous, img-commons-gif-horse, img-commons-animated-webp |
| `image.upscale` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `image.upscale` | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. Check the file and the options, then try again. | img-vips-tiff-subsampled |
| `image.upscale` | This picture is already 2,592 x 1,728. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. Check the file and the options, then try again. | img-raw-canon-7d-sraw |
| `image.upscale` | This picture is already 3,866 x 2,908. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. Check the file and the options, then try again. | img-raw-pixel-7-pro-dng |
| `media.extract_audio` | ... is the one you meant. Check the file and the options, then try again. | vid-bbb-360-h264, vid-bbb-1080-h264, vid-bbb-720-vp9, vid-bbb-360-h264+vid-sintel-720-h264+vid-bbb-1080-h264 |
| `media.normalize` | ... is the one you meant. Check the file and the options, then try again. | vid-bbb-360-h264, vid-bbb-1080-h264, vid-bbb-720-vp9, vid-bbb-360-h264+vid-sintel-720-h264+vid-bbb-1080-h264 |
| `media.normalize` | camera-clip-mvi0131.ogv holds a kind of video or audio that this format cannot carry. Pick another format such as MKV. Check the file and the options, then try again. | vid-commons-camera-ogv |
| `media.to_mono` | ... is the one you meant. Check the file and the options, then try again. | vid-bbb-360-h264, vid-bbb-1080-h264, vid-bbb-720-vp9, vid-bbb-360-h264+vid-sintel-720-h264+vid-bbb-1080-h264 |
| `media.to_mono` | camera-clip-mvi0131.ogv holds a kind of video or audio that this format cannot carry. Pick another format such as MKV. Check the file and the options, then try again. | vid-commons-camera-ogv |
| `media.trim` | Say what to keep, for example 0:12-1:40, or what to remove with the remove option Check the file and the options, then try again. | aud-commons-lachmoeweisma-flac |
| `pdf.ocr` | ... is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. Check the file and the options, then try again. | img-avif-animated-8bpc, img-avif-gainmap-avif, img-raw-canon-7d-sraw, img-vips-gif-dispose-previous, img-commons-gif-horse, img-heic-unsupported-depth, img-heic-rotated-cw90, img-vips-ico, img-vips-jp2, img-vips-tiff-subsampled, img-raw-pixel-7-pro-dng, img-commons-animated-webp |
| `pdf.ocr` | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. Check the file and the options, then try again. | img-turbo-arithmetic |
| `pdf.organize` | Nothing to do. Say which pages to keep or delete, a rotation, or turn on reverse. Check the file and the options, then try again. | pdf-qpdf-inherited-rotate |

## Budget exceptions in use

These are written down in DECISIONS.md ("Exceptions to the budgets") and on the Budgets line of the module's spec. The matrix honours only these:

| Module | Cap | Source |
|---|---|---|
| `image.remove_background` | 2000 MB peak, no 100 ms first preview | id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP |
| `image.erase` | 600 MB peak, no 100 ms first preview | id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s |
| `image.upscale` | no 100 ms first preview | id-and-image.md 3.11 Budgets: time and model size, preview tile in seconds |
| `image.convert` | 400 MB peak | id-and-image.md 3.1 Budgets: RAW development is about 8 bytes per pixel, 45 MP is about 360 MB |

Jobs in this run that needed an exception:

- `image.erase` on img-heic-unsupported-depth: peak memory 580 MB, above 300 MB: id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s
- `image.erase` on img-vips-ico: peak memory 575 MB, above 300 MB: id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s
- `image.erase` on img-vips-jp2: peak memory 596 MB, above 300 MB: id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s
- `image.erase` on img-pngsuite-odd-size-palette: peak memory 572 MB, above 300 MB: id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s
- `image.erase` on img-avif-png-icc-exif-xmp: peak memory 580 MB, above 300 MB: id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s
- `image.remove_background` on img-avif-gainmap-avif: peak memory 848 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-raw-canon-7d-sraw: peak memory 334 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-heic-unsupported-depth: peak memory 789 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-heic-rotated-cw90: peak memory 1222 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-commons-wide-nasa: peak memory 616 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-pngsuite-odd-size-palette: peak memory 789 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-avif-png-icc-exif-xmp: peak memory 799 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-raw-pixel-7-pro-dng: peak memory 496 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP
- `image.remove_background` on img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa: peak memory 1087 MB, above 300 MB: id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP

## Full table

Job ms is the time inside the job (the worker thread, as the result card measures it). Process ms is the whole child, with process start and the registry. Peak MB is the child's high-water mark.

### `files.hash` (Hash files, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-vips-exr |  | 378.6 KB | PASS | 1 | 20 | 5 | report | dd7355a2c2a694bae041500af4f61ae96bdc02a6e9474fa53d4e4ccef83c8774  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-0/sample.exr |
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 0 | 18 | 5 | report | 2f8683d21725261f37f86e115f0c212cc52d0fefd3a2ddfcc4fa648c1859906d  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-1/colors-animated-8bpc.avif |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 0 | 19 | 5 | report | 9bf9c6a7606951de07e4079cd63c2cfe379d95139cd99ab9142d8a6ee22d28c7  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-2/seine-hdr-gainmap.avif |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 8 | 25 | 6 | report | 3789b1ba5d880613104637c40a06619cd9f3b54b0cf62d2d95b9f9e885edcf6e  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-3/Canon---EOS-7D---sRAW2-sRAW-3x2.CR2 |
| off-poi-docx-rtl | docx | 6.8 KB | PASS | 0 | 19 | 5 | report | d159b6ac6734f14a9ecf8e97ca5029bdcddbfae2d33ba0128aa774924e0b190d  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-4/rtl.docx |
| off-poi-docx-comments | docx | 65.3 KB | PASS | 0 | 19 | 5 | report | 06b63b13f0b78a7179151af52c2282557c1ed204899feee9fcef4ca575344f97  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-5/comments.docx |
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 2 | 18 | 6 | report | cd53a05924b02513cee5ccef00f8b4bc9cc4d90fdc87446202a695823c2af21b  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-6/lachmoeweisma.flac |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 2 | 19 | 6 | report | 0c563965a86b422c695892d0dae07b458d200d8b08f741d1a9d01ead37588337  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-7/glockenweihe.flac |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 0 | 19 | 4 | report | bd0beae04d8688100541bec4cd87c87b51bb8ab2c9b7f257bc20a0e0e2011291  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-8/dispose-previous.gif |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 1 | 19 | 5 | report | b6b2d01837fe70230094010a2bd106d1f6b8e23f1370575e470402987d5a0244  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-9/muybridge-race-horse.gif |
| img-heic-unsupported-depth | heif | 698 B | PASS | 0 | 19 | 5 | report | 0b421065e0ec70dde8dace2d300c4c27d2a49e3d19babd15673d626f3c9d3740  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-10/unsupported-depth.heic |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 4 | 19 | 6 | report | f0d5e88be07b9d70a3715218ca612ccf45b126e5b4b42172dce14a62d2099932  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-11/guitar-rotated-cw90.hif |
| img-vips-ico | ico | 7.4 KB | PASS | 0 | 19 | 5 | report | 461350ef07af2b4376c277ba9cb62a98e535ef6a13760ebfacc82cf95d347acb  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-12/favicon.ico |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 0 | 19 | 5 | report | ac78346a830a6bca8eeb99c56d45571dbbbde2e3994907102ff9a099d0bbad34  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-13/world.jp2 |
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 0 | 19 | 5 | report | 4672c7f08864cd0a8c73a4fa4b66ca32b635d38464551c1ecf06564ae8c89b38  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-14/testimgari.jpg |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 4 | 19 | 6 | report | de3151e7c5c786215428e799da596356120cc8d09adb6c98d5af6a7bee034a08  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-15/day-earth-smiled.jpg |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 1 | 19 | 5 | report | caba82e1205938dd89548539805baf487c7121692922ae528e1e0832f41431cf  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-16/aac-lc-stereo.m4a |
| CONTRIBUTING.md | md | 3.0 KB | PASS | 0 | 19 | 5 | report | 8405c08e4d92e2b65138813c3a95be8a76e9e5ad3cfb501632ff939085ee30d0  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-17/CONTRIBUTING.md |
| docs/FLOWS.md | md | 16.7 KB | PASS | 0 | 19 | 5 | report | 7ff371f35c33f0336cb0de79c5f3486ea506e26076248c59c6f008c40f69000d  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-18/FLOWS.md |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 2 | 19 | 6 | report | f789ec720d4901f80417aecc1ec615eb94f53118c4ec0ba0b065b419e2866933  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-19/multi-audio-3-tracks-subs.mkv |
| vid-gen-prores | mov | 2.9 MB | PASS | 3 | 19 | 6 | report | ff5abdcf98f938be804a2d593948bf0f63dbf14d4e2ac130b20947867f221aae  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-20/prores-proxy-720p.mov |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 1 | 19 | 5 | report | 3f4cd70ef98bbff1e592b4b96f0fa556f94d49da4ec4c3bbc55155684053d43e  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-21/recorder-improvisation.mp3 |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 3 | 18 | 6 | report | 7727db92e45ab2fba3997ecd396d19230a8b6d98208f097ce91729be04cb2910  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-22/stay-down-here-1915.mp3 |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 1 | 18 | 6 | report | 77145c94c11f3754207499158df22406e1fe7635553c1c86dc5e881dfeb32016  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-23/bbb-360p-h264-10s.mp4 |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 7 | 25 | 6 | report | 1bb2a62abfcb5c4bf2e9216b63ba715b358f22c9fa8e68ca7149c4788b906826  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-24/bbb-1080p-h264-10s.mp4 |
| off-poi-doc-legacy | msi | 27.1 KB | PASS | 0 | 19 | 5 | report | 717585c0a88f20862c86220163dd03da87cd743970354c6b29add32725f2e833  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-25/sample.doc |
| off-poi-ppt-legacy | msi | 125.4 KB | PASS | 1 | 18 | 5 | report | 64275a6685b8f267827178037dcc59de927475e56385725916f4c6039c7ce7f6  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-26/sample.ppt |
| off-gen-odp-sample | odp | 1.0 KB | PASS | 0 | 19 | 5 | report | b8012719061d326ba5b6e7547609da994d15397851f745f252902099b595761a  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-27/sample.odp |
| off-poi-ods-sample | ods | 20.4 KB | PASS | 0 | 19 | 5 | report | a551ef35d3faddae4bf135960bde419f4a0140f9398c2218614b089ae8a4b46f  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-28/sample.ods |
| off-gen-odt-sample | odt | 1.0 KB | PASS | 0 | 19 | 5 | report | 26a6325f96f6e3e3c80aedfec03a7777d5996b24213db81be7039101657d9127  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-29/sample.odt |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 1 | 19 | 5 | report | 6ed2068e155e362ac5eaf208e2c3eaf3247af3b63697a514aea8fe64a45f5532  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-30/netaji-1943.ogg |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 2 | 19 | 6 | report | 3eef53a936f0315eec41cb1d20641d2b2289702ad9186ea894505d67d1b12e66  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-31/camera-clip-mvi0131.ogv |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 0 | 18 | 5 | report | a8ac5f3019f070de83323daf7fd2a010efa8d29e588eb08ae817a8f7f1aac129  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-32/speech-opus-7kbps.opus |
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 0 | 17 | 5 | report | 69cc42c34812790a88d8275d45ac9b0a72b9e5ad62aa70427d25caeee4a9f46a  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-33/inherited-rotate.pdf |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 5 | 25 | 6 | report | c0562b7c09948044dab28a65de4b16f59079a715624b2e3a04b6b71aaebc7eef  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-34/ia-scan-mining-claims.pdf |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 0 | 18 | 5 | report | 1b9e244a5c1d6cc9830391fff4eac565ada026d000e60faf7525c8add6bdc726  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-35/s05n3p02.png |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 1 | 19 | 5 | report | 308bb51c3338ed9519cdaaa79186b934a21d1748d94120ed0348eb4f193d0988  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-36/paris-icc-exif-xmp.png |
| off-poi-pptx-sample | pptx | 39.1 KB | PASS | 0 | 19 | 5 | report | bfb4b2f07c9233afd2f32aa5781d849d0c7d225bf03721a10becf487b481d828  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-37/sample.pptx |
| off-poi-pptx-smartart | pptx | 41.4 KB | PASS | 0 | 18 | 5 | report | b97e4c6d2ee1dd4094f50f9043820610268dd452d7717c537f5456636adcc353  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-38/smartart.pptx |
| img-vips-tiff-subsampled | tif | 72.8 KB | PASS | 1 | 19 | 5 | report | 264d4955898a35ae70af0ec6eed0dbc49d7929f6c18c92a4ed6198f6db396f4b  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-39/subsampled.tif |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 8 | 26 | 6 | report | 6b921558944db5fdb4f1c63dc64919b6666ef700857a187e18500099c6e7c6c6  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-40/Google---Pixel-7-Pro---16bit-4x3.dng |
| tests/corpus/pdf.toml | toml | 28.2 KB | PASS | 0 | 18 | 5 | report | e3dddca42bb73a30e492da7e6b973012cccac77ac7af856c9ecdc0ba9fa56c27  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-41/pdf.toml |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 1 | 19 | 5 | report | 322728e6d71288c219919579376112d80cf0d8f5df82212af8c6bf285a4c3988  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-42/alaa1.wav |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 4 | 25 | 6 | report | 5d83087e7fe53aa61db02e159adb10b46411ab75263a95d5814a09e24214a3d4  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-43/phone-clip-vid20240601.webm |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 4 | 26 | 6 | report | f4457bf8cee98b882c659f13883e58ad058d72b80516ecdf3f21b45ef70dd188  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-44/bbb-720p-vp9-10s.webm |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 2 | 24 | 6 | report | 3f24267941a9253f49a7d8ab8401132fe868bcb229618a9970db4d3049303d29  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-45/animated-webp-sample.webp |
| off-poi-xlsx-charts | xlsx | 13.0 KB | PASS | 0 | 19 | 5 | report | 47e9e8dea87494d17dbacce9aa3a9ec31cb2b26b288de05ca95c0a4622ca7f80  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-46/charts.xlsx |
| img-vips-svg | xml | 4.7 KB | PASS | 0 | 18 | 5 | report | 0e2e46c26b8335bdca45a930af59366329e6fa66bf186ffeb4a90f2129c8f664  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-47/logo.svg |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 5 | 25 | 6 | report | 69cc42c34812790a88d8275d45ac9b0a72b9e5ad62aa70427d25caeee4a9f46a  /var/folders/1p/2sjkczk95p1g0v6z8gqr1njc0000gp/T/panoply-smoke-12538-48/inherited-rotate.pdf |

### `id.passport_photo` (Passport photo, pattern D)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed country=IN) | 1.2 KB | DECLINED |  | 309 | 160 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-gainmap-avif | avif (typed country=IN) | 123.0 KB | DECLINED |  | 280 | 216 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-animated-8bpc | defaults only | 1.2 KB | DECLINED |  | 19 | 5 |  | Say which country's rule to use, for example country=IN or country=US. |
| img-raw-canon-7d-sraw | cr2 (typed country=IN) | 11.1 MB | DECLINED |  | 2315 | 260 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-gif-dispose-previous | gif (typed country=IN) | 1.1 KB | DECLINED |  | 143 | 158 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-commons-gif-horse | gif (typed country=IN) | 297.2 KB | DECLINED |  | 145 | 161 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-heic-unsupported-depth | heif (typed country=IN) | 698 B | DECLINED |  | 220 | 159 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-heic-rotated-cw90 | heif (typed country=IN) | 5.2 MB | DECLINED |  | 3184 | 697 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-ico | ico (typed country=IN) | 7.4 KB | DECLINED |  | 132 | 159 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-jp2 | jp2 (typed country=IN) | 64.0 KB | DECLINED |  | 169 | 165 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-turbo-arithmetic | jpg (typed country=IN) | 5.1 KB | DECLINED |  | 13 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed country=IN) | 5.0 MB | DECLINED |  | 855 | 249 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-pngsuite-odd-size-palette | png (typed country=IN) | 129 B | DECLINED |  | 142 | 159 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-avif-png-icc-exif-xmp | png (typed country=IN) | 153.2 KB | DECLINED |  | 154 | 162 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-vips-tiff-subsampled | tif (typed country=IN) | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed country=IN) | 12.2 MB | DECLINED |  | 2941 | 360 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-commons-animated-webp | webp (typed country=IN) | 2.9 MB | DECLINED |  | 250 | 205 |  | No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed country=IN) | 5.4 MB | DECLINED |  | 979 | 273 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `id.print_sheet` (Print sheet, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed size=35x45) | 1.2 KB | PASS | 191 | 209 | 143 | 194.7 KB | 1 KB to 195 KB, 15666% bigger |
| img-avif-gainmap-avif | avif (typed size=35x45) | 123.0 KB | PASS, over budget | 265 | 288 | 336 | 2.5 MB | 123 KB to 2.5 MB, 1947% bigger |
| img-avif-animated-8bpc | defaults only | 1.2 KB | DECLINED |  | 137 | 9 |  | Panoply does not know how big to print this photo, and it will not guess. Say the size with size=35x45 (millimetres), or take it from a country rule with rule=in-passport. |
| img-raw-canon-7d-sraw | cr2 (typed size=35x45) | 11.1 MB | PASS | 732 | 753 | 260 | 3.9 MB | 11 MB to 3.9 MB, 65% smaller |
| img-vips-gif-dispose-previous | gif (typed size=35x45) | 1.1 KB | PASS | 68 | 87 | 139 | 779.0 KB | 1 KB to 779 KB, 71238% bigger |
| img-commons-gif-horse | gif (typed size=35x45) | 297.2 KB | PASS | 85 | 106 | 140 | 1.4 MB | 297 KB to 1.4 MB, 360% bigger |
| img-heic-unsupported-depth | heif (typed size=35x45) | 698 B | PASS | 183 | 200 | 142 | 303.7 KB | 698 bytes to 304 KB, 43408% bigger |
| img-heic-rotated-cw90 | heif (typed size=35x45) | 5.2 MB | PASS, over budget | 3528 | 3573 | 792 | 1.8 MB | 5.2 MB to 1.8 MB, 66% smaller |
| img-vips-ico | ico (typed size=35x45) | 7.4 KB | PASS | 85 | 118 | 140 | 881.0 KB | 7 KB to 881 KB, 11796% bigger |
| img-vips-jp2 | jp2 (typed size=35x45) | 64.0 KB | PASS | 173 | 203 | 146 | 3.0 MB | 64 KB to 3.0 MB, 4558% bigger |
| img-turbo-arithmetic | jpg (typed size=35x45) | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed size=35x45) | 5.0 MB | PASS | 1034 | 1069 | 265 | 1.9 MB | 5.0 MB to 1.9 MB, 62% smaller |
| img-pngsuite-odd-size-palette | png (typed size=35x45) | 129 B | PASS | 80 | 99 | 139 | 1.1 MB | 129 bytes to 1.1 MB, 823481% bigger |
| img-avif-png-icc-exif-xmp | png (typed size=35x45) | 153.2 KB | PASS | 123 | 146 | 143 | 2.5 MB | 153 KB to 2.5 MB, 1526% bigger |
| img-vips-tiff-subsampled | tif (typed size=35x45) | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed size=35x45) | 12.2 MB | PASS, over budget | 1240 | 1265 | 360 | 2.8 MB | 12 MB to 2.8 MB, 77% smaller |
| img-commons-animated-webp | webp (typed size=35x45) | 2.9 MB | PASS | 155 | 178 | 171 | 1.7 MB | 2.9 MB to 1.7 MB, 43% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed size=35x45) | 5.4 MB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `id.signature_cleaner` (Signature cleaner, pattern D)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 142 | 157 | 10 | 150 B | 1 KB to 150 bytes, 88% smaller |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 225 | 249 | 206 | 26.8 KB | 123 KB to 27 KB, 78% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 1426 | 1448 | 274 | 156.7 KB | 11 MB to 157 KB, 99% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS | 7 | 25 | 6 | 562 B | 1 KB to 562 bytes, 49% smaller |
| img-commons-gif-horse | gif | 297.2 KB | PASS | 23 | 37 | 7 | 5.0 KB | 297 KB to 5 KB, 98% smaller |
| img-heic-unsupported-depth | heif | 698 B | DECLINED |  | 147 | 11 |  | No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame. |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS, over budget | 1072 | 1095 | 373 | 27.3 KB | 5.2 MB to 27 KB, 99% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 4 | 20 | 6 | 554 B | 7 KB to 554 bytes, 93% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 167 | 188 | 25 | 25.3 KB | 64 KB to 25 KB, 60% smaller |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 547 | 568 | 69 | 61.2 KB | 5.0 MB to 61 KB, 99% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | DECLINED |  | 19 | 5 |  | No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame. |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 45 | 63 | 10 | 18.1 KB | 153 KB to 18 KB, 88% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 24 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS, over budget | 1827 | 1851 | 356 | 114.6 KB | 12 MB to 115 KB, 99% smaller |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 660 | 680 | 88 | 56.1 KB | 2.9 MB to 56 KB, 98% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 1319 | 1345 | 109 | 312.0 KB | 2 files, 312 KB |

### `image.compress` (Compress image, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 31 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 687 | 711 | 269 | 20.2 KB | 123 KB to 20 KB, 84% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS, over budget | 11846 | 11879 | 494 | 528.8 KB | 11 MB to 529 KB, 95% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 19 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 19 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 362 | 404 | 16 | 487 B | 698 bytes to 487 bytes, 30% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS, over budget | 7355 | 7392 | 710 | 260.7 KB | 5.2 MB to 261 KB, 95% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 265 | 286 | 55 | 315 B | 7 KB to 315 bytes, 96% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 2115 | 2137 | 93 | 56.3 KB | 64 KB to 56 KB, 12% smaller |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 18 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 1400 | 1422 | 198 | 268.5 KB | 5.0 MB to 268 KB, 95% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 1 | 19 | 6 | 86 B | 129 bytes to 86 bytes, 33% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 101 | 117 | 11 | 130.4 KB | 153 KB to 130 KB, 15% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 26 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS, over budget | 16939 | 16971 | 649 | 754.7 KB | 12 MB to 755 KB, 94% smaller |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 50 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 2473 | 2498 | 203 | 567.1 KB | 2 files, 567 KB |

### `image.convert` (Convert image, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 57 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 195 | 264 | 200 | 31.7 KB | 123 KB to 32 KB, 74% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 580 | 621 | 271 | 578.5 KB | 11 MB to 578 KB, 95% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 19 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 19 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 123 | 141 | 11 | 382 B | 698 bytes to 382 bytes, 45% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS, over budget | 2134 | 2160 | 763 | 1.4 MB | 5.2 MB to 1.4 MB, 73% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 1 | 19 | 5 | 907 B | 7 KB to 907 bytes, 88% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 39 | 57 | 12 | 73.1 KB | 64 KB to 73 KB, 14% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 98 | 117 | 31 | 972.1 KB | 5.0 MB to 972 KB, 81% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 9 | 31 | 5 | 352 B | 129 bytes to 352 bytes, 173% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 5 | 26 | 6 | 18.3 KB | 153 KB to 18 KB, 88% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS, over budget | 1027 | 1051 | 400 | 1.1 MB | 12 MB to 1.1 MB, 91% smaller |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 56 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 134 | 157 | 41 | 1.4 MB | 2 files, 1.4 MB |

### `image.erase` (Erase object, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed rect=10%,10%,20%,20%) | 1.2 KB | DECLINED |  | 25 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif (typed rect=10%,10%,20%,20%) | 123.0 KB | PASS, over budget | 573 | 609 | 771 | 178.7 KB | 123 KB to 179 KB, 45% bigger |
| img-avif-animated-8bpc | defaults only | 1.2 KB | DECLINED |  | 19 | 5 |  | Say what to erase: give a mask picture as the second file (white is erased), or set rect to a rectangle such as 100,200,300,80. |
| img-raw-canon-7d-sraw | cr2 (typed rect=10%,10%,20%,20%) | 11.1 MB | PASS, over budget | 1965 | 2002 | 660 | 6.9 MB | 11 MB to 6.9 MB, 38% smaller |
| img-vips-gif-dispose-previous | gif (typed rect=10%,10%,20%,20%) | 1.1 KB | DECLINED |  | 25 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif (typed rect=10%,10%,20%,20%) | 297.2 KB | DECLINED |  | 32 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif (typed rect=10%,10%,20%,20%) | 698 B | PASS | 448 | 482 | 580 | 2.8 KB | 698 bytes to 3 KB, 297% bigger |
| img-heic-rotated-cw90 | heif (typed rect=10%,10%,20%,20%) | 5.2 MB | PASS, over budget | 11482 | 11548 | 1300 | 23.1 MB | 5.2 MB to 23 MB, 347% bigger |
| img-vips-ico | ico (typed rect=10%,10%,20%,20%) | 7.4 KB | PASS | 309 | 345 | 575 | 1.9 KB | 7 KB to 2 KB, 74% smaller |
| img-vips-jp2 | jp2 (typed rect=10%,10%,20%,20%) | 64.0 KB | PASS | 417 | 450 | 596 | 496.9 KB | 64 KB to 497 KB, 677% bigger |
| img-turbo-arithmetic | jpg (typed rect=10%,10%,20%,20%) | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed rect=10%,10%,20%,20%) | 5.0 MB | PASS, over budget | 1715 | 1755 | 1028 | 1.5 MB | 5.0 MB to 1.5 MB, 69% smaller |
| img-pngsuite-odd-size-palette | png (typed rect=10%,10%,20%,20%) | 129 B | PASS | 264 | 292 | 572 | 148 B | 129 bytes to 148 bytes, 15% bigger |
| img-avif-png-icc-exif-xmp | png (typed rect=10%,10%,20%,20%) | 153.2 KB | PASS | 265 | 288 | 580 | 154.3 KB | 153 KB to 154 KB, 1% bigger |
| img-vips-tiff-subsampled | tif (typed rect=10%,10%,20%,20%) | 72.8 KB | DECLINED |  | 13 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed rect=10%,10%,20%,20%) | 12.2 MB | PASS, over budget | 2841 | 2873 | 937 | 12.8 MB | 12 MB to 13 MB, 5% bigger |
| img-commons-animated-webp | webp (typed rect=10%,10%,20%,20%) | 2.9 MB | DECLINED |  | 49 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed rect=10%,10%,20%,20%) | 5.4 MB | PASS, over budget | 1623 | 1655 | 1158 | 1.8 MB | 2 files, 1.8 MB |

### `image.remove_background` (Remove background, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 23 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 758 | 786 | 848 | 2.7 KB | 123 KB to 3 KB, 98% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 1091 | 1112 | 334 | 1.4 MB | 11 MB to 1.4 MB, 88% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 19 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 675 | 699 | 789 | 577 B | 698 bytes to 577 bytes, 17% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 4651 | 4704 | 1222 | 3.7 MB | 5.2 MB to 3.7 MB, 28% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 199 | 219 | 202 | 1.8 KB | 7 KB to 2 KB, 75% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 360 | 383 | 232 | 562.4 KB | 64 KB to 562 KB, 779% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 18 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 3050 | 3089 | 616 | 3.5 MB | 5.0 MB to 3.5 MB, 30% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 915 | 959 | 789 | 94 B | 129 bytes to 94 bytes, 27% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 864 | 907 | 799 | 36.0 KB | 153 KB to 36 KB, 76% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 2722 | 2758 | 496 | 2.8 MB | 12 MB to 2.8 MB, 77% smaller |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 61 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 3458 | 3499 | 1087 | 3.5 MB | 2 files, 3.5 MB |

### `image.remove_watermark` (Remove watermark, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif (typed own_content=true) | 1.2 KB | DECLINED |  | 19 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif (typed own_content=true) | 123.0 KB | DECLINED |  | 197 | 204 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-avif-animated-8bpc | defaults only | 1.2 KB | DECLINED |  | 13 | 5 |  | This tool is for pictures you own or have the right to edit: your own photos with a date stamp, your own exports with a logo you added, your own AI images with a visible badge. Set own_content to true to confirm that, then run it again. |
| img-raw-canon-7d-sraw | cr2 (typed own_content=true) | 11.1 MB | PASS, over budget | 1536 | 1574 | 787 | 6.9 MB | 11 MB to 6.9 MB, 38% smaller |
| img-vips-gif-dispose-previous | gif (typed own_content=true) | 1.1 KB | DECLINED |  | 13 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif (typed own_content=true) | 297.2 KB | DECLINED |  | 13 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif (typed own_content=true) | 698 B | DECLINED |  | 110 | 11 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-heic-rotated-cw90 | heif (typed own_content=true) | 5.2 MB | PASS, over budget | 8188 | 8227 | 1542 | 23.2 MB | 5.2 MB to 23 MB, 350% bigger |
| img-vips-ico | ico (typed own_content=true) | 7.4 KB | DECLINED |  | 19 | 5 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-vips-jp2 | jp2 (typed own_content=true) | 64.0 KB | DECLINED |  | 234 | 29 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-turbo-arithmetic | jpg (typed own_content=true) | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg (typed own_content=true) | 5.0 MB | DECLINED |  | 1412 | 343 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-pngsuite-odd-size-palette | png (typed own_content=true) | 129 B | DECLINED |  | 19 | 5 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-avif-png-icc-exif-xmp | png (typed own_content=true) | 153.2 KB | DECLINED |  | 93 | 13 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-vips-tiff-subsampled | tif (typed own_content=true) | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif (typed own_content=true) | 12.2 MB | DECLINED |  | 1714 | 395 |  | No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages). |
| img-commons-animated-webp | webp (typed own_content=true) | 2.9 MB | DECLINED |  | 51 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg (typed own_content=true) | 5.4 MB | PASS, over budget | 2424 | 2460 | 909 | 318.4 KB | 1 file, 318 KB |

### `image.resize` (Resize image, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 25 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 253 | 276 | 225 | 23.7 KB | 123 KB to 24 KB, 81% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 870 | 889 | 274 | 4.0 MB | 11 MB to 4.0 MB, 64% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 19 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 17 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 158 | 175 | 15 | 623 B | 698 bytes to 623 bytes, 11% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS, over budget | 604 | 625 | 308 | 32.1 KB | 5.2 MB to 32 KB, 99% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 5 | 24 | 5 | 1.4 KB | 7 KB to 1 KB, 81% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 84 | 103 | 11 | 496.9 KB | 64 KB to 497 KB, 677% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 101 | 117 | 23 | 40.5 KB | 5.0 MB to 40 KB, 99% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 1 | 19 | 5 | 102 B | 129 bytes to 102 bytes, 21% smaller |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 21 | 38 | 6 | 151.1 KB | 153 KB to 151 KB, 1% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS, over budget | 925 | 956 | 355 | 4.3 MB | 12 MB to 4.3 MB, 65% smaller |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 81 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 229 | 272 | 30 | 148.1 KB | 2 files, 148 KB |

### `image.resize_to_kb` (Resize to exact KB, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 49 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 311 | 364 | 200 | 31.2 KB | 123 KB to 31 KB, 75% smaller |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 1259 | 1314 | 261 | 636.2 KB | 11 MB to 636 KB, 94% smaller |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 99 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 25 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 188 | 218 | 11 | 382 B | 698 bytes to 382 bytes, 45% smaller |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS, over budget | 5007 | 5059 | 696 | 453.7 KB | 5.2 MB to 454 KB, 91% smaller |
| img-vips-ico | ico | 7.4 KB | PASS | 3 | 49 | 5 | 1.0 KB | 7 KB to 1 KB, 86% smaller |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 104 | 144 | 13 | 78.2 KB | 64 KB to 78 KB, 22% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 24 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 1106 | 1129 | 84 | 243.8 KB | 5.0 MB to 244 KB, 95% smaller |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 2 | 54 | 5 | 416 B | 129 bytes to 416 bytes, 222% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 16 | 45 | 6 | 19.8 KB | 153 KB to 20 KB, 87% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS, over budget | 2524 | 2566 | 359 | 1.1 MB | 12 MB to 1.1 MB, 91% smaller |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 89 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 1379 | 1410 | 104 | 656.9 KB | 2 files, 657 KB |

### `image.strip_metadata` (Strip metadata, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS | 0 | 31 | 5 | 1.2 KB | 1 KB to 1 KB, about the same size |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 1 | 37 | 5 | 123.0 KB | 123 KB to 123 KB, about the same size |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS | 5 | 29 | 27 | 11.1 MB | 11 MB to 11 MB, about the same size |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 35 | 5 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 37 | 5 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 0 | 23 | 5 | 698 B | 698 bytes to 698 bytes, about the same size |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS | 3 | 28 | 15 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| img-vips-ico | ico | 7.4 KB | DECLINED |  | 24 | 4 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-vips-jp2 | jp2 | 64.0 KB | DECLINED |  | 32 | 5 |  | This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again. |
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 0 | 25 | 5 | 5.1 KB | 5 KB to 5 KB, about the same size |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 12 | 32 | 15 | 5.0 MB | 5.0 MB to 5.0 MB, about the same size |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 1 | 37 | 5 | 129 B | 129 bytes to 129 bytes, about the same size |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 1 | 33 | 5 | 151.5 KB | 153 KB to 151 KB, 1% smaller |
| img-vips-tiff-subsampled | tif | 72.8 KB | PASS | 1 | 36 | 5 | 72.8 KB | 73 KB to 73 KB, about the same size |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS | 6 | 67 | 29 | 12.2 MB | 12 MB to 12 MB, about the same size |
| img-commons-animated-webp | webp | 2.9 MB | PASS | 2 | 49 | 10 | 2.9 MB | 2.9 MB to 2.9 MB, about the same size |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 16 | 50 | 16 | 5.4 MB | 5.4 MB to 5.4 MB, about the same size |

### `image.to_text` (Image to text, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | PASS (no change) | 417 | 463 | 157 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS (no change), over budget | 469 | 818 | 380 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | PASS (no change) | 1445 | 1480 | 283 | report | Z4  MINERAL WATER |
| img-vips-gif-dispose-previous | gif | 1.1 KB | PASS (no change) | 270 | 375 | 156 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-commons-gif-horse | gif | 297.2 KB | PASS (no change) | 196 | 301 | 157 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-heic-unsupported-depth | heif | 698 B | PASS (no change) | 375 | 411 | 157 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-heic-rotated-cw90 | heif | 5.2 MB | PASS (no change), over budget | 3041 | 3101 | 1101 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-vips-ico | ico | 7.4 KB | PASS (no change) | 395 | 445 | 160 | report | W |
| img-vips-jp2 | jp2 | 64.0 KB | PASS (no change) | 285 | 336 | 168 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 20 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS (no change), over budget | 2948 | 2988 | 704 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-pngsuite-odd-size-palette | png | 129 B | PASS (no change) | 187 | 217 | 150 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS (no change) | 212 | 250 | 159 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 42 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | PASS (no change) | 549 | 893 | 220 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-commons-animated-webp | webp | 2.9 MB | PASS (no change) | 337 | 377 | 215 | report | No text found. If the picture has text in another language, choose the language, for example --opt languages=hi. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | DECLINED |  | 22 | 5 |  | testimgari.jpg: This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |

### `image.upscale` (Upscale, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 62 | 6 |  | colors-animated-8bpc.avif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-avif-gainmap-avif | avif | 123.0 KB | PASS | 1087 | 1116 | 245 | 2.1 MB | 123 KB to 2.1 MB, 1635% bigger |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | DECLINED |  | 885 | 261 |  | This picture is already 2,592 x 1,728. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 22 | 5 |  | dispose-previous.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 33 | 5 |  | muybridge-race-horse.gif is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-heic-unsupported-depth | heif | 698 B | PASS | 379 | 420 | 178 | 14.1 KB | 698 bytes to 14 KB, 1916% bigger |
| img-heic-rotated-cw90 | heif | 5.2 MB | DECLINED |  | 3898 | 710 |  | guitar-rotated-cw90.hif is 4,672 x 7,008, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. |
| img-vips-ico | ico | 7.4 KB | PASS | 153 | 185 | 176 | 26.5 KB | 7 KB to 26 KB, 257% bigger |
| img-vips-jp2 | jp2 | 64.0 KB | PASS | 1744 | 1771 | 187 | 4.7 MB | 64 KB to 4.7 MB, 7247% bigger |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 19 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | DECLINED |  | 921 | 132 |  | day-earth-smiled.jpg is 9,000 x 3,500, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is. |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 133 | 154 | 176 | 878 B | 129 bytes to 878 bytes, 581% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 761 | 813 | 182 | 2.1 MB | 153 KB to 2.1 MB, 1263% bigger |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 19 | 5 |  | This TIFF stores pixels as YCbCr(8), which Panoply cannot read yet. Re-save it as a plain TIFF or PNG. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | DECLINED |  | 1017 | 393 |  | This picture is already 3,866 x 2,908. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway. |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 56 | 31 |  | animated-webp-sample.webp is animated. Choose "first frame" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet. |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS, over budget | 2531 | 2557 | 313 | 901.7 KB | 1 file, 902 KB |

### `media.compress` (Compress video, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 501 | 528 | 5 | 279.6 KB | 1.6 MB to 280 KB, 82% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 289 | 309 | 5 | 533.1 KB | 1.9 MB to 533 KB, 72% smaller |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 276 | 300 | 5 | 148.3 KB | 200 KB to 148 KB, 26% smaller |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 1224 | 1246 | 5 | 1.0 MB | 2.1 MB to 1.0 MB, 50% smaller |
| vid-gen-prores | mov | 2.9 MB | PASS | 1033 | 1064 | 5 | 277.1 KB | 2.9 MB to 277 KB, 91% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 573 | 603 | 5 | 293.8 KB | 555 KB to 294 KB, 47% smaller |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 1502 | 1544 | 5 | 2.1 MB | 2.8 MB to 2.1 MB, 24% smaller |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 2372 | 2390 | 5 | 400.4 KB | 991 KB to 400 KB, 60% smaller |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 5330 | 5355 | 5 | 7.1 MB | 10 MB to 7.1 MB, 33% smaller |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 591 | 613 | 5 | 649.4 KB | 395 KB to 649 KB, 65% bigger |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 1088 | 1106 | 5 | 860.9 KB | 2.5 MB to 861 KB, 65% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 203 | 221 | 5 | 68.1 KB | 10 KB to 68 KB, 605% bigger |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 197 | 214 | 5 | 17.4 KB | 230 KB to 17 KB, 92% smaller |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 2840 | 2859 | 5 | 3.5 MB | 5.1 MB to 3.5 MB, 32% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 1947 | 1965 | 5 | 1.6 MB | 5.2 MB to 1.6 MB, 70% smaller |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 6471 | 6491 | 6 | 8.7 MB | 17 MB to 8.7 MB, 48% smaller |

### `media.convert` (Convert video or audio, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 334 | 348 | 5 | 462.3 KB | 1.6 MB to 462 KB, 71% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 258 | 274 | 5 | 533.1 KB | 1.9 MB to 533 KB, 72% smaller |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS (no change) | 37 | 50 | 5 | report | aac-lc-stereo.m4a is already M4A, so there was nothing to convert. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 114 | 131 | 5 | 2.1 MB | 2.1 MB to 2.1 MB, about the same size |
| vid-gen-prores | mov | 2.9 MB | PASS | 415 | 426 | 5 | 1.2 MB | 2.9 MB to 1.2 MB, 58% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 277 | 294 | 5 | 582.8 KB | 555 KB to 583 KB, 5% bigger |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 821 | 836 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, 1% bigger |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS (no change) | 38 | 50 | 5 | report | bbb-360p-h264-10s.mp4 is already MP4, so there was nothing to convert. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS (no change) | 57 | 69 | 5 | report | bbb-1080p-h264-10s.mp4 is already MP4, so there was nothing to convert. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 351 | 365 | 5 | 862.5 KB | 395 KB to 863 KB, 119% bigger |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 507 | 519 | 5 | 1.8 MB | 2.5 MB to 1.8 MB, 28% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 112 | 124 | 5 | 155.1 KB | 10 KB to 155 KB, 1506% bigger |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 133 | 144 | 5 | 38.9 KB | 230 KB to 39 KB, 83% smaller |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 130 | 144 | 5 | 5.1 MB | 5.1 MB to 5.1 MB, about the same size |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 110 | 125 | 5 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS (no change) | 131 | 144 | 5 | report | bbb-360p-h264-10s.mp4 is already MP4, so there was nothing to convert. |

### `media.extract_audio` (Extract audio, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 93 | 105 | 5 | 1.6 MB | 1.6 MB to 1.6 MB, about the same size |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 75 | 85 | 5 | 1.9 MB | 1.9 MB to 1.9 MB, about the same size |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 75 | 86 | 5 | 200.4 KB | 200 KB to 200 KB, about the same size |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 92 | 105 | 5 | 74.3 KB | 2.1 MB to 74 KB, 96% smaller |
| vid-gen-prores | mov | 2.9 MB | PASS | 92 | 107 | 5 | 192.1 KB | 2.9 MB to 192 KB, 93% smaller |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 74 | 85 | 5 | 555.5 KB | 555 KB to 556 KB, about the same size |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 76 | 88 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | DECLINED |  | 49 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | DECLINED |  | 68 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 93 | 103 | 5 | 394.7 KB | 395 KB to 395 KB, about the same size |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 110 | 124 | 5 | 401.5 KB | 2.5 MB to 401 KB, 84% smaller |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 58 | 68 | 5 | 9.7 KB | 10 KB to 10 KB, about the same size |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 109 | 119 | 5 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 106 | 116 | 5 | 163.9 KB | 5.1 MB to 164 KB, 97% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | DECLINED |  | 50 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | DECLINED |  | 50 | 5 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.hdr_to_sdr` (HDR to SDR, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| vid-gen-multi-audio | mkv | 2.1 MB | PASS (no change) | 19 | 32 | 5 | report | multi-audio-3-tracks-subs.mkv is already standard range, so nothing was changed. |
| vid-gen-prores | mov | 2.9 MB | PASS (no change) | 36 | 50 | 5 | report | prores-proxy-720p.mov is already standard range, so nothing was changed. |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS (no change) | 38 | 55 | 5 | report | bbb-360p-h264-10s.mp4 is already standard range, so nothing was changed. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS (no change) | 54 | 68 | 5 | report | bbb-1080p-h264-10s.mp4 is already standard range, so nothing was changed. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS (no change) | 36 | 50 | 5 | report | phone-clip-vid20240601.webm is already standard range, so nothing was changed. |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS (no change) | 36 | 50 | 5 | report | bbb-720p-vp9-10s.webm is already standard range, so nothing was changed. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS (no change) | 130 | 143 | 5 | report | bbb-360p-h264-10s.mp4 is already standard range, so nothing was changed. |

### `media.info` (Video and audio info, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 19 | 35 | 5 | report | lachmoeweisma.flac: FLAC, 22.8 s, 1.6 MB, 552 kb/s |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 19 | 31 | 5 | report | glockenweihe.flac: FLAC, 2:55, 1.9 MB, 87 kb/s |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 36 | 50 | 5 | report | aac-lc-stereo.m4a: MP4, 12.0 s, 200 KB, 134 kb/s |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 76 | 90 | 5 | report | multi-audio-3-tracks-subs.mkv: Matroska (MKV), 6.0 s, 2.1 MB, 2.8 Mb/s |
| vid-gen-prores | mov | 2.9 MB | PASS | 38 | 50 | 5 | report | prores-proxy-720p.mov: QuickTime MOV, 2.0 s, 2.9 MB, 11.7 Mb/s |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 19 | 31 | 5 | report | recorder-improvisation.mp3: MP3, 24.0 s, 555 KB, 185 kb/s |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 17 | 30 | 5 | report | stay-down-here-1915.mp3: MP3, 2:55, 2.8 MB, 128 kb/s |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS | 76 | 86 | 5 | report | bbb-360p-h264-10s.mp4: MP4, 10.0 s, 991 KB, 793 kb/s |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS | 110 | 124 | 5 | report | bbb-1080p-h264-10s.mp4: MP4, 10.0 s, 10.5 MB, 8.4 Mb/s |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 36 | 50 | 5 | report | netaji-1943.ogg: Ogg, 53.2 s, 395 KB, 59 kb/s |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 38 | 50 | 5 | report | camera-clip-mvi0131.ogv: Ogg, 32.3 s, 2.5 MB, 611 kb/s |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 19 | 30 | 5 | report | speech-opus-7kbps.opus: Ogg, 10.8 s, 10 KB, 7 kb/s |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 34 | 48 | 5 | report | alaa1.wav: WAV, 2.6 s, 230 KB, 706 kb/s |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 73 | 87 | 5 | report | phone-clip-vid20240601.webm: WebM, 9.5 s, 5.1 MB, 4.3 Mb/s |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 73 | 87 | 5 | report | bbb-720p-vp9-10s.webm: WebM, 10.0 s, 5.2 MB, 4.2 Mb/s |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 252 | 267 | 5 | report | == bbb-360p-h264-10s.mp4 |

### `media.normalize` (Normalize loudness, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 220 | 235 | 6 | 1.7 MB | 1.6 MB to 1.7 MB, 8% bigger |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 404 | 416 | 5 | 2.1 MB | 1.9 MB to 2.1 MB, 11% bigger |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 257 | 267 | 7 | 220.8 KB | 200 KB to 221 KB, 10% bigger |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 331 | 344 | 7 | 2.1 MB | 2.1 MB to 2.1 MB, 2% bigger |
| vid-gen-prores | mov | 2.9 MB | PASS | 209 | 228 | 6 | 2.9 MB | 2.9 MB to 2.9 MB, about the same size |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 565 | 583 | 8 | 577.5 KB | 555 KB to 577 KB, 4% bigger |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 6859 | 6879 | 8 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | DECLINED |  | 68 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | DECLINED |  | 110 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 945 | 962 | 7 | 554.8 KB | 395 KB to 555 KB, 41% bigger |
| vid-commons-camera-ogv | ogg | 2.5 MB | DECLINED |  | 204 | 7 |  | camera-clip-mvi0131.ogv holds a kind of video or audio that this format cannot carry. Pick another format such as MKV. |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 576 | 592 | 6 | 154.2 KB | 10 KB to 154 KB, 1496% bigger |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 644 | 660 | 7 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 744 | 770 | 6 | 5.2 MB | 5.1 MB to 5.2 MB, 1% bigger |
| vid-bbb-720-vp9 | webm | 5.2 MB | DECLINED |  | 74 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | DECLINED |  | 70 | 5 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.repair` (Repair video, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 130 | 148 | 5 | 1.6 MB | 1.6 MB to 1.6 MB, about the same size |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS | 185 | 206 | 5 | 1.9 MB | 1.9 MB to 1.9 MB, about the same size |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS (no change) | 54 | 70 | 5 | report | aac-lc-stereo.m4a is not damaged: its index is complete, all 521 samples are in the file and it plays start to end. Nothing was changed. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 181 | 201 | 5 | 2.1 MB | 2.1 MB to 2.1 MB, about the same size |
| vid-gen-prores | mov | 2.9 MB | PASS (no change) | 71 | 92 | 9 | report | prores-proxy-720p.mov is not damaged: its index is complete, all 96048 samples are in the file and it plays start to end. Nothing was changed. |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 150 | 167 | 5 | 555.5 KB | 555 KB to 556 KB, about the same size |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 340 | 357 | 5 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS (no change) | 112 | 129 | 5 | report | bbb-360p-h264-10s.mp4 is not damaged: its index is complete, all 300 samples are in the file and it plays start to end. Nothing was changed. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS (no change) | 698 | 716 | 5 | report | bbb-1080p-h264-10s.mp4 is not damaged: its index is complete, all 600 samples are in the file and it plays start to end. Nothing was changed. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 212 | 233 | 5 | 394.7 KB | 395 KB to 395 KB, about the same size |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS | 204 | 227 | 5 | 2.5 MB | 2.5 MB to 2.5 MB, about the same size |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS | 132 | 149 | 5 | 9.7 KB | 10 KB to 10 KB, about the same size |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS | 162 | 180 | 5 | 229.8 KB | 230 KB to 230 KB, about the same size |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 700 | 719 | 5 | 5.1 MB | 5.1 MB to 5.1 MB, about the same size |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS | 703 | 728 | 5 | 5.2 MB | 5.2 MB to 5.2 MB, about the same size |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS | 1493 | 1509 | 5 | 5.2 MB | 1 file, 5.2 MB |

### `media.streams` (Streams, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS (no change) | 39 | 55 | 5 | report | lachmoeweisma.flac: 1 streams. Say what to do with drop, extract, default or language. |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS (no change) | 38 | 55 | 5 | report | glockenweihe.flac: 1 streams. Say what to do with drop, extract, default or language. |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS (no change) | 38 | 56 | 5 | report | aac-lc-stereo.m4a: 2 streams. Say what to do with drop, extract, default or language. |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS (no change) | 37 | 55 | 5 | report | multi-audio-3-tracks-subs.mkv: 5 streams. Say what to do with drop, extract, default or language. |
| vid-gen-prores | mov | 2.9 MB | PASS (no change) | 39 | 56 | 5 | report | prores-proxy-720p.mov: 2 streams. Say what to do with drop, extract, default or language. |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS (no change) | 37 | 57 | 5 | report | recorder-improvisation.mp3: 1 streams. Say what to do with drop, extract, default or language. |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS (no change) | 33 | 55 | 5 | report | stay-down-here-1915.mp3: 1 streams. Say what to do with drop, extract, default or language. |
| vid-bbb-360-h264 | mp4 | 991.0 KB | PASS (no change) | 37 | 57 | 5 | report | bbb-360p-h264-10s.mp4: 1 streams. Say what to do with drop, extract, default or language. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | PASS (no change) | 90 | 109 | 5 | report | bbb-1080p-h264-10s.mp4: 1 streams. Say what to do with drop, extract, default or language. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS (no change) | 38 | 55 | 5 | report | netaji-1943.ogg: 1 streams. Say what to do with drop, extract, default or language. |
| vid-commons-camera-ogv | ogg | 2.5 MB | PASS (no change) | 37 | 56 | 5 | report | camera-clip-mvi0131.ogv: 2 streams. Say what to do with drop, extract, default or language. |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS (no change) | 32 | 50 | 5 | report | speech-opus-7kbps.opus: 1 streams. Say what to do with drop, extract, default or language. |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS (no change) | 57 | 75 | 5 | report | alaa1.wav: 1 streams. Say what to do with drop, extract, default or language. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS (no change) | 54 | 69 | 5 | report | phone-clip-vid20240601.webm: 2 streams. Say what to do with drop, extract, default or language. |
| vid-bbb-720-vp9 | webm | 5.2 MB | PASS (no change) | 54 | 69 | 5 | report | bbb-720p-vp9-10s.webm: 1 streams. Say what to do with drop, extract, default or language. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | PASS (no change) | 168 | 187 | 5 | report | bbb-360p-h264-10s.mp4: 1 streams. Say what to do with drop, extract, default or language. |

### `media.to_mono` (Stereo to mono, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac | 1.6 MB | PASS | 335 | 354 | 7 | 1.3 MB | 1.6 MB to 1.3 MB, 20% smaller |
| aud-commons-glocken-flac | flac | 1.9 MB | PASS (no change) | 36 | 56 | 5 | report | glockenweihe.flac is already mono, so there was nothing to change. |
| aud-gen-m4a-aac | m4a | 200.4 KB | PASS | 332 | 352 | 8 | 220.9 KB | 200 KB to 221 KB, 10% bigger |
| vid-gen-multi-audio | mkv | 2.1 MB | PASS | 329 | 348 | 8 | 2.1 MB | 2.1 MB to 2.1 MB, 1% bigger |
| vid-gen-prores | mov | 2.9 MB | PASS (no change) | 38 | 56 | 5 | report | prores-proxy-720p.mov is already mono, so there was nothing to change. |
| aud-commons-recorder-mp3 | mp3 | 555.3 KB | PASS | 507 | 527 | 8 | 577.5 KB | 555 KB to 577 KB, 4% bigger |
| aud-commons-1915-mp3 | mp3 | 2.8 MB | PASS | 1739 | 1761 | 8 | 2.8 MB | 2.8 MB to 2.8 MB, about the same size |
| vid-bbb-360-h264 | mp4 | 991.0 KB | DECLINED |  | 69 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-1080-h264 | mp4 | 10.5 MB | DECLINED |  | 105 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| aud-commons-netaji-ogg | ogg | 394.7 KB | PASS | 786 | 806 | 8 | 737.6 KB | 395 KB to 738 KB, 87% bigger |
| vid-commons-camera-ogv | ogg | 2.5 MB | DECLINED |  | 228 | 7 |  | camera-clip-mvi0131.ogv holds a kind of video or audio that this format cannot carry. Pick another format such as MKV. |
| aud-commons-opus-7kbps | opus | 9.7 KB | PASS (no change) | 36 | 57 | 5 | report | speech-opus-7kbps.opus is already mono, so there was nothing to change. |
| aud-commons-alaa-wav | wav | 229.7 KB | PASS (no change) | 57 | 73 | 5 | report | alaa1.wav is already mono, so there was nothing to change. |
| vid-commons-phone-webm | webm | 5.1 MB | PASS | 476 | 496 | 6 | 5.1 MB | 5.1 MB to 5.1 MB, 1% smaller |
| vid-bbb-720-vp9 | webm | 5.2 MB | DECLINED |  | 87 | 5 |  | This file has no sound to work with. Check that the file is the one you meant. |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 | 16.7 MB | DECLINED |  | 82 | 5 |  | bbb-360p-h264-10s.mp4: This file has no sound to work with. Check that the file is the one you meant. |

### `media.trim` (Trim video or audio, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| aud-commons-lachmoeweisma-flac | flac (typed keep=0:00-0:01) | 1.6 MB | PASS | 112 | 130 | 5 | 78.8 KB | 1.6 MB to 79 KB, 95% smaller |
| aud-commons-glocken-flac | flac (typed keep=0:00-0:01) | 1.9 MB | PASS | 111 | 130 | 5 | 10.8 KB | 1.9 MB to 11 KB, 99% smaller |
| aud-commons-lachmoeweisma-flac | defaults only | 1.6 MB | DECLINED |  | 18 | 5 |  | Say what to keep, for example 0:12-1:40, or what to remove with the remove option |
| aud-gen-m4a-aac | m4a (typed keep=0:00-0:01) | 200.4 KB | PASS | 40 | 58 | 5 | 17.6 KB | 200 KB to 18 KB, 91% smaller |
| vid-gen-multi-audio | mkv (typed keep=0:00-0:01) | 2.1 MB | PASS | 112 | 129 | 5 | 361.7 KB | 2.1 MB to 362 KB, 83% smaller |
| vid-gen-prores | mov (typed keep=0:00-0:01) | 2.9 MB | PASS | 45 | 63 | 11 | 1.5 MB | 2.9 MB to 1.5 MB, 49% smaller |
| aud-commons-recorder-mp3 | mp3 (typed keep=0:00-0:01) | 555.3 KB | PASS | 127 | 146 | 5 | 28.5 KB | 555 KB to 29 KB, 95% smaller |
| aud-commons-1915-mp3 | mp3 (typed keep=0:00-0:01) | 2.8 MB | PASS | 110 | 124 | 5 | 17.4 KB | 2.8 MB to 17 KB, 99% smaller |
| vid-bbb-360-h264 | mp4 (typed keep=0:00-0:01) | 991.0 KB | PASS | 514 | 533 | 5 | 116.5 KB | 991 KB to 117 KB, 88% smaller |
| vid-bbb-1080-h264 | mp4 (typed keep=0:00-0:01) | 10.5 MB | PASS | 904 | 925 | 6 | 1.2 MB | 10 MB to 1.2 MB, 88% smaller |
| aud-commons-netaji-ogg | ogg (typed keep=0:00-0:01) | 394.7 KB | PASS | 115 | 133 | 5 | 10.8 KB | 395 KB to 11 KB, 97% smaller |
| vid-commons-camera-ogv | ogg (typed keep=0:00-0:01) | 2.5 MB | PASS | 110 | 125 | 5 | 96.8 KB | 2.5 MB to 97 KB, 96% smaller |
| aud-commons-opus-7kbps | opus (typed keep=0:00-0:01) | 9.7 KB | PASS | 112 | 131 | 5 | 1.1 KB | 10 KB to 1 KB, 89% smaller |
| aud-commons-alaa-wav | wav (typed keep=0:00-0:01) | 229.7 KB | PASS | 149 | 166 | 5 | 90.2 KB | 230 KB to 90 KB, 61% smaller |
| vid-commons-phone-webm | webm (typed keep=0:00-0:01) | 5.1 MB | PASS | 160 | 175 | 5 | 778.3 KB | 5.1 MB to 778 KB, 85% smaller |
| vid-bbb-720-vp9 | webm (typed keep=0:00-0:01) | 5.2 MB | PASS | 168 | 186 | 5 | 558.2 KB | 5.2 MB to 558 KB, 89% smaller |
| vid-bbb-360-h264, vid-sintel-720-h264, vid-bbb-1080-h264 | batch of 3 mp4 (typed keep=0:00-0:01) | 16.7 MB | PASS | 1356 | 1374 | 8 | 1.8 MB | 17 MB to 1.8 MB, 89% smaller |

### `pdf.compress` (Compress PDF, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 11 | 25 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 5901 | 5924 | 126 | 2.8 MB | 6.5 MB to 2.8 MB, 57% smaller |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 5970 | 5990 | 124 | 2.8 MB | 2 files, 2.8 MB |

### `pdf.images_to_pdf` (Images to PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-turbo-arithmetic | jpg | 5.1 KB | PASS | 5 | 25 | 5 | 5.9 KB | 5 KB to 6 KB, 16% bigger |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS | 7 | 25 | 10 | 5.0 MB | 5.0 MB to 5.0 MB, about the same size |
| img-pngsuite-odd-size-palette | png | 129 B | PASS | 6 | 25 | 5 | 917 B | 129 bytes to 917 bytes, 611% bigger |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS | 5 | 25 | 5 | 152.2 KB | 153 KB to 152 KB, 1% smaller |
| img-turbo-arithmetic, img-exifori-landscape-8, img-commons-wide-nasa | batch of 3 jpg | 5.4 MB | PASS | 7 | 25 | 10 | 5.4 MB | 1 file, 5.4 MB |

### `pdf.merge` (Merge PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 5 | 19 | 5 | 789 B | 978 bytes to 789 bytes, 19% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 38 | 56 | 5 | 6.5 MB | 6.5 MB to 6.5 MB, about the same size |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 52 | 74 | 5 | 6.6 MB | 1 file, 6.6 MB |

### `pdf.ocr` (OCR, pattern B)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| img-avif-animated-8bpc | avif | 1.2 KB | DECLINED |  | 19 | 5 |  | colors-animated-8bpc.avif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-avif-gainmap-avif | avif | 123.0 KB | DECLINED |  | 18 | 5 |  | seine-hdr-gainmap.avif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-raw-canon-7d-sraw | cr2 | 11.1 MB | DECLINED |  | 19 | 16 |  | Canon---EOS-7D---sRAW2-sRAW-3x2.CR2 is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-vips-gif-dispose-previous | gif | 1.1 KB | DECLINED |  | 19 | 5 |  | dispose-previous.gif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-commons-gif-horse | gif | 297.2 KB | DECLINED |  | 19 | 5 |  | muybridge-race-horse.gif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-heic-unsupported-depth | heif | 698 B | DECLINED |  | 19 | 4 |  | unsupported-depth.heic is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-heic-rotated-cw90 | heif | 5.2 MB | DECLINED |  | 18 | 10 |  | guitar-rotated-cw90.hif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-vips-ico | ico | 7.4 KB | DECLINED |  | 19 | 5 |  | favicon.ico is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-vips-jp2 | jp2 | 64.0 KB | DECLINED |  | 18 | 5 |  | world.jp2 is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-turbo-arithmetic | jpg | 5.1 KB | DECLINED |  | 18 | 5 |  | This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first. |
| img-commons-wide-nasa | jpg | 5.0 MB | PASS (no change), over budget | 2127 | 2159 | 663 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example --lang hi. |
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS, over budget | 1095 | 1126 | 385 | 1.9 KB | 978 bytes to 2 KB, 95% bigger |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS (no change), over budget | 482 | 508 | 350 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example --lang hi. |
| img-pngsuite-odd-size-palette | png | 129 B | PASS (no change) | 125 | 145 | 150 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example --lang hi. |
| img-avif-png-icc-exif-xmp | png | 153.2 KB | PASS (no change) | 145 | 167 | 159 | report | No text was found on 1 page, so nothing was changed and no new file was written. If the text is in another language, choose it, for example --lang hi. |
| img-vips-tiff-subsampled | tif | 72.8 KB | DECLINED |  | 18 | 5 |  | subsampled.tif is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-raw-pixel-7-pro-dng | tif | 12.2 MB | DECLINED |  | 19 | 17 |  | Google---Pixel-7-Pro---16bit-4x3.dng is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| img-commons-animated-webp | webp | 2.9 MB | DECLINED |  | 19 | 7 |  | animated-webp-sample.webp is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first. |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS, over budget | 1277 | 1313 | 566 | 1.9 KB | 1 file, 2 KB |

### `pdf.organize` (Organize PDF pages, pattern C)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf (typed rotate=90) | 978 B | PASS | 5 | 24 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf (typed rotate=90) | 6.5 MB | PASS | 39 | 56 | 5 | 6.5 MB | 6.5 MB to 6.5 MB, about the same size |
| pdf-qpdf-inherited-rotate | defaults only | 978 B | DECLINED |  | 19 | 5 |  | Nothing to do. Say which pages to keep or delete, a rotation, or turn on reverse. |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf (typed rotate=90) | 6.6 MB | PASS | 46 | 63 | 5 | 6.6 MB | 6.6 MB to 6.6 MB, about the same size |

### `pdf.split` (Split PDF, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 5 | 25 | 5 | 701 B | 978 bytes to 701 bytes, 28% smaller |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 176 | 198 | 6 | 6.5 MB | 32 files, 6.5 MB |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 188 | 206 | 6 | 6.6 MB | 34 files, 6.6 MB |

### `pdf.to_images` (PDF to images, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| pdf-qpdf-inherited-rotate | pdf | 978 B | PASS | 37 | 56 | 26 | 38.2 KB | 978 bytes to 38 KB, 3801% bigger |
| pdf-commons-ia-scan-mining-claims | pdf | 6.5 MB | PASS | 3204 | 3226 | 132 | 8.5 MB | 32 files, 8.5 MB |
| pdf-qpdf-inherited-rotate, pdf-pdfjs-annotation-highlight, pdf-commons-ia-scan-mining-claims | batch of 3 pdf | 6.6 MB | PASS | 2232 | 2252 | 147 | 8.6 MB | 34 files, 8.6 MB |

### `text.slugify_lines` (Slugify lines, pattern A)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| CONTRIBUTING.md | md | 3.0 KB | PASS | 18 | 31 | 11 | 2.7 KB | 3 KB to 3 KB, 7% smaller |
| docs/FLOWS.md | md | 16.7 KB | PASS | 15 | 30 | 11 | 15.0 KB | 17 KB to 15 KB, 10% smaller |
| img-vips-svg | xml | 4.7 KB | PASS | 13 | 24 | 11 | 4.3 KB | 5 KB to 4 KB, 9% smaller |
| CONTRIBUTING.md, README.md, docs/FLOWS.md | batch of 3 md | 23.5 KB | PASS | 14 | 25 | 11 | 21.1 KB | 24 KB to 21 KB, 10% smaller |

### `text.word_count` (Word count, pattern I)

| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |
|---|---|---:|---|---:|---:|---:|---:|---|
| CONTRIBUTING.md | md | 3.0 KB | PASS | 10 | 24 | 10 | report | CONTRIBUTING.md: 71 lines, 439 words, 2958 characters, 2958 bytes |
| docs/FLOWS.md | md | 16.7 KB | PASS | 11 | 25 | 10 | report | FLOWS.md: 231 lines, 2725 words, 16742 characters, 16746 bytes |
| img-vips-svg | xml | 4.7 KB | PASS | 10 | 25 | 10 | report | logo.svg: 31 lines, 327 words, 4700 characters, 4700 bytes |
| CONTRIBUTING.md, README.md, docs/FLOWS.md | batch of 3 md | 23.5 KB | PASS | 11 | 25 | 10 | report | CONTRIBUTING.md: 71 lines, 439 words, 2958 characters, 2958 bytes |

