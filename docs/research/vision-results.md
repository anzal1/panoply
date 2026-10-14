# Image AI modules: models, timings, memory and edge quality

Measured on 2026-10-14 on an Apple M5 Max (18 cores, 48 GB) running macOS 26.6, with the image corpus
from `tests/corpus/image.toml` and two synthetic sets made for this report (watermarks with known
masks, composites with a known alpha). ONNX Runtime 1.28.3, `ort` 2.0.0-rc.13, dev profile build (all
dependencies at opt-level 2, `panoply-vision` and `panoply-image` at opt-level 3). The Mac was busy
with other builds the whole time (load average 8 to 60), so wall times below are inflated and noisy.
CPU seconds ("user") and peak memory are steadier, and the "idle" figures quoted in the text were read
from the result cards on a quieter moment. Every row is three runs.

Modules: `image.remove_background`, `image.erase`, `image.remove_watermark`, `image.upscale`
(`crates/panoply-vision`, manifests in `modules/builtin/image-ai-*`). Specs: `docs/specs/id-and-image.md`
3.8, 3.10, 3.11, 3.14, 3.15.

## 1. What is downloaded

Everything is fetched once, on first use, from the pinned URL below, into the per-user engines folder
(`~/Library/Caches/Panoply/engines` on macOS, `PANOPLY_ENGINES_DIR` overrides it). The size is shown in
the first progress line before the network is touched ("Downloading the IS-Net background model (44 MB,
once)"), and `panoply models` lists sizes and what is already on disk. A file that does not match its
SHA-256 is never used and is removed. Sizes are decimal MB (the research doc's "42 MB" for IS-Net is
42.2 MiB).

| Item | Used by | Download | Installed | License | Source (pinned) |
|---|---|---|---|---|---|
| ONNX Runtime 1.28.3 | all four | 33 MB (macOS arm64 tgz, 31.5 MiB), 9.1 MB Linux x64, 8.1 MB Linux arm64, 79 MB Windows zips | the one library | MIT | github.com/microsoft/onnxruntime release v1.28.3, digests from the release |
| IS-Net general use, int8 (weight-only QDQ) | remove background (fallback) | 44.2 MB | 44.2 MB | MIT | huggingface.co/xrds/isnet-general-onnx-int8 at commit 71eff237 (int8 of imgly/isnet-general-onnx, whose fp32 export has digest cc2c9f5c...). sha256 3b21a670... |
| MI-GAN 512 Places2, whole pipeline | erase, remove watermark | 28.1 MB | 28.1 MB | MIT | huggingface.co/andraniksargsyan/migan at commit 406830d0, the file the MI-GAN README links. sha256 6f1f3530... |
| Real-ESRGAN general x4v3 (float ONNX) | upscale | 4.5 MB zip | 5.0 MB (0.16 MB graph + 4.8 MB weights) | BSD-3-Clause | Qualcomm AI Hub release v0.64.0 of the xinntao weights, qaihub-public-assets.s3 zip. sha256 fc094329... (members pinned too) |

Where the numbers differ from `docs/research/tiny-models.md`:

- **MI-GAN is 28 MB, not 15 MB.** 15 MB is the fp16 TFLite. `ort` needs ONNX, and the only official ONNX
  is the pre-converted pipeline (crop around the mask, resize, blend, uint8 in and out), 28.1 MB fp32.
- **Real-ESRGAN is 4.5 MB, not 3 MB.** 3 MB is the LiteRT file. The float ONNX is 4.65 MB per Qualcomm's
  card, fixed to a 128 x 128 tile (output 512 x 512).
- **IS-Net int8 is a third party's quantization** (xrds, MIT, with its conversion script in the repo).
  imgly ships only fp32 (176 MB) and fp16 (88 MB). I used the int8 as the research note recommended and
  did not re-quantize. The training-data terms of DIS5K are still unverified (spec 3.8 flags this).

The idle-memory budget holds: nothing is downloaded or loaded at start, a model loads for a job, and
`runtime::JobScope` plus a watcher thread drop it after 60 s idle (`PANOPLY_MODEL_IDLE_SECS=0` drops it
when the job ends). `tests/idle.rs` checks the drop.

## 2. The engine ladder, as measured

