## 4. Downloaded at run time

Panoply does not contain these files. The first job that needs one downloads it from the address below, checks
its SHA-256 against a pin in the source, and keeps it in your cache folder (`~/Library/Caches/Panoply/engines`
on macOS, `%LOCALAPPDATA%\Panoply\cache\engines` on Windows, `~/.cache/panoply/engines` on Linux). The size
is shown before anything is fetched. Every address names a release, tag or commit, never a moving branch.
Licenses below were checked against the upstream repository or the model page on 2026-10-14.

| Part | Used for | Version or commit | License | Source |
|---|---|---|---|---|
| PDFium (pdfium-binaries build) | Drawing PDF pages, JPEG 2000 and JBIG2 images | chromium/7881 (pdfium 151.0.7881.0) | BSD-3-Clause and Apache-2.0 for PDFium, MIT for the build scripts. The package carries a `licenses/` folder for the libraries inside it | https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F7881, upstream https://pdfium.googlesource.com/pdfium/ |
| ONNX Runtime | Runs every model below | 1.28.3 | MIT | https://github.com/microsoft/onnxruntime/releases/tag/v1.28.3 |
| PP-OCRv5 mobile text detection | OCR fallback | Hugging Face commit e6f4fa85 | Apache-2.0 | https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx |
| PP-OCRv5 mobile recognition, English | OCR fallback | commit 3fafbc3b | Apache-2.0 | https://huggingface.co/PaddlePaddle/en_PP-OCRv5_mobile_rec_onnx |
| PP-OCRv5 mobile recognition, Latin | OCR fallback | commit 89d3a50e | Apache-2.0 | https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx |
| PP-OCRv5 mobile recognition, Devanagari | OCR fallback | commit 251aec19 | Apache-2.0 | https://huggingface.co/PaddlePaddle/devanagari_PP-OCRv5_mobile_rec_onnx |
| Tesseract language data (tessdata_fast) for Bengali, English and Hindi | OCR for languages the OS engines do not read | commit 87416418 | Apache-2.0 | https://github.com/tesseract-ocr/tessdata_fast |
| YuNet face detector (2023mar) | Passport photo and face tools off macOS, and as a fallback | opencv_zoo commit 26cc381e | MIT (Copyright (c) 2020 Shiqi Yu) | https://github.com/opencv/opencv_zoo/tree/26cc381e4d2594bb9f47a26eb8fd96c94a13660d/models/face_detection_yunet |
| IS-Net general use, int8 | Remove background when the OS engine is missing or fails | Hugging Face commit 71eff237 | MIT on the model page. The upstream DIS code is Apache-2.0 | https://huggingface.co/xrds/isnet-general-onnx-int8, from https://huggingface.co/imgly/isnet-general-onnx and https://github.com/xuebinqin/DIS |
| MI-GAN 512 Places2 | Erase objects, remove watermarks | Hugging Face commit 406830d0 | MIT (weights, LICENSE-WEIGHTS, Copyright (c) 2024 Picsart AI Research) | https://huggingface.co/andraniksargsyan/migan, from https://github.com/Picsart-AI-Research/MI-GAN |
| Real-ESRGAN general x4v3 (Qualcomm AI Hub ONNX export) | Upscale | AI Hub release v0.64.0 | BSD-3-Clause (xinntao/Real-ESRGAN) | https://huggingface.co/qualcomm/Real-ESRGAN-General-x4v3, from https://github.com/xinntao/Real-ESRGAN |
| FFmpeg and FFprobe, LGPL build (Windows and Linux) | Video and audio tools | BtbN tag autobuild-2026-09-30-13-08, FFmpeg n9.0.2 (commit 2a571b6068) | LGPL-3.0-or-later (built with `--enable-version3`, without GPL or nonfree parts) | https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-09-30-13-08 |

The pinned checksums and exact URLs are in `crates/panoply-pdf/src/pdfium.rs`,
`crates/panoply-ocr/src/models.rs`, `crates/panoply-id/src/runtime.rs`, `crates/panoply-vision/src/models.rs`,
`crates/panoply-vision/src/runtime.rs` and `crates/panoply-media/src/engine/pins.rs`. Panoply keeps the
LICENSE file of PDFium and ONNX Runtime next to the library it downloads.

### 4.1 The FFmpeg helper (LGPL notice)

FFmpeg is Copyright (c) the FFmpeg developers and is licensed under the GNU Lesser General Public License,
version 3 or later, in the build Panoply fetches. The build also contains other libraries under their own
permissive and LGPL licenses, listed in the build scripts and carried as license files in the archive.

- Panoply does not link FFmpeg. It starts `ffmpeg` and `ffprobe` as separate programs and talks to them with
  command-line arguments, so the LGPL applies to the helper and not to Panoply. You can replace the helper with
  your own build by setting `PANOPLY_FFMPEG`.
- Panoply does not distribute the helper. You download it from BtbN's release page. If a Panoply release ever
  mirrors those archives, that release will carry the corresponding source and the build scripts.
- FFmpeg source for the pinned build: https://github.com/FFmpeg/FFmpeg at commit
  2a571b606854520cf89804d8030c8b328e621689 (release 9.0 line, n9.0.2 plus 17 commits). The build scripts and the
  versions of every library in it: https://github.com/BtbN/FFmpeg-Builds. The LGPL v3 text:
  https://www.gnu.org/licenses/lgpl-3.0.txt. FFmpeg's own legal page: https://ffmpeg.org/legal.html.
- On macOS no LGPL build is pinned yet. Panoply uses the `ffmpeg` and `ffprobe` found on your PATH (for example
  from Homebrew) and only asks them for encoders in the LGPL set.

### 4.2 Training data

Licenses above are the licenses of the published model files. They do not always cover the data the model
was trained on. Two cases are open and stated here instead of hidden:

- IS-Net general use was trained on the DIS5K dataset. DIS5K's terms of use allow non-commercial research and
  education and say nothing about trained weights. The weights are published under MIT. Panoply uses the OS
  cutout engine first on macOS and loads IS-Net only as the fallback.
- MI-GAN was trained on Places2 and FFHQ images. Panoply has not reviewed those datasets' terms.

If you ship Panoply inside a commercial product and these matter to you, replace the model through the
`PANOPLY_MODEL_ISNET` and `PANOPLY_MODEL_MIGAN` variables or leave those two modules out.

## 5. Software you install yourself

Panoply uses these programs only if you have already installed them. It never downloads or ships them.

- **Tesseract** (Apache-2.0, https://github.com/tesseract-ocr/tesseract), found as `tesseract` on your PATH or
  through `PANOPLY_TESSERACT`, for Bengali and the other languages that need it. The language data it reads is
  the tessdata_fast data in section 4.
- **ffmpeg and ffprobe on macOS**, as described in section 4.1.
- **Operating system engines**: Apple Vision and ImageIO on macOS, the Windows Imaging Component and Media
  Foundation on Windows. They are part of your system and are not distributed with Panoply.
