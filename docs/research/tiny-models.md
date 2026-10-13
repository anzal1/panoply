# Panoply tiny clean-license model survey

Date: 2026-10-13. Gate: weights under Apache-2.0, MIT or BSD (no Gemma terms, Llama license, CC-BY-NC, research-only, custom restricted). Sizes come from the Hugging Face API (`/api/models/<id>?blobs=true`) on 2026-10-13 unless marked "unverified". MB here means MiB (bytes / 1048576, floored).

Status: COMPLETE. Sections A, per-job detail (1 to 14), B (summary table) and C (download totals) below. Sizes exclude tokenizer, config and vocab files (small, not summed) unless stated. "Unverified" marks anything not read from a model card, license file or API response this session.

## A) EmbeddingGemma verdict

Two models exist.

### EmbeddingGemma 1 (google/embeddinggemma-300m, Sept 2025)
- License: HF tag `license:gemma`, card links Gemma Terms of Use (https://ai.google.dev/gemma/terms) and the Gemma Prohibited Use Policy. Gated=manual. ONNX mirror onnx-community/embeddinggemma-300m-ONNX is also `license:gemma`.
- Size: 308M params. model.safetensors 1155 MB (bf16-ish). ONNX: fp32 1177 MB, fp16 588 MB, q8 (model_quantized) 294 MB, q4 187 MB, q4f16 167 MB.
- MTEB multilingual v2 mean(task): 61.15 (from the EmbeddingGemma 2 card comparison column).
- VERDICT: REJECT. Gemma Terms of Use is a custom restricted license.

### EmbeddingGemma 2 (google/embeddinggemma-2, released ~2026-10-06, newer)
- License: HF metadata `license: apache-2.0`, card header links https://ai.google.dev/gemma/docs/gemma_4_license which renders standard Apache License 2.0 text. Not gated (gated=false). Same for onnx-community/embeddinggemma-2-ONNX and litert-community/embeddinggemma-2-text-270m-litert-lm (both apache-2.0).
- Caveat: the card's Ethics section still says "Deployments must adhere to the Gemma Prohibited Use Policy". That sentence is guidance in a model card, not part of the Apache 2.0 license text; the license page contains no Gemma-specific terms. Flag for a human to read; treated as passing the gate on the metadata, with a residual-risk note.
- Size: 740M total, but modular. Text-only part is 270M (130M transformer + 140M embedder), vision 170M, audio 300M, loadable separately. Single safetensors is 1419 MB. Text-only ONNX (onnx-community/embeddinggemma-2-ONNX): fp32 1033 MB, fp16 516 MB, q8 299 MB, q4 165 MB, q4f16 149 MB. LiteRT-LM text-270m generic: 157 MB.
- Quality: MTEB multilingual v2 mean(task) 61.36 at 768d, 61.17 at 512d, 60.41 at 256d, 57.89 at 128d (card). Hindi and Bengali are not named on the card (says 100+ languages); per-language numbers unverified.
- VERDICT vs gate: PASSES the license gate (Apache-2.0 per card). FAILS the "tiniest" philosophy for the default embedding job: 149-165 MB at 4-bit versus tens of MB for static embeddings. Use it only as an optional "quality" tier, and prefer OS engines first (Apple NLContextualEmbedding).

## Job 1: Text embeddings (multilingual incl. Hindi and Bengali)

OS engines first: Apple NLContextualEmbedding (Apple platforms), no Android ML Kit text embedder with Hindi/Bengali verified (MediaPipe Text Embedder exists but its bundled models are English; unverified), Windows AI APIs (unverified). The fallback below is for Linux, Windows and Android.

| Candidate | Params | Smallest shipped file | License (HF metadata) | Hindi/Bengali | Quality note |
|---|---|---|---|---|---|
| ibm-granite/granite-embedding-97m-multilingual-r2 | 97M | onnx/model_quint8_avx2.onnx 93 MB (fp32 onnx 371 MB) | apache-2.0, https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2 | Both in the 52 enhanced languages (hi, bn, mr, ur, te) | MTEB multilingual retrieval (18 tasks) 60.3 vs multilingual-e5-small 50.9 (card). 384d, ModernBERT |
| minishlab/potion-multilingual-128M (Model2Vec static) | 128M (lookup table) | model.onnx 488 MB fp32; third-party int8 safetensors 122 MB (Tokenade/potion-multilingual-128M-i8-tokenade, MIT) | mit, https://huggingface.co/minishlab/potion-multilingual-128M | 101 languages, hi and bn in card language list | MTEB multilingual mean(task) 47.31 vs LaBSE 52.07 (card). Zero transformer compute (mean of embedded tokens), 256d |
| intfloat/multilingual-e5-small | 118M (unverified count) | onnx/model_qint8_avx512_vnni.onnx 112 MB in intfloat repo (avx512 only); Xenova/multilingual-e5-small onnx/model_int8.onnx 112 MB (portable, repo has no license tag, underlying model is MIT) | mit, https://huggingface.co/intfloat/multilingual-e5-small | 100 languages (unverified Bengali eval) | Old baseline, 50.9 multilingual retrieval per Granite card |
| microsoft/harrier-oss-v1-270m | 270M | onnx-community q4f16 163 MB | mit, https://huggingface.co/microsoft/harrier-oss-v1-270m | Hindi in language list, bn in tags | MTEB v2 66.5 (card). Best quality, too big for tiniest |
| sentence-transformers/static-similarity-mrl-multilingual-v1 | static | onnx/model_int8.onnx 103 MB | apache-2.0 | hi, mr, ur, gu in list; NO Bengali in the language list | MTEB mean 47.24 (potion card table) |
| MongoDB/mdbr-leaf-mt (English only) | ~23M (unverified) | onnx model_quantized 21 MB | apache-2.0, https://huggingface.co/MongoDB/mdbr-leaf-mt | English | Use only as an English tier |
| minishlab/potion-base-8M (English only) | 8M | onnx/model.onnx 28 MB fp32 (int8 would be ~7 MB, unverified) | mit | English | Smallest English; potion-base-2M is 7 MB fp32, potion-base-4M 14 MB |
| all-MiniLM-L6-v2 | 22M | onnx/model.onnx 86 MB fp32 (int8 variants unverified here) | apache-2.0 | English | Heavier than the leaf/potion options above for no gain |
| Snowflake/snowflake-arctic-embed-xs / -s | 22M / 33M | 86 MB / 126 MB fp32 | -s apache-2.0; -xs repo has NO license tag on HF (card unverified) | English | Skip: license unclear for xs |
| BAAI/bge-small-en-v1.5 | 33M | 126 MB fp32 | mit | English | No multilingual |
| nomic-embed-text-v1.5 | 137M | 521 MB fp32 | apache-2.0 | English | Too large |
| ibm-granite/granite-embedding-107m-multilingual (older) | 107M | 408 MB onnx | apache-2.0 | 12 languages, Hindi/Bengali not in the list (unverified) | Superseded by the r2 above |
| jinaai/jina-embeddings-v3 and v5-text-nano/small | 570M+ / ~200M / more | 1091 MB / 403 MB / 1136 MB | cc-by-nc-4.0 | | REJECT (non-commercial) |

Pick: granite-embedding-97m-multilingual-r2, int8 93 MB. Runner-up: potion-multilingual-128M (int8 122 MB, near zero compute, lower quality). English-only cheap tier: mdbr-leaf-mt 21 MB or potion-base-8M.
Open idea (unverified, not tested): prune the potion-multilingual-128M vocabulary to en+hi+bn tokens to cut it far below 122 MB; Model2Vec supports vocabulary operations but the resulting size and quality were not measured.
Compare against EmbeddingGemma 2 text: 149 MB at q4f16 for MTEB multilingual v2 61.36, a different benchmark from the Granite retrieval number, so not directly comparable.

## Job 2: Image embeddings for photo search by meaning

OS engines: Apple Vision VNGenerateImageFeaturePrintRequest does image-to-image similarity only, no text-to-image, so Apple platforms still need a CLIP-style fallback for text queries (the Apple-side claim is from memory, unverified this session). Android ML Kit image labeling and MediaPipe Image Embedder are image-to-image or fixed labels (unverified for text queries).

REJECTED: MobileCLIP and MobileCLIP2 (apple/MobileCLIP*, timm/MobileCLIP2-*), FastVLM. HF tag `license:apple-amlr`. The LICENSE file (https://huggingface.co/apple/MobileCLIP2-S0/raw/main/LICENSE) says the model is "for the sole purpose of scientific research" and "Research Purposes does not include any commercial exploitation, product development or use in any commercial product". Research-only, fails the gate. Xenova/mobileclip_s0 is tagged `license:other` (same Apple terms). NLLB-CLIP (visheratin, used by Immich) is cc-by-nc-4.0, rejected.

| Candidate | Size shipped (int8) | License | Languages | Quality note |
|---|---|---|---|---|
| TinyCLIP ViT-40M/32 Text-19M LAION400M | onnx-community/...-ONNX onnx/model_int8.onnx 81 MB (image+text in one graph; fp32 321 MB) | wkcn/TinyCLIP-ViT-40M-32-Text-19M-LAION400M is `mit`; the onnx-community mirror has no license tag. Code repo LICENSE is MIT (https://raw.githubusercontent.com/microsoft/Cream/main/TinyCLIP/LICENSE) | English | ImageNet-1K zero-shot 59.8% (card model zoo) |
| TinyCLIP ViT-8M/16 Text-3M YFCC15M | onnx-community/TinyCLIP-ViT-8M-16-Text-3M-YFCC15M-ONNX model_int8.onnx 23 MB | mit (https://huggingface.co/onnx-community/TinyCLIP-ViT-8M-16-Text-3M-YFCC15M-ONNX) | English | ImageNet-1K 41.1%, probably too weak for good photo search |
| TinyCLIP ViT-39M/16 Text-19M YFCC15M | only 317 MB fp32 safetensors on HF; int8 would be ~80 MB (estimate, unverified) | mit | English | ImageNet-1K 63.5%, best of the small ones |
| OpenAI CLIP ViT-B/32 | Xenova/clip-vit-base-patch32 vision int8 84 MB + text int8 61 MB = 145 MB | MIT (https://raw.githubusercontent.com/openai/CLIP/main/LICENSE; the HF mirror has no license tag) | English | The usual baseline |
| google/siglip2-base-patch16-224 | onnx-community/siglip2-base-patch16-224-ONNX vision_model_int8 90 MB + text_model_int8 270 MB = 360 MB (q4f16 text 422 MB, no gain) | apache-2.0 (https://huggingface.co/google/siglip2-base-patch16-224) | Multilingual (Hindi/Bengali coverage not verified on card) | Best quality here but text tower is 270 MB because of the 256k vocab |
| sentence-transformers/clip-ViT-B-32-multilingual-v1 (text) + OpenAI CLIP vision | text tower onnx 515 MB fp32 (canavar/clip-ViT-B-32-multilingual-v1-ONNX); int8 not shipped, ~130 MB estimate unverified; plus vision int8 84 MB | apache-2.0 | 50+ incl. hi, mr, ur, gu; NO bn in the card list | Multilingual text tower aligned to CLIP B/32 |

Pick: TinyCLIP ViT-40M/32 int8, 81 MB, English queries. For Hindi and Bengali queries, route the query through the OS translation engine (Apple Translation, Android ML Kit Translate, Windows AI) to English first, which costs nothing in download. Runner-up: SigLIP2 base int8 360 MB if native multilingual is required.
Runtime: ONNX Runtime. Quality gap to heavy choice: SigLIP2/OpenCLIP large models score far higher; TinyCLIP 40M is 59.8% IN-1K vs the ~63% class for B/32 (OpenAI figure from memory, unverified).

## Job 3: OCR fallback

OS engines: Apple Vision VNRecognizeTextRequest, Android ML Kit Text Recognition v2 (Latin, Chinese, Devanagari, Japanese, Korean scripts from memory, no Bengali; unverified this session), Windows.Media.Ocr / Windows AI text recognition. Hindi/Bengali coverage per OS is unverified here. The fallback is for Linux and gaps.

| Candidate | Size | License | Scripts | Notes |
|---|---|---|---|---|
| PP-OCRv5 mobile detection (PaddlePaddle/PP-OCRv5_mobile_det_onnx) | 4 MB inference.onnx | apache-2.0 (https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx) | script-agnostic boxes | Shared detector for every script |
| PP-OCRv5 mobile rec en / latin / devanagari / ta / te / arabic (PaddlePaddle/<lang>_PP-OCRv5_mobile_rec_onnx) | 7 MB each, inference.onnx | apache-2.0 (e.g. https://huggingface.co/PaddlePaddle/devanagari_PP-OCRv5_mobile_rec_onnx) | devanagari covers Hindi/Marathi | Download per script on demand |
| PP-OCRv5 mobile rec multilingual (zh, en, ja) | 15 MB | apache-2.0 | CJK+Latin | Only if CJK is needed |
| PP-OCRv6 tiny det + rec (PaddlePaddle/PP-OCRv6_tiny_det_onnx, _tiny_rec_onnx) | 1 MB + 4 MB (1.1M param rec) | apache-2.0 | 49 languages claimed, but the recognizer dictionary (inference.yml) contains zero Devanagari/Bengali/Tamil/Arabic/Thai/Cyrillic characters, only CJK + Latin | Smallest of all; no Indic |
| PP-OCRv6 small det + rec | 9 MB + 20 MB | apache-2.0 | same dictionary check: no Indic | rec W-Avg 81.3 vs v5 mobile 73.7 on the card benchmark |
| Tesseract tessdata_fast | ben 0.82 MB, hin 1.07 MB, eng 3.9 MB (GitHub API byte sizes 855841, 1122751, 4113088) | Apache-2.0 (https://github.com/tesseract-ocr/tessdata_fast/blob/main/LICENSE; engine is also Apache-2.0) | 100+ incl. Bengali | Lower accuracy than PP-OCR; only fallback for Bengali since no PP-OCR Bengali model exists |
| docTR parseq multilingual (Felix92/doctr-torch-parseq-multilingual-v1) | size unverified | apache-2.0 | Latin + some | Heavier, not measured |
| RapidOCR | runtime only (ONNX/PaddleOCR models inside) | apache-2.0 | same as PP-OCR | Packaging option, no own weights of interest |
| PaddleOCR-VL 1.5/1.6 (GGUF) | far larger (unverified) | apache-2.0 | | Not tiny; skip |

Pick: PP-OCRv5 mobile det 4 MB + per-script rec 7 MB (en or latin, plus devanagari for Hindi) = 11 MB for English, 18 MB with Hindi; Bengali via Tesseract ben 0.82 MB. Runner-up: Tesseract fast for all scripts (0.8 to 4 MB per language, lower accuracy).
Runtime: ONNX Runtime (PP-OCR), libtesseract (Tesseract).

## Job 4: Background removal

OS engines: Apple Vision VNGenerateForegroundInstanceMaskRequest and VNGeneratePersonSegmentationRequest, Android ML Kit Subject Segmentation (Google Play services model, not a bundled file) and Selfie Segmentation, Windows AI "Image Foreground Extractor" (Windows AI APIs). All unverified this session beyond name recall.

REJECTED: briaai/RMBG-1.4 and RMBG-2.0 (HF tag `license:other`, non-commercial Bria terms). onnx-community/ISNet-ONNX is tagged agpl-3.0 (a repackaging with a different tag than imgly's mit; avoid, use the imgly or original DIS source).

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| U2-Net-P (u2netp) (BritishWerewolf/U-2-Netp, coilabs/u2netp-q) | 4 MB onnx | apache-2.0 on HF; upstream https://github.com/xuebinqin/U-2-Net is Apache-2.0 (GitHub API spdx) | Smallest; edges are rough vs heavy models |
| MODNet (Xenova/modnet; litert-community/MODNet-LiteRT) | 6 MB int8 onnx (24 MB fp32; LiteRT 24 MB) | apache-2.0; upstream https://github.com/ZHKKKe/MODNet README says code, models and demos are Apache-2.0 | Portrait matting only, not general objects |
| IS-Net general-use (imgly/isnet-general-onnx; xrds/isnet-general-onnx-int8 for int8) | 42 MB int8 weight-only QDQ (fp32 167 MB, fp16 84 MB) | imgly repo `mit`; upstream DIS code https://github.com/xuebinqin/DIS is Apache-2.0 (the DIS5K dataset has its own terms of use, training-data terms unverified) | General objects, third-party int8 with claimed fp32-equal output |
| BiRefNet_lite (ZhengPeng7/BiRefNet_lite; onnx-community/BiRefNet_lite-ONNX) | 109 MB fp16 onnx (213 MB fp32; 169 MB safetensors); no int8 shipped (unverified) | mit (https://huggingface.co/ZhengPeng7/BiRefNet_lite); upstream repo MIT | Best quality in the clean set, 1024px input |
| BEN2 (PramaLLC/BEN2, onnx-community/BEN2-ONNX) | size not checked | mit | Not evaluated further |

Pick: IS-Net general-use int8 42 MB for general objects (check the third-party int8 yourself before shipping, or quantize imgly's fp32). Cheaper tier: u2netp 4 MB. Runner-up: BiRefNet_lite fp16 109 MB for quality.
Runtime: ONNX Runtime (also LiteRT for MODNet/DIS).

## Job 5: Inpainting for erase

OS engines: none verified as a public API. Apple has no public object-erase API (Apple Intelligence Clean Up is not exposed, from memory, unverified). Android: none in ML Kit. Windows AI: "Object Erase" exists in Windows AI Imaging APIs (from memory, unverified). So this is a fallback on most platforms.

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| MI-GAN 512 Places2 (litert-community/MI-GAN-512-Places2-LiteRT; Acly/MIGAN-GGUF) | 15 MB fp16 .tflite; 14 MB F16 gguf | mit (https://huggingface.co/litert-community/MI-GAN-512-Places2-LiteRT); upstream https://github.com/Picsart-AI-Research/MI-GAN is MIT (GitHub API) | Mobile-targeted by design. Training data (Places2) has its own terms, not reviewed: unverified |
| LaMa big-lama (Carve/LaMa-ONNX) | 197 MB fp32 onnx (the "fp16" file in anyisalin/big-lama-onnx is also 197 MB, so no shrink verified) | apache-2.0 (https://huggingface.co/Carve/LaMa-ONNX; upstream https://github.com/advimman/lama is Apache-2.0) | The usual heavier choice, better on large holes |

Pick: MI-GAN 512 at 15 MB (14 MB gguf alternative), about 13x smaller than LaMa. Runner-up: LaMa 197 MB. Runtime: LiteRT/TFLite or GGUF (ggml); ONNX export of MI-GAN exists upstream but was not size-checked.
Quality note: MI-GAN paper claims LaMa-comparable results on 512px at a fraction of the compute (paper claim, not re-verified here).

## Job 6: Upscale

OS engines: Apple has no public photo super-resolution API for apps (from memory, unverified); Windows AI "Image Super Resolution" API exists (from memory, unverified). Android: none in ML Kit.

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| realesr-general-x4v3 (SRVGGNetCompact, ~1.2M params, parameter count from memory) | 3 MB .tflite (litert-community/real-esrgan-x4v3-litert); original .pth 4 MB (clavidk/realesr-general-x4v3); Qualcomm card lists 4.65 MB float and 1.25 MB w8a8 | bsd-3-clause (https://huggingface.co/litert-community/real-esrgan-x4v3-litert; upstream https://github.com/xinntao/Real-ESRGAN BSD-3-Clause per GitHub API) | Best size to quality; real-world denoise-capable general model |
| SPAN 4x (4xNomosUni_span_multijpg) | 4 MB safetensors, 1 MB onnx fp32 (nesaorg mirror, no license tag) | cc-by-4.0 (https://huggingface.co/Phips/4xNomosUni_span_multijpg), community weights, attribution license outside the Apache/MIT/BSD gate. The SPAN code repo had no LICENSE at hongyuanyu/SPAN (404) | Fast, but fails the strict gate |
| RealPLKSR x4 (darktable-org/upscale-realplksr-onnx) | 28 MB onnx | mit (https://huggingface.co/darktable-org/upscale-realplksr-onnx) | Heavier, MIT, good quality (not benchmarked here) |
| Real-ESRGAN x4plus | 63 MB safetensors (Comfy-Org repack) | bsd-3-clause | The heavier usual choice |
| Swin2SR lightweight x2 | 3 MB safetensors | apache-2.0 | x2 only |
| APISR GRL/RRDB | | gpl-3.0 | REJECT |

Pick: realesr-general-x4v3 at 3 MB (LiteRT) or 4 to 5 MB fp32. Runner-up: RealPLKSR x4 MIT 28 MB.

## Job 7: Speech to text

OS engines: Apple SpeechAnalyzer/SpeechTranscriber and SFSpeechRecognizer (on-device, language availability for Hindi/Bengali not verified), Android SpeechRecognizer on-device / ML Kit GenAI Speech Recognition (unverified), Windows AI / Windows Speech recognition (unverified). Fallback is for Linux and unsupported locales.

REJECTED by the gate: NVIDIA Parakeet TDT 0.6B v2/v3 and CTC 1.1B (HF tag cc-by-4.0, attribution license outside Apache/MIT/BSD, and 0.6B is large anyway). Moonshine non-English variants (moonshine-tiny-ar/ja/ko/uk/vi/zh, base-*, moonshine-es) are tagged `license:other` (community license). The Moonshine streaming tiny/small/medium and the English moonshine-tiny/base are MIT. Vosk small Hindi is Apache 2.0 but has no Bengali.

| Candidate | Languages | Size shipped | License | Notes |
|---|---|---|---|---|
| Moonshine tiny (onnx-community/moonshine-tiny-ONNX) | English | encoder int8 7 MB + decoder_model_merged int8 19 MB = 26 MB (27M params) | mit (https://huggingface.co/moonshine-ai/moonshine-tiny) | English only. Moonshine base: 19 + 40 = 59 MB (61M params) |
| Moonshine streaming tiny (moonshine-ai/moonshine-streaming-tiny) | English | 168 MB safetensors (34M params; quantized ONNX not checked) | mit | Open ASR avg WER 12.01 (tiny) vs 7.84 (small, 123M) vs 6.65 (medium, 245M), from the card; also MIT variants for de, es, ja, vi, ar, zh, tl |
| IndicConformer hi / bn, one model per language (OpenVoiceOS/ai4bharat-indicconformer-hi-onnx, -bn-onnx, converted from ai4bharat) | Hindi; Bengali | model.int8.onnx 131 MB each (fp32 ~459 MB) | mit (https://huggingface.co/OpenVoiceOS/ai4bharat-indicconformer-bn-onnx; upstream ai4bharat repos also mit). WER not on the cards: unverified | Dedicated Indic models, loaded via onnx-asr. Best clean option for Hindi and Bengali |
| Whisper tiny / base / small (OpenAI weights, apache-2.0 on HF, MIT repo; ggml files from ggerganov/whisper.cpp, MIT) | 99 languages incl. Hindi, Bengali | ggml q5_1: tiny 30 MB, base 56 MB, small 181 MB (q8_0: 41 / 77 / 252 MB; ONNX int8 decoder+encoder from onnx-community/whisper-* not summed) | apache-2.0 (https://huggingface.co/openai/whisper-small) | Hindi/Bengali accuracy at tiny and base is unverified and expected to be poor; third-party PolyWhisper reports FLEURS Bengali WER 130 to 199 on a Whisper-Small backbone with LoRA, Hindi 43 to 46, indicating Whisper small is weak for Bengali |
| Vosk small Hindi (vosk-model-small-hi-0.22) | Hindi | 42 MB | Apache 2.0 (https://alphacephei.com/vosk/models); WER 20.89 IITM, 24.72 MUCS on the page | No Bengali on the Vosk page |
| sherpa-onnx | runtime (Apache-2.0 claimed, repo license check hit a rate limit: unverified) | | | Runtime for the models above, not weights |

Pick: English: Moonshine tiny int8 26 MB. Hindi: IndicConformer hi int8 131 MB. Bengali: IndicConformer bn int8 131 MB. Other languages: Whisper base q5_1 56 MB (or small q5_1 181 MB if quality demands).
Runner-up: Whisper (ggml q5_1) for everything; Vosk small hi 42 MB for Hindi if size beats accuracy.
Runtime: ONNX Runtime (Moonshine, IndicConformer), whisper.cpp/GGML (Whisper).

## Job 8: Text to speech

OS engines: Apple AVSpeechSynthesizer, Android TextToSpeech, Windows SAPI/WinRT SpeechSynthesizer. Hindi and Bengali voice availability per OS unverified. Fallback only where the OS lacks the language.

| Candidate | Size shipped | License | Languages | Notes |
|---|---|---|---|---|
| KittenTTS nano 0.8 int8 (KittenML/kitten-tts-nano-0.8-int8) | 23 MB onnx + 3 MB voices.npz = 26 MB (fp32 54 MB; micro 39 MB; mini 74 MB) | apache-2.0 on HF (https://huggingface.co/KittenML/kitten-tts-nano-0.8-int8). NOTE: the newer KittenML/kitten-tts-2 is `other` (stellon-labs-community-license, 469 MB to 3.3 GB): REJECT | English | Smallest clean TTS. Quality vs Kokoro not measured |
| Kokoro-82M v1.0 (onnx-community/Kokoro-82M-v1.0-ONNX) | model_quantized 88 MB (q8f16 82 MB, fp16 155 MB, fp32 310 MB); all voices 27 MB total, ~0.5 MB per voice | apache-2.0 (https://huggingface.co/hexgrad/Kokoro-82M; "Apache-licensed weights" in card) | Multiple incl. Hindi: hf_alpha, hf_beta, hm_omega, hm_psi (VOICES.md). No Bengali | Best clean quality at this size |
| Piper voices (rhasspy/piper-voices) | ~60 MB hi_IN, 73 MB bn_BD, 63 MB en_US lessac medium | Repo tag mit, but each voice carries its own dataset license in MODEL_CARD: hi_IN pratham and priyamvada CC-BY-NC-SA 4.0 (REJECT), hi_IN rohan IITM custom license (REJECT unreviewed), bn_BD google CC BY-SA 4.0 + CMU license (share-alike, outside gate), en_US lessac Blizzard 2013 license (not reviewed, not Apache/MIT/BSD) | | All Piper voices checked fail or are unclear. Also the espeak-ng phonemizer used by Piper is GPL-3.0 (from memory, unverified) |

Pick: English: KittenTTS nano 0.8 int8, 26 MB. Hindi: Kokoro int8 88 MB + 0.5 MB per voice. Bengali: none clean found; rely on OS TTS.
Runner-up: Kokoro for English too (88 MB). Runtime: ONNX Runtime.

## Job 9: VAD and noise suppression

OS engines: VAD is built into the OS speech stacks (Apple SpeechAnalyzer, Android SpeechRecognizer); Apple AVAudioEngine voice processing and Android NoiseSuppressor AudioEffect do noise suppression (from memory, unverified); Windows has Voice Clarity / noise suppression APIs (unverified).

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| Silero VAD (snakers4/silero-vad; istupakov/silero-vad-onnx mirror) | silero_vad.onnx 2,327,524 bytes (2.2 MB); 16k-only op15 variant 1.2 MB | MIT (https://raw.githubusercontent.com/snakers4/silero-vad/master/LICENSE) | De facto VAD, ONNX/CoreML/MLX mirrors exist |
| GTCRN (Xiaobin-Rong/gtcrn) | model_trained_on_dns3.tar 579,819 bytes (0.55 MB); 48.2K params, 33.0 MMACs per second (README) | MIT (https://raw.githubusercontent.com/Xiaobin-Rong/gtcrn/main/LICENSE) | Ultra-small speech enhancement; sherpa-onnx ships it (not checked) |
| RNNoise v0.2 (xiph/rnnoise; vokra/rnnoise-v0.2 gguf) | 1.4 MB gguf (library weights are compiled in, size of C build not measured) | BSD-3-Clause style (COPYING at https://raw.githubusercontent.com/xiph/rnnoise/main/COPYING; HF mirror tag bsd-3-clause) | Classic, very low compute, lower quality than DeepFilterNet |
| DeepFilterNet3 (Rikorose/DeepFilterNet; soniqo/DeepFilterNet3-ONNX) | deepfilter.onnx 8.6 MB (+0.13 MB aux); upstream tar.gz 7,983,136 bytes | MIT or Apache-2.0 dual (LICENSE-MIT and LICENSE-APACHE at https://github.com/Rikorose/DeepFilterNet) | Best quality of the four, still small |

Pick: Silero VAD 2.2 MB plus GTCRN 0.55 MB for noise suppression. Runner-up: DeepFilterNet3 8.6 MB for higher quality, RNNoise 1.4 MB for classic.
Runtime: ONNX Runtime for Silero, GTCRN, DeepFilterNet3; plain C for RNNoise.

## Job 10: Face detection and landmarks

OS engines: Apple Vision VNDetectFaceLandmarksRequest, Android ML Kit Face Detection and MediaPipe Face Landmarker (the model downloads a .task file), Windows AI/Windows.Media.FaceAnalysis. The fallback only matters on Linux.

| Candidate | Size shipped (exact bytes from HF API / HTTP HEAD) | License | Notes |
|---|---|---|---|
| YuNet 2023mar (opencv/face_detection_yunet) | int8 100,416 B (0.1 MB); fp32 232,589 B | MIT (https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_detection_yunet/LICENSE, "All files in this directory are licensed under MIT"); litert-community YuNet fp16 tflite 256,228 B is tagged bsd-3-clause | Box + 5 landmarks, CPU real-time |
| MediaPipe BlazeFace short range (Google bucket tflite) | 229,746 B | Weights license not stated on the MediaPipe page (page text CC BY 4.0, code samples Apache 2.0); mirrors qualcomm/MediaPipe-Face-Detection and Heliosoph/mediapipe-face-onnx are tagged apache-2.0. Treat as UNVERIFIED at the source | Fast detector |
| MediaPipe Face Landmarker .task bundle (478 landmarks + blendshapes) | 3,758,596 B (3.6 MB, float16) | Same unverified weight license status; MediaPipe repo itself is Apache-2.0 | Use only if 478-point mesh is needed |
| Heliosoph/mediapipe-face-onnx int8 | detector 214,587 + 517,152 data B; 6-point landmark 182,596 + 2,403,868 data B (~3.3 MB total) | apache-2.0 tag, "Apache-2.0 throughout the entire chain" per the card | ONNX route with 6 keypoints, not 478 |

Pick: YuNet int8 0.1 MB (detection + 5 points). Runner-up: MediaPipe Face Landmarker 3.6 MB when dense landmarks are required, after confirming the weight license from Google's model card.
Runtime: ONNX Runtime or OpenCV DNN (YuNet), MediaPipe Tasks / LiteRT.

## Job 11: Document layout and tables

OS engines: Apple VisionKit document scanner and Vision RecognizeDocumentsRequest (newer OS; structure/table output from memory, unverified); Android ML Kit Document Scanner (page crop, not layout); Windows AI text recognition returns lines only (unverified).

REJECTED: DocLayout-YOLO. HF repos juliozhao/DocLayout-YOLO-DocStructBench and darknoah99 are tagged apache-2.0, but the upstream repo (https://github.com/opendatalab/DocLayout-YOLO, LICENSE file) is GNU AGPL v3, built on the Ultralytics/YOLOv10 stack; the tag conflicts with upstream, so fails the gate.

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| PP-DocLayout-S (PaddlePaddle/PP-DocLayout-S; ONNX conversion stefanj0/PP-DocLayout-S-ONNX) | 4,917,852 B onnx (4.6 MiB); official repo inference.pdiparams 4.8 MB | apache-2.0 (https://huggingface.co/PaddlePaddle/PP-DocLayout-S) | PicoDet-S, 23 categories, mAP(0.5) 70.9 on its own test set per card. ONNX is a third-party conversion; the official Paddle files are not ONNX |
| PP-DocLayout-M | 22.2 MiB pdiparams | apache-2.0 | Middle option |
| PP-DocLayout_plus-L ONNX (official) | 129,736,329 B (123.7 MiB) | apache-2.0 | Heavy |
| SLANet_plus table structure (PaddlePaddle/SLANet_plus_onnx) | 7,782,138 B (7.4 MiB) | apache-2.0 | Table to HTML structure, accuracy 63.69 on card, 6.9M storage |
| Table Transformer (microsoft/table-transformer-structure-recognition, -detection) | 115 MB safetensors each (110 MiB) | mit | Heavier usual choice, no int8 shipped (unverified) |
| granite-docling-258M | GGUF Q8_0 177.9 MB + mmproj Q8_0 103.8 MB (~270 MiB); ONNX q4 decoder 99.8 MiB + embed int8 55.1 + vision int8 89.5 | apache-2.0 | Whole-page conversion VLM, far from tiny. Runner-up only if full page-to-markdown is needed |

Pick: PP-DocLayout-S 4.6 MiB + SLANet_plus 7.4 MiB = 12 MiB. Runner-up: Table Transformer 110 MiB, granite-docling for end-to-end.
Runtime: ONNX Runtime.

## Job 12: Names NER, multilingual

OS engines: Apple NLTagger .nameType (language coverage, Hindi/Bengali unverified), Android ML Kit Entity Extraction (dates, addresses, phone etc., not person names), Windows: none known. So this job is mostly a fallback.

| Candidate | Size shipped | License | Languages | Notes |
|---|---|---|---|---|
| knowledgator/gliner-x-small (MT5 encoder, zero-shot GLiNER) | onnx/model_quantized.onnx 172,716,978 B (164.7 MiB); fp32 onnx 656.8 MiB | apache-2.0 (https://huggingface.co/knowledgator/gliner-x-small) | 20 listed incl. Hindi; Bengali NOT listed | Card F1 table (da, de, en, pt, sv, zh): 0.58 to 0.83, beats urchade/gliner_multi-v2.1 |
| urchade/gliner_multi-v2.1 (onnx-community mirror) | model_int8.onnx 332.9 MiB | apache-2.0 on urchade repo; mirror untagged | multilingual | Heavier |
| urchade/gliner_small-v2.1 | int8 174.9 MiB | apache-2.0 | English | Same size as gliner-x-small but English only |
| spaCy xx_ent_wiki_sm | 11,099,332 B whl (10.5 MiB) | mit tag (https://huggingface.co/spacy/xx_ent_wiki_sm); trained on WikiNER whose data license was not reviewed here | Multi-language, Hindi/Bengali coverage not confirmed; labels PER/LOC/ORG/MISC, ents F 83.08 | Tiny; best only for Latin-script languages (unverified) |
| ai4bharat/IndicNER | 636 MiB fp32 (int8 not shipped, ~160 MiB if quantized, estimate unverified) | mit | Indic languages (Hindi and Bengali expected, card language list not found: unverified) | Heavy, closest to a real Hindi/Bengali NER |
| Davlan/xlm-roberta-base-ner-hrl | | afl-3.0 | 10 high-resource languages | AFL not in gate list |

Pick: GLiNER-X small int8 164.7 MiB (flexible entity labels, Hindi covered, no Bengali). Runner-up: spaCy xx_ent_wiki_sm 10.5 MiB for cheap Latin-script names. Bengali: no verified clean model in the small class; use OS NLTagger or accept a gap.
Runtime: ONNX Runtime via the gliner package.

## Job 13: Music stems

OS engines: none public on Apple, Android or Windows (Apple Music's lyrics/instrumental features are not exposed APIs; unverified).

REJECTED: HiDolen/Mini-BS-RoFormer (cc-by-nc-4.0), MansfieldPlumbing Demucs TRT (cc-by-nc-4.0), anvuew BS-RoFormer (gpl-3.0).

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| htdemucs (4 stems, Demucs v4) official | 84,141,911 B (80.2 MiB) .th; GGUF conversions cstr/htdemucs-GGUF: f16 80.4 MiB, q8_0 52.2 MiB, q4_k 37.2 MiB | MIT (https://raw.githubusercontent.com/facebookresearch/demucs/main/LICENSE; README: "Demucs is released under the MIT license"). GGUF repo tagged mit | SDR 9.00 dB on MUSDB HQ (README). Quality of q8_0 and q4_k conversions NOT verified |
| htdemucs_6s (6 stems incl. guitar, piano) | 27M params, GGUF F16 70.8 MiB (thepatch) | mit tag | 6 stems |
| htdemucs_ft | 4 x 42M, GGUF F16 400.8 MiB | mit | Best SDR 9.20 per README, 5x larger |
| Open-Unmix HQ (umxhq) | 33.9 MiB per stem x 4 = 135.5 MiB (aufklarer MLX port); umxl 107.8 MiB x 4 | MIT (https://raw.githubusercontent.com/sigsep/open-unmix-pytorch/master/LICENSE) | Per-target models, larger in total and older, lower quality (from memory) |
| Spleeter | | MIT code (https://raw.githubusercontent.com/deezer/spleeter/master/LICENSE) | Size not checked; older, TensorFlow, lower quality |

Pick: htdemucs, GGUF q8_0 52.2 MiB (q4_k 37.2 MiB if the quality check passes; official fp32 80.2 MiB). Runner-up: Open-Unmix HQ 135 MiB (larger and worse). Nothing materially smaller than htdemucs with a clean license was found.
Runtime: GGUF (demucs.cpp style), ONNX, CoreML conversions exist (dexxdean/htdemucs-coreml 181 MiB fp16).

## Job 14: Frame interpolation

OS engines: Apple VideoToolbox VTFrameProcessor frame interpolation (macOS 15.4+/iOS 26, from memory, unverified); Windows: none; Android: none.

| Candidate | Size shipped | License | Notes |
|---|---|---|---|
| RIFE v4.25 lite (Practical-RIFE weights, Comfy-Org/frame_interpolation rife_v4.25_lite.safetensors) | 22,506,384 B (21.4 MiB) | MIT: https://raw.githubusercontent.com/hzwer/Practical-RIFE/main/LICENSE and README line "The content of these links is under the same MIT license as this project". The Comfy-Org repack tag says `other`; use the mit-tagged mlx-community/RIFE-4.25 (21.6 MiB) or daydreamlive/RIFE flownet.pkl 20.4 MiB for a clean tag | Real-time optical-flow interpolation. ONNX export size not checked |
| RIFE v4.25 / v4.26 | 21.6 MiB | mit | Standard |
| RIFE v4.25 heavy | 82.6 MiB | MIT (same repo) | Quality tier |
| FILM net | film_net_fp16.safetensors 65.6 MiB (Comfy-Org repack tagged other); upstream https://github.com/google-research/frame-interpolation is Apache-2.0 | | Better on large motion, 3x larger |

Pick: RIFE v4.25 lite 21.4 MiB (use the MIT-tagged mirror for fp32 21.6 MiB). Runner-up: FILM 65.6 MiB fp16.
Runtime: PyTorch/MLX safetensors; ONNX/ncnn exports exist (not size-checked).

## B) Summary table (picks)

Sizes in MiB unless noted. Every license cell links the page that was read.

| Job | Pick | Size on disk | License (verified link) | Runtime | OS engine replacing it | Runner-up |
|---|---|---|---|---|---|---|
| 0 Embedding Gemma | EmbeddingGemma 1: REJECT. EmbeddingGemma 2: passes on metadata, not tiny | v2 text q4f16 149, q4 165; v1 q4f16 167 | v1 Gemma Terms https://ai.google.dev/gemma/terms ; v2 Apache-2.0 https://ai.google.dev/gemma/docs/gemma_4_license | ONNX, LiteRT-LM | Apple NLContextualEmbedding | none (optional quality tier) |
| 1 Text embeddings (hi, bn) | granite-embedding-97m-multilingual-r2 int8 | 93 | Apache-2.0 https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2 | ONNX | Apple NLContextualEmbedding (Indic script coverage unverified) | potion-multilingual-128M int8 122 (MIT https://huggingface.co/minishlab/potion-multilingual-128M) ; English-only tier mdbr-leaf-mt 21 |
| 2 Image embeddings | TinyCLIP ViT-40M/32 int8 (English queries; translate hi/bn via OS translation) | 81 | MIT https://raw.githubusercontent.com/microsoft/Cream/main/TinyCLIP/LICENSE ; MobileCLIP rejected (apple-amlr research only https://huggingface.co/apple/MobileCLIP2-S0/raw/main/LICENSE) | ONNX | none for text-to-image (Apple Vision feature print is image-to-image) | SigLIP2 base int8 360, Apache-2.0 https://huggingface.co/google/siglip2-base-patch16-224 |
| 3 OCR | PP-OCRv5 mobile det + latin rec + devanagari rec ONNX; Tesseract ben for Bengali | 4 + 7 + 7 + 0.82 | Apache-2.0 https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx ; https://github.com/tesseract-ocr/tessdata_fast/blob/main/LICENSE | ONNX, libtesseract | Apple Vision text recognition, ML Kit Text v2, Windows OCR | Tesseract fast only (0.8 to 4 per language) |
| 4 Background removal | IS-Net general-use int8 | 42 | MIT (imgly) https://huggingface.co/imgly/isnet-general-onnx ; upstream Apache-2.0 https://github.com/xuebinqin/DIS | ONNX | Apple Vision foreground mask, ML Kit Subject Segmentation, Windows AI | u2netp 4 (Apache-2.0) ; BiRefNet_lite fp16 109 (MIT https://huggingface.co/ZhengPeng7/BiRefNet_lite) |
| 5 Inpainting | MI-GAN 512 Places2 fp16 | 15 | MIT https://huggingface.co/litert-community/MI-GAN-512-Places2-LiteRT | LiteRT / GGUF | none public (Windows AI Object Erase unverified) | LaMa big-lama 197, Apache-2.0 https://huggingface.co/Carve/LaMa-ONNX |
| 6 Upscale | realesr-general-x4v3 | 3 | BSD-3-Clause https://huggingface.co/litert-community/real-esrgan-x4v3-litert | LiteRT, PyTorch, ONNX | Windows AI Super Resolution, Apple VTFrameProcessor (both unverified) | RealPLKSR x4 28, MIT https://huggingface.co/darktable-org/upscale-realplksr-onnx |
| 7 Speech to text | English: Moonshine tiny int8; Hindi and Bengali: IndicConformer int8 each; others: Whisper base q5_1 | 26 ; 131 each ; 56 | MIT https://huggingface.co/moonshine-ai/moonshine-tiny ; MIT https://huggingface.co/OpenVoiceOS/ai4bharat-indicconformer-bn-onnx ; Apache-2.0 https://huggingface.co/openai/whisper-small | ONNX, whisper.cpp | Apple SpeechAnalyzer, Android on-device recognizer, Windows speech | Whisper small q5_1 181; Vosk small hi 42 (Apache-2.0). Parakeet rejected (CC-BY-4.0) |
| 8 Text to speech | English: KittenTTS nano 0.8 int8; Hindi: Kokoro-82M int8 | 26 (23 + 3 voices) ; 88 + ~0.5 per voice | Apache-2.0 https://huggingface.co/KittenML/kitten-tts-nano-0.8-int8 ; Apache-2.0 https://huggingface.co/hexgrad/Kokoro-82M ; Piper voices and KittenTTS 2 rejected | ONNX | Apple AVSpeechSynthesizer, Android TTS, Windows speech | Kokoro for English too. Bengali: none clean found |
| 9 VAD and noise suppression | Silero VAD + GTCRN | 2.2 + 0.55 | MIT https://raw.githubusercontent.com/snakers4/silero-vad/master/LICENSE ; MIT https://raw.githubusercontent.com/Xiaobin-Rong/gtcrn/main/LICENSE | ONNX | OS speech VAD; Apple voice processing, Android NoiseSuppressor | DeepFilterNet3 8.6 (MIT/Apache-2.0 https://github.com/Rikorose/DeepFilterNet) ; RNNoise 1.4 (BSD) |
| 10 Face detection and landmarks | YuNet int8 | 0.1 | MIT https://raw.githubusercontent.com/opencv/opencv_zoo/main/models/face_detection_yunet/LICENSE | ONNX / OpenCV | Apple Vision, ML Kit Face Detection, Windows | MediaPipe Face Landmarker 3.6 (weight license unverified) |
| 11 Document layout and tables | PP-DocLayout-S + SLANet_plus | 4.6 + 7.4 | Apache-2.0 https://huggingface.co/PaddlePaddle/PP-DocLayout-S ; https://huggingface.co/PaddlePaddle/SLANet_plus_onnx. DocLayout-YOLO rejected (AGPL-3.0 upstream https://github.com/opendatalab/DocLayout-YOLO) | ONNX | Apple Vision RecognizeDocumentsRequest (unverified), ML Kit Document Scanner (crop only) | Table Transformer 110 MIT ; granite-docling ~270 Apache-2.0 |
| 12 Names NER | GLiNER-X small int8 (no Bengali) | 164.7 | Apache-2.0 https://huggingface.co/knowledgator/gliner-x-small | ONNX | Apple NLTagger nameType (language coverage unverified) | spaCy xx_ent_wiki_sm 10.5 MIT https://huggingface.co/spacy/xx_ent_wiki_sm |
| 13 Music stems | htdemucs 4 stems, GGUF q8_0 (q4_k 37.2; official 80.2) | 52.2 | MIT https://raw.githubusercontent.com/facebookresearch/demucs/main/LICENSE | GGUF / ONNX / CoreML | none | Open-Unmix HQ 135 for 4 stems (MIT) |
| 14 Frame interpolation | RIFE v4.25 lite | 21.4 | MIT https://raw.githubusercontent.com/hzwer/Practical-RIFE/main/LICENSE | PyTorch/MLX, ONNX export unverified | Apple VTFrameProcessor (unverified) | FILM 65.6 fp16, Apache-2.0 https://github.com/google-research/frame-interpolation |

Gaps and open risks:
- Bengali: no clean small option for NER, TTS or text-to-image directly; STT and OCR have one.
- MI-GAN and IS-Net training-data terms (Places2, DIS5K) were not reviewed.
- EmbeddingGemma 2 card still points at the Gemma Prohibited Use Policy; get a human read before shipping.
- GGUF/ONNX int8 conversions by third parties (IS-Net int8, htdemucs GGUF, Tokenade potion int8, Xenova e5/TinyCLIP) need a local quality spot-check; sizes are real, quality is unmeasured.
- MediaPipe model weights, Silero v6 weights and the espeak-ng phonemizer licenses were not read from primary model cards.

## C) Download totals

Per-job picks summed (MiB): text emb 93, image emb 81, OCR 18.8, bg removal 42, inpaint 15, upscale 3, STT 344 (Moonshine 26 + IndicConformer hi 131 + bn 131 + Whisper base 56), TTS 141 (Kitten 26 + Kokoro 88 + all voices 27), VAD+NS 2.8, face 3.7, doc layout 12, NER 164.7, stems 52.2, interpolation 21.4.

- Linux user installing every fallback, all languages: about 995 MiB (about 1.04 GB decimal). STT (344), NER (165) and TTS (141) are 65% of it.
- Linux user, English only (Moonshine, Kitten, English OCR, no NER): about 389 MiB.
- Mac user, typical: about 173 MiB (TinyCLIP 81, MI-GAN 15, Real-ESRGAN 3, RIFE 21.4, htdemucs q8_0 52.2), assuming Apple's OS engines cover embeddings, OCR, background removal, STT, TTS, VAD, faces, layout and names. About 266 MiB if Hindi/Bengali text embeddings need Granite (93), about 430 MiB if Indic NER is also needed (GLiNER-X 165). The OS coverage assumptions are NOT verified for Hindi and Bengali.
- If the heavy tiers in the summary were used instead (SigLIP2, BiRefNet, LaMa, Whisper small), the Linux total would rise by roughly 650 MiB (SigLIP2 +279, BiRefNet_lite fp16 +67, LaMa +182, Whisper small q5_1 +125 over their tiny picks).