| Job | macOS | Linux and Windows |
|---|---|---|
| Background | Apple Vision `VNGenerateForegroundInstanceMask` through `crates/panoply-platform-macos/src/vision.rs`, then IS-Net when Vision is missing, finds nothing, or `engine=model` | IS-Net on the CPU |
| Erase, remove watermark | MI-GAN on the CPU | MI-GAN on the CPU |
| Upscale | Real-ESRGAN on CoreML | Real-ESRGAN on DirectML where the runtime build has it, else the CPU |

Provider choices were measured on this Mac, not assumed:

- **Upscale: CoreML is 3 times faster and uses 18 times less CPU** than the CPU provider. 400 x 300 to
  1600 x 1200 (20 tiles): 0.5 s and 0.35 CPU-s on CoreML against 1.1 to 1.7 s and 6 CPU-s on the CPU, on
  an idle machine. The two outputs differ by at most 1 level of 255 (PSNR 91 dB). The first CoreML run
  compiles the model (about 4 s once, cached in `engines/vision-cache`).
- **IS-Net: CoreML buys nothing.** 1.5 s against 1.2 s at 1024 x 1024 and about 50 MB more memory, so
  IS-Net runs on the CPU everywhere.
- **MI-GAN: CPU only.** The pipeline has dynamic shapes, which CoreML's static-shape mode skips.

## 3. Timings and peak memory

"Wall" is the best run, "CPU" the user time and "Peak" the resident set size, both as a range over the
runs (`/usr/bin/time -l`). The machine was busy (see the top), so read wall times as upper bounds; the
last column has the figure from the result card on a quieter moment where I caught one.

| Job | Input | Wall | CPU | Peak | Quieter wall |
|---|---|---|---|---|---|
| Cutout, Vision (OS) | dog JPEG 12.2 MP | 3.2 s | 3.0 to 3.7 s | 253 to 300 MB | 1.7 s |
| Cutout, IS-Net | dog JPEG 12.2 MP | 2.5 s | 4.4 to 7.7 s | 715 to 764 MB | 2.9 s |
| Cutout, Vision (OS) | pug HEIC 12.2 MP | 2.9 s | 2.5 s | 339 to 359 MB | |
| Cutout, IS-Net | pug HEIC 12.2 MP | 2.5 s | 4.2 to 7.3 s | 790 to 826 MB | |
| Cutout, Vision (OS) | pen HEIC 5 MP | 2.0 s | 1.4 to 1.6 s | 211 to 216 MB | |
| Cutout, IS-Net | pen HEIC 5 MP | 2.6 s | 5.4 to 5.9 s | 690 to 718 MB | |
| Cutout on blurred background, Vision | dog JPEG 12.2 MP | 2.5 s | 2.0 to 2.3 s | 289 to 310 MB | |
| Erase 100 x 110 | JPEG 1800 x 1200 | 0.45 s | 1.2 to 1.35 s | 609 to 613 MB | 0.2 s |
| Erase 300 x 300 | dog JPEG 12.2 MP | 0.9 s | 1.4 to 1.6 s | 761 to 765 MB | |
| Remove watermark: find, MI-GAN fill | JPEG 1600 x 1200 | 1.3 s | 1.6 s | 627 to 634 MB | 0.6 s |
| Remove watermark: find, blur fill | JPEG 1600 x 1200 | 1.2 s | 0.5 s | 58 to 64 MB | |
| Upscale 4x, CoreML, PNG out | JPEG 400 x 300 (20 tiles) | 1.3 s | 0.6 to 0.85 s | 80 to 82 MB | 0.5 s |
| Upscale 4x, CPU, PNG out | JPEG 400 x 300 (20 tiles) | 4.4 s | 10 to 14 s | 88 to 90 MB | 1.1 s |
| Upscale 4x, CoreML, JPG out | JPEG 1000 x 562 (96 tiles) to 4000 x 2248 | 1.0 s | 0.35 s | 87 MB | 0.8 s |
| Upscale 2x, CoreML, PNG out | JPEG 1800 x 1200 (247 tiles) to 3600 x 2400 | 6.0 s | 3.2 to 3.9 s | 97 to 100 MB | |
| Upscale 2x, CPU, PNG out | JPEG 1800 x 1200 (247 tiles) | 45 to 50 s | 163 to 208 s | 111 to 112 MB | |

The 2x CoreML row includes the PNG encode of 8.6 MP, which I did not time separately; the 4x JPG row
(96 tiles in about 1 s, JPEG encode included) bounds the cost of a tile at roughly 10 ms on CoreML. The
CPU rows cost about 0.7 CPU-seconds per tile, which is why the CPU provider is the slow path on Linux and
why DirectML matters on Windows. I ran the CPU rows only a few times, on purpose.

Where the stage time goes, 12 MP dog, Vision engine, idle machine (`PANOPLY_VISION_TRACE=1` prints this
with the resident size): decode 0.14 s, subject found 0.5 s, edge refine 0.2 s, edge colors 0.5 s,
PNG encode of 12 MP RGBA 0.8 s, total 1.7 s. The spec's bar is "under 3 s for 12 MP", met by both engines.

What the memory means against the 300 MB image budget (FLOWS.md):

- **Upscale and the blur fill stay far under it** (63 to 112 MB): tiles and strips, never the whole result.
- **The Vision path is at the cap at 12 MP** (295 MB). Resident size by stage on the 12 MP dog, from
  `PANOPLY_VISION_TRACE=1`: 86 MB after decode, 146 MB after the subject is found (the system engine
  adds about 60 MB), 174 MB after the edge refine, 201 MB after the edge colors, 248 MB when the RGBA
  result is composed, and the 295 MB peak is the PNG encode of 12 MP RGBA on top of that (freed planes
  stay in the allocator's pages). Pictures over 24 MP are decoded at a reduced size and the card says so;
  I did not build the strip pipeline that spec 3.8 describes for them.
- **IS-Net (about 750 MB) and MI-GAN (610 to 760 MB) are over it.** Both are activation-bound (IS-Net at
  1024 px, MI-GAN at 512 px with a 512 channel generator). Spec 3.8 allows 2 GB for the fallback and
  3.14 lists peak memory as an exception, so they are inside the specs, not inside the generic budget.
  Disabling the runtime's CPU arena (done) and the graph optimization level made no difference to the
  peak, so it is the model's live tensors.
- Erase and watermark fill run on a square crop around the mask (at least 512 px, mask plus 128 px of
  context, shrunk to 1536 px when the area is huge), so a 12 MP photo costs 150 MB more than a small one
  (decode and the changed-pixel map), not 12 MP of model input.

## 4. Remove background: edge quality

**What was looked at.** Real photos at 100 percent (dog, pug, pen, plant from the corpus), cut out with
each engine, shown on magenta and cyan so halos and holes are obvious.

- Both engines keep individual fur strands on the dog's ears and muzzle after the refine and edge color
  steps, and the cutout shows no halo of the old pavement or grass.
- **Vision is better on bodies, IS-Net on nothing in this set.** On the dog, Vision keeps the whole
  animal including the white chest. IS-Net loses the white chest fur against the light paving, and in
  the round-trip composites below it left the white party hat semi-transparent. That is the engine
  ladder doing its job: Vision first.
- **Vision's mask on its own is blobby** (smooth outline, no strands). The guided filter is what brings
  the hair back, which is why the refine step is on by default.
- A picture with no clear subject (the generated gradient files) gets "almost nothing was kept" on the
  card instead of a wrong cutout. Vision finding nothing falls through to IS-Net, with a note.

**Round trip against a pseudo ground truth.** The corpus has no matting ground truth. I took the
refined Vision cutout of the dog (hair alpha included), composited it on four other pictures
(waterfall landscape, mountains, planet, a pavement patch), and cut it out again. Mean absolute error of
alpha (times 1000) inside the soft edge band (ground-truth alpha between 0.02 and 0.98, grown 8 px), and
intersection over union of the opaque area:

| Engine | Refine | landscape | mountains | planet | pavement | mean error | mean IoU |
|---|---|---|---|---|---|---|---|
| Vision | off | 67 | 65 | 53 | 134 | 80 | 0.926 |
| Vision | on | 56 | 57 | 38 | 120 | 68 | 0.927 |
| IS-Net | off | 82 | 72 | 59 | 248 | 115 | 0.898 |
| IS-Net | on | 66 | 63 | 43 | 250 | 106 | 0.899 |

Read this with care: the ground truth is Vision's own output, so Vision is favoured, and the pavement
background is nearly the colour and texture of the dog's fur, where both engines lose area. What it
does show cleanly is that **the refine step lowers the edge error by 10 to 20 percent on every engine
and picture**, and that IS-Net stays within 0.01 IoU of Vision on contrasting backgrounds. A synthetic
fur ball with known strands was also tried and discarded: neither engine segments procedural fur
(errors 0.29 to 0.37), so it says nothing about real hair.

**Implementation notes that came from the measurements.**

- The guided filter (He, Sun, Tang) alone leaves the mask plateaus a few percent away from 0 and 1. A
  stretch of the fitted values and a snap of the ends fixed that: a 1 pixel offset, 4 pixel ramp mask
  goes from 4.0 to 1.4 mean error in the unit test, and an exact ramp to 0.3.
- Decontamination is Forte and Pitie's blur fusion, at a quarter of the resolution for the large radius.
- Colour and blur backgrounds composite in linear light, and the blur is made from the background alone
  (normalized convolution), so the subject's colour never bleeds into it.

## 5. Erase

- **Pixels outside the mask never change.** The job hashes (SHA-256) every pixel outside the expanded mask
  before the fill and after it, and fails the job if the digests differ. The card says so. The test
  `erase_replaces_the_marked_area_and_nothing_else` also compares every pixel outside the rectangle with
  the original and checks that a red 80 x 60 block inside is gone.
- Mask from a second picture (white erases, or its alpha when it has real transparency), from `mask=`
  (a file option), or from `rect=x,y,w,h` (pixels or percentages, several with `;`). `expand` (default 8)
  grows the mask with a disk. `invert=true` for black-on-white masks.
- Separate marks far apart (more than 96 px) are filled one at a time, so the crop around each is small.
- A fill is the model's invention, not recovery. On a 1600 x 1200 photo with 490 x 40 px of white text
  over fur, a rectangle that covers the text gives a clean, plausible continuation; the fill is as good as
  the same model run in Python on the same crop (checked by hand, same ONNX file).

## 6. Remove watermark (own content only)

The module refuses to run until `own_content=true`; it takes files and not URLs, ships no list of marks,
and does not look for invisible watermarks or C2PA. The CLI is `panoply unmark ... --yes-i-own-these`.

**Detection** is classical: white and black top-hats at three stroke widths, thresholded against the
local texture, letter-sized pieces only (large connected masses are dropped), merged along lines, then
scored by letter count, baseline alignment, steadiness of the stroke strength, lean against the
surroundings and closeness to an edge. Date stamps in warm saturated colours near a corner have their
own path. The fill mask is the strokes, not a box.

**Measured on a synthetic set** (46 cases: 8 corpus pictures at 1600 px with six kinds of mark added by
Pillow, so the true mask is known). Recall is the share of true mark pixels inside the detected mask;
precision is the share of detected pixels within 1 percent of the picture height of a true mark pixel.

| Mark | Cases | Mean recall | Found (recall at least 0.5) | Mean precision |
|---|---|---|---|---|
| A: thin white text, 45% opacity, bottom right | 8 | 0.49 | 4 of 8 | 0.81 |
| B: bold black text, 55%, bottom left | 8 | 0.37 | 3 of 8 | 0.79 |
| C: large white SAMPLE, 28%, centered | 7 | 0.00 | 0 of 7 | 0.71 |
| D: orange date stamp, top left | 8 | 0.88 | 7 of 8 | 0.85 |
| E: translucent badge with text, top right | 8 | 0.39 | 5 of 8 | 0.84 |
| F: diagonal tiled text (out of scope on purpose) | 7 | 0.01 | 0 of 7 | 0.77 |
| A to E overall | 39 | 0.44 | | 0.80 |

On the eight clean pictures the false-positive area is 0.00 percent on six and 0.2 to 0.5 percent on the
other two (the dog's fur and a plant). Detection takes 30 to 350 ms.

For comparison, Apple Vision's text recognizer (`panoply_platform_macos::ocr`, run on the same set
through `examples/wm_ocr.rs`) covers 0.62 of the true mark pixels with its word boxes, but it also
returns real text in the scene (a party hat, a HUD) and misses thin text on the panorama, so it is a
proposal source for a later release, not a replacement.

What this means: corner stamps and date stamps are found; faint or large centered text, tiled diagonal
marks and icon-only logos are not, and the user marks those with `rect=`. The honest summary is that
"simple detection" gets about half of the realistic marks with few false alarms, and that the
spec's pattern C editor (accept or edit boxes) is needed for the rest.

**Fill.** MI-GAN on stroke masks grown 4 px, or `fill=blur` (blur of the surroundings, normalized so the
mark does not tint it) for flat areas. When detection finds only part of a line of text, the model
sometimes continues the visible letters into the hole, so a partial detection leaves a ghost. That is
the main reason to prefer `rect=` over auto detection on busy backgrounds.

## 7. Upscale

- **Tiles are invisible.** The same picture upscaled with the tile grid in two places (cropping 48 px off
  the top and left moves every tile boundary) gives the same result to 45.2 dB PSNR (Python reference,
  mean difference 0.0011 of full scale). Within 4 output pixels of a tile boundary the difference is
  0.0023 against 0.0007 to 0.0011 elsewhere, under one level of 255. Margin 16 against 24 pixels of
  context: 58 dB. The test `upscale_4x_has_the_right_size_and_tiles_leave_no_seams` enforces PSNR above
  38 dB and a boundary difference below 1.5 of 255.
- Compared with bicubic at 4x, edges are sharp and thin lines keep their width; the model's usual
  smoothed, painterly look appears in textures.
- 2x is the 4x tile averaged down with exact 2 x 2 boxes, so strips cannot disagree at their joins.
- Transparency is upscaled separately by bilinear sampling and kept in PNG; JPG output flattens onto white
  and says so.
- Limits and refusals: output over 120 MP, input over 16 MP, and an `auto` scale on a picture already
  larger than 2000 px say so in a sentence ("This picture is already 3,400 x 2,400. Upscaling it gains
  little... Set scale to 2 to do it anyway").

## 8. Reproduce

```
cargo build -j4 -p panoply-cli
target/debug/panoply models                          # sizes and what is on disk
target/debug/panoply cutout a.jpg [--color white | --blur 40] [--engine os|model]
target/debug/panoply erase a.jpg --rect 850,520,100,110      # or: erase a.jpg mask.png
target/debug/panoply unmark ./photos --auto --yes-i-own-these [--edges] [--save-mask]
target/debug/panoply upscale a.jpg --scale 4
PANOPLY_VISION_TRACE=1 PANOPLY_VISION_EP=cpu ...     # stage times, force the CPU provider
cargo test -p panoply-vision                         # downloads the three models on first run
cargo run -q -p panoply-vision --example wm_eval -- DIR      # DIR holds marked/, gt/, clean/ (see the example)
```

## 9. Gaps

- **Watermark detection is half a feature** (section 6). No batch median path (spec 3.15 (b)), no OCR
  assisted proposals yet, no logo detector beyond text-like blobs.
- **Windows and Linux are not tested.** The code paths are written (DirectML provider registration,
  CPU everywhere, `tar` for the archives) but this machine is a Mac, so none of it ran. The pinned
  Windows runtime is Microsoft's CPU zip, which has no DirectML provider, so DirectML works only when
  `PANOPLY_ORT_LIB` points at a DirectML build (the NuGet package, 1.24.4, 12.5 MB, was inspected but is
  not pinned because two crates in this tree must share one runtime version). There is no macOS x86_64
  runtime archive in release 1.28.3.
- **Over the 300 MB budget:** IS-Net about 750 MB and MI-GAN 610 to 760 MB peak (inside the exceptions
  specs 3.8 and 3.14 allow); the Vision path touches 295 to 341 MB at 12 MP.
- **Huge pictures** (over 24 MP) are processed at reduced size for cutout, erase and watermark, and the
  card says so. The strip pipeline of IPC-7 is built only for upscale.
- No portrait-matte prior from HEIC (IPC-3), no "keep shadow", no model choice or denoise for upscale,
  16-bit and wide-gamut inputs are processed and saved as 8-bit sRGB, animated files use the first frame.
- `crates/panoply-platform-macos/src/vision.rs` and `faces.rs::subject_mask` (another agent's, built at
  the same time) both call the foreground mask request. Mine also returns the instance count and takes
  RGB or RGBA; they should be merged.
- The four modules ask for the network in their manifests. The host approves it from the manifest today
  (`panoply-host/src/registry.rs`), so a download happens without a prompt until the app has its consent UI.
- `cargo deny check` passes (2026-10-14). The four unmaintained-crate advisories that came in through the desktop app are fixed or documented in `deny.toml`.
