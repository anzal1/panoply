"""Objective metrics for the Panoply vs web tools benchmark.

Entry point: compute(task, input_rel, output_rel, seconds=None) -> dict
Every metric group fails soft: on exception the key `<group>_error` is set.

Formulas (stated once, used everywhere):
  ratio      = output_bytes / input_bytes (pdf-merge: / sum of input bytes). Lower is smaller.
  SSIM       = skimage structural_similarity on 8-bit RGB, channel_axis=2, mean over channels,
               data_range=255 (images); grayscale 100 DPI renders for PDFs and OCR.
  PSNR       = skimage peak_signal_noise_ratio, data_range=255, on RGB.
  SSIMULACRA2= libjxl `ssimulacra2` binary (jpeg-xl 0.12.0 via brew), higher is better, 100 = identical.
  Alpha      = flattened onto white for BOTH images before scoring; 16 bit -> 8 bit sRGB the same
               way for both (Pillow decode, then ICC profile -> sRGB if the file carries one).
  CER        = Levenshtein(ref_chars, hyp_chars) / len(ref_chars), whitespace collapsed + stripped.
  WER        = Levenshtein over whitespace tokens / number of reference tokens.
  Text sim   = rapidfuzz Indel normalized similarity (0..1) of whitespace-normalized text.
"""
import io, json, os, re, shutil, subprocess, tempfile, zipfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageCms, ImageOps

B = Path("/Users/anzalhussainabidi/personal/panoply-bench-web.noindex")
INPUTS = B / "inputs"
SCRATCH = Path("/private/tmp/claude-502/panoply-bench-metrics")
NICE = ["nice", "-n", "15"]
SSIMULACRA2 = shutil.which("ssimulacra2") or "/opt/homebrew/bin/ssimulacra2"
Image.MAX_IMAGE_PIXELS = None

LABELS = {
    "scan-lands-p15": INPUTS / "labels/corpus-ia-lands-p15.txt",
    "scan-claims-p9": INPUTS / "labels/corpus-ia-claims-p9.txt",
    "scan-en1": INPUTS / "labels/en-1-scan.txt",
}


def _run(cmd, **kw):
    return subprocess.run(NICE + cmd, capture_output=True, text=True, stdin=subprocess.DEVNULL, **kw)


def soft(fn, *a, **kw):
    try:
        return fn(*a, **kw)
    except Exception as e:  # noqa
        return {"error": f"{type(e).__name__}: {e}"[:300]}


# ---------------------------------------------------------------- basics
def sniff(path):
    h = Path(path).read_bytes()[:64]
    if h[:5] == b"%PDF-": return "pdf"
    if h[:3] == b"\xff\xd8\xff": return "jpeg"
    if h[:8] == b"\x89PNG\r\n\x1a\n": return "png"
    if h[:4] == b"RIFF" and h[8:12] == b"WEBP": return "webp"
    if h[:6] in (b"GIF87a", b"GIF89a"): return "gif"
    if h[:4] in (b"II*\x00", b"MM\x00*"): return "tiff"
    if h[:2] == b"BM": return "bmp"
    if h[4:8] == b"ftyp":
        b = h[8:12]
        if b in (b"avif", b"avis"): return "avif"
        if b in (b"heic", b"heix", b"mif1", b"heif", b"hevc"): return "heic/heif"
        if b in (b"qt  ",): return "mov"
        return "mp4/iso-bmff(" + b.decode("latin1") + ")"
    if h[:4] == b"\x1a\x45\xdf\xa3": return "matroska/webm"
    if h[:4] == b"PK\x03\x04": return "zip(docx/odt?)"
    if h[:15].lower().startswith((b"<!doctype html", b"<html")): return "html"
    try:
        h.decode("utf-8"); return "text"
    except Exception:
        return "unknown"


def basics(task, inputs, output, seconds):
    ins = [INPUTS / i for i in inputs]
    in_b = sum(p.stat().st_size for p in ins)
    out_b = Path(output).stat().st_size
    return dict(input_bytes=in_b, output_bytes=out_b, ratio=round(out_b / in_b, 4) if in_b else None,
                output_format=sniff(output), seconds=seconds)


# ---------------------------------------------------------------- images
_SRGB = ImageCms.createProfile("sRGB")


def _meta_flags(im):
    info = im.info
    exif = bool(info.get("exif")) or bool(info.get("Raw profile type exif")) or bool(im.getexif() if hasattr(im, "getexif") else False)
    xmp = bool(info.get("xmp") or info.get("XML:com.adobe.xmp") or info.get("Raw profile type xmp"))
    alpha = im.mode in ("RGBA", "LA", "PA") or (im.mode == "P" and "transparency" in info)
    return dict(icc=bool(info.get("icc_profile")), exif=exif, xmp=xmp, alpha=alpha,
                mode=im.mode, size=list(im.size))


def load_rgb8_flat(path):
    """Decode, EXIF-transpose, ICC -> sRGB, 8 bit, flatten alpha onto white. Returns (rgb Image, meta)."""
    im = Image.open(path)
    im.load()
    meta = _meta_flags(im)
    if not meta["xmp"]:  # byte search: Pillow's getxmp needs defusedxml
        meta["xmp"] = b"xmpmeta" in Path(path).read_bytes()
    icc = im.info.get("icc_profile")
    try:
        im = ImageOps.exif_transpose(im)
    except Exception:
        pass
    if im.mode in ("I;16", "I;16L", "I;16B", "I"):
        arr = np.asarray(im).astype(np.float64)
        im = Image.fromarray((arr / (65535.0 if arr.max() > 255 else 255.0) * 255).clip(0, 255).astype(np.uint8))
    if icc and im.mode in ("RGB", "RGBA", "L", "LA", "P"):
        try:
            prof = ImageCms.ImageCmsProfile(io.BytesIO(icc))
            if "srgb" not in ImageCms.getProfileDescription(prof).lower():
                a = None
                if im.mode == "RGBA":
                    a = im.getchannel("A")
                base = im.convert("RGB")
                base = ImageCms.profileToProfile(base, prof, _SRGB, outputMode="RGB")
                if a is not None:
                    base.putalpha(a); im = base
                else:
                    im = base
        except Exception:
            pass
    if im.mode in ("RGBA", "LA", "PA", "P"):
        im = im.convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        bg.alpha_composite(im)
        im = bg
    return im.convert("RGB"), meta


def ssimulacra2_score(ref, dist):
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
        a, b = Path(td) / "a.png", Path(td) / "b.png"
        ref.save(a); dist.save(b)
        p = _run([SSIMULACRA2, str(a), str(b)], timeout=600)
        return float(p.stdout.strip().split()[-1])


def _ssim_rgb(a, b):
    from skimage.metrics import structural_similarity, peak_signal_noise_ratio
    A, Bb = np.asarray(a), np.asarray(b)
    ssim = structural_similarity(A, Bb, channel_axis=2, data_range=255)
    psnr = peak_signal_noise_ratio(A, Bb, data_range=255)
    return float(ssim), float(psnr)


def image_metrics(input_rel, output):
    inp = INPUTS / input_rel
    ref, imeta = load_rgb8_flat(inp)
    dist, ometa = load_rgb8_flat(output)
    res = dict(in_size=imeta["size"], out_size=ometa["size"], in_mode=imeta["mode"], out_mode=ometa["mode"],
               in_alpha=imeta["alpha"], out_alpha=ometa["alpha"],
               icc_in=imeta["icc"], icc_out=ometa["icc"], exif_in=imeta["exif"], exif_out=ometa["exif"],
               xmp_in=imeta["xmp"], xmp_out=ometa["xmp"],
               icc_status=("n/a" if not imeta["icc"] else ("kept" if ometa["icc"] else "stripped")),
               exif_status=("n/a" if not imeta["exif"] else ("kept" if ometa["exif"] else "stripped")),
               score_basis="flattened on white, 8-bit sRGB, SSIM = channel-averaged RGB")
    res["downscaled"] = False
    if dist.size != ref.size:
        res["downscaled"] = dist.size[0] * dist.size[1] < ref.size[0] * ref.size[1]
        res["resized_for_scoring"] = True
        dist = dist.resize(ref.size, Image.LANCZOS)
    res["ssim"], res["psnr"] = (round(v, 5) for v in _ssim_rgb(ref, dist))
    if res["psnr"] == float("inf"):
        res["psnr"] = None; res["psnr_note"] = "identical pixels"
    res["ssimulacra2"] = soft(ssimulacra2_score, ref, dist)
    if isinstance(res["ssimulacra2"], dict):
        res["ssimulacra2_error"] = res.pop("ssimulacra2")["error"]
        res["ssimulacra2"] = None
    return res


# ---------------------------------------------------------------- pdf helpers
def norm_ws(s):
    return re.sub(r"\s+", " ", s or "").strip()


def pdf_text_pages(path):
    p = _run(["pdftotext", "-enc", "UTF-8", str(path), "-"], timeout=600)
    if p.returncode != 0 and not p.stdout:
        raise RuntimeError("pdftotext: " + p.stderr[:150])
    pages = p.stdout.split("\f")
    if pages and pages[-1] == "":
        pages = pages[:-1]
    return pages


def pdf_pages(path):
    from pypdf import PdfReader
    return len(PdfReader(str(path)).pages)


def text_sim(a, b):
    from rapidfuzz.distance import Indel
    a, b = norm_ws(a), norm_ws(b)
    if not a and not b:
        return 1.0
    return float(Indel.normalized_similarity(a, b))


_SZ = {"B": 1, "K": 1024, "M": 1024 ** 2, "G": 1024 ** 3}


def pdfimages_list(path):
    p = _run(["pdfimages", "-list", str(path)], timeout=300)
    lines = p.stdout.splitlines()[2:]
    n, tot = 0, 0
    for ln in lines:
        parts = ln.split()
        if len(parts) < 14:
            continue
        n += 1
        s = parts[13]
        try:
            tot += int(float(s[:-1]) * _SZ.get(s[-1], 1)) if s[-1] in _SZ else int(float(s))
        except Exception:
            pass
    return n, tot


def pdffonts_count(path):
    p = _run(["pdffonts", str(path)], timeout=120)
    return max(0, len(p.stdout.splitlines()) - 2)


def render_pages(path, idxs, dpi=100):
    """Render 1-based page indexes to grayscale arrays."""
    out = []
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
        for i in idxs:
            pref = Path(td) / f"p{i}"
            _run(["pdftoppm", "-gray", "-r", str(dpi), "-f", str(i), "-l", str(i), "-png", str(path), str(pref)], timeout=300)
            fs = sorted(Path(td).glob(f"p{i}*.png"))
            out.append(Image.open(fs[0]).convert("L").copy() if fs else None)
            for f in fs:
                f.unlink()
    return out


def evenly(n, k=10):
    if n <= k:
        return list(range(1, n + 1))
    return sorted({int(round(1 + j * (n - 1) / (k - 1))) for j in range(k)})


def mean_render_ssim(ref_pdf, out_pdf, n_ref, n_out, dpi=100):
    from skimage.metrics import structural_similarity
    idx = evenly(n_ref)
    ref_imgs = render_pages(ref_pdf, idx, dpi)
    out_idx = [max(1, min(n_out, int(round(i * n_out / n_ref)))) if n_out != n_ref else i for i in idx]
    out_imgs = render_pages(out_pdf, out_idx, dpi)
    sc = []
    for a, b in zip(ref_imgs, out_imgs):
        if a is None or b is None:
            continue
        if b.size != a.size:
            b = b.resize(a.size, Image.LANCZOS)
        sc.append(structural_similarity(np.asarray(a), np.asarray(b), data_range=255))
    return dict(pages_sampled=len(sc), render_ssim_mean=round(float(np.mean(sc)), 5) if sc else None,
                render_ssim_min=round(float(np.min(sc)), 5) if sc else None)


# ---------------------------------------------------------------- pdf-compress
def pdf_compress_metrics(input_rel, output):
    inp = INPUTS / input_rel
    res = {}
    n_in, n_out = pdf_pages(inp), pdf_pages(output)
    res.update(pages_in=n_in, pages_out=n_out, page_count_differs=(n_in != n_out))
    t_in, t_out = pdf_text_pages(inp), pdf_text_pages(output)
    a, b = norm_ws(" ".join(t_in)), norm_ws(" ".join(t_out))
    res.update(text_chars_in=len(a), text_chars_out=len(b), text_similarity=round(text_sim(a, b), 5),
               selectable_text_in=len(a) >= 20, selectable_text_out=len(b) >= 20)
    res.update(soft(mean_render_ssim, inp, output, n_in, n_out))
    res.update(fonts_in=pdffonts_count(inp), fonts_out=pdffonts_count(output))
    (ni, bi), (no, bo) = pdfimages_list(inp), pdfimages_list(output)
    res.update(images_in=ni, image_bytes_in=bi, images_out=no, image_bytes_out=bo)
    return res


# ---------------------------------------------------------------- video
def ffprobe(path):
    p = _run(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)], timeout=60)
    return json.loads(p.stdout)


def _vsummary(path):
    d = ffprobe(path)
    v = next(s for s in d["streams"] if s["codec_type"] == "video")
    a = [s for s in d["streams"] if s["codec_type"] == "audio"]
    dur = float(v.get("duration") or d["format"].get("duration") or 0)
    size = int(d["format"]["size"])
    vbr = v.get("bit_rate")
    if vbr is None:
        abr = sum(int(s.get("bit_rate") or 0) for s in a)
        fdur = float(d["format"].get("duration") or dur or 1)
        vbr = max(0, size * 8 / fdur - abr)
    num, den = (v.get("r_frame_rate") or "0/1").split("/")
    fps = float(num) / float(den) if float(den) else 0.0
    return dict(codec=v["codec_name"], width=v["width"], height=v["height"], fps=round(fps, 3), duration=round(dur, 3),
                v_bitrate_kbps=round(float(vbr) / 1000, 1), audio=bool(a),
                audio_codec=(a[0]["codec_name"] if a else None), pix_fmt=v.get("pix_fmt"))


def video_metrics(input_rel, output):
    inp = INPUTS / input_rel
    i, o = _vsummary(inp), _vsummary(output)
    res = {"in": i, "out": o, "audio_kept": (o["audio"] or not i["audio"]),
           "duration_delta_s": round(o["duration"] - i["duration"], 3),
           "bitrate_ratio": round(o["v_bitrate_kbps"] / i["v_bitrate_kbps"], 4) if i["v_bitrate_kbps"] else None,
           "resolution_changed": (i["width"], i["height"]) != (o["width"], o["height"])}
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=SCRATCH) as td:
        log = Path(td) / "v.json"
        fg = (f"[0:v]scale={i['width']}:{i['height']}:flags=bicubic,fps={i['fps']},format=yuv420p,setpts=PTS-STARTPTS[d];"
              f"[1:v]format=yuv420p,setpts=PTS-STARTPTS[r];"
              f"[d][r]libvmaf=n_threads=4:log_fmt=json:log_path={log}:feature='name=psnr|name=float_ssim'")
        p = _run(["ffmpeg", "-hide_banner", "-v", "error", "-i", str(output), "-i", str(inp), "-lavfi", fg, "-f", "null", "-"], timeout=1800)
        if not log.exists():
            res["quality_error"] = (p.stderr or "no log")[:300]
        else:
            j = json.loads(log.read_text())
            pm = j["pooled_metrics"]
            res["vmaf_mean"] = round(pm["vmaf"]["mean"], 3)
            res["vmaf_min"] = round(pm["vmaf"]["min"], 3)
            res["psnr_y_mean"] = round(pm["psnr_y"]["mean"], 3) if "psnr_y" in pm else None
            res["ssim_mean"] = round(pm["float_ssim"]["mean"], 5) if "float_ssim" in pm else None
            res["frames_scored"] = len(j.get("frames", []))
            res["quality_basis"] = "output scaled to input res, fps matched, yuv420p; vmaf_v0.6.1; n_threads=4"
    return res


# ---------------------------------------------------------------- ocr
def lev_ratio(ref, hyp):
    from rapidfuzz.distance import Levenshtein
    return Levenshtein.distance(ref, hyp) / max(1, len(ref))


def cer(ref, hyp):
    return lev_ratio(norm_ws(ref), norm_ws(hyp))


def wer(ref, hyp):
    from rapidfuzz.distance import Levenshtein
    r, h = norm_ws(ref).split(), norm_ws(hyp).split()
    return Levenshtein.distance(r, h) / max(1, len(r))


def read_text_any(path):
    path = Path(path)
    kind = sniff(path)
    if kind == "pdf":
        return " ".join(pdf_text_pages(path))
    if kind.startswith("zip"):
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            if "word/document.xml" in names:
                x = z.read("word/document.xml").decode("utf8", "ignore")
                x = re.sub(r"</w:p>", "\n", x)
                return re.sub(r"<[^>]+>", "", x)
            if "content.xml" in names:
                return re.sub(r"<[^>]+>", " ", z.read("content.xml").decode("utf8", "ignore"))
        raise RuntimeError("unknown zip layout")
    raw = path.read_bytes().decode("utf-8", "ignore")
    if kind == "html":
        raw = re.sub(r"(?is)<(script|style).*?</\1>", "", raw)
        raw = re.sub(r"<[^>]+>", " ", raw)
        import html as _h
        raw = _h.unescape(raw)
    return raw


def ocr_metrics(input_rel, output, extra_outputs=()):
    inp = INPUTS / input_rel
    base = inp.stem
    ref = LABELS[base].read_text(encoding="utf-8", errors="ignore") if base in LABELS else None
    res = {"ground_truth": str(LABELS[base].relative_to(INPUTS)) if ref else None,
           "ground_truth_note": ("typed by the Panoply team" if base in ("scan-lands-p15", "scan-claims-p9") else "provided en-1-scan label") if ref else None,
           "cer_formula": "Levenshtein(chars)/len(ref chars), whitespace collapsed", "wer_formula": "Levenshtein(tokens)/len(ref tokens)"}
    texts = {}
    outputs = [output] + [B / e if not str(e).startswith("/") else Path(e) for e in extra_outputs]
    for o in outputs:
        o = Path(o) if str(o).startswith("/") else B / o
        if not o.exists():
            continue
        try:
            t = read_text_any(o)
        except Exception as e:
            texts[o.name] = (None, f"{type(e).__name__}: {e}")
            continue
        texts[o.name] = (t, None)
    res["texts"] = {}
    for name, (t, err) in texts.items():
        d = {"error": err} if err else {"chars": len(norm_ws(t))}
        if t is not None and ref:
            d["cer"], d["wer"] = round(cer(ref, t), 5), round(wer(ref, t), 5)
        res["texts"][name] = d
    # headline = the primary output's own text (PDF text layer or the file itself)
    prim = texts.get(Path(output).name)
    if prim and prim[0] is not None:
        res["text_chars"] = len(norm_ws(prim[0]))
        if ref:
            res["cer"], res["wer"] = round(cer(ref, prim[0]), 5), round(wer(ref, prim[0]), 5)
    elif prim and prim[1]:
        res["text_error"] = prim[1]
    # pdf-specific
    if sniff(output) == "pdf":
        n_in, n_out = pdf_pages(inp), pdf_pages(output)
        res.update(pages_in=n_in, pages_out=n_out, page_count_differs=n_in != n_out)
        res.update(soft(mean_render_ssim, inp, output, n_in, n_out))
        (ni, bi), (no, bo) = pdfimages_list(inp), pdfimages_list(output)
        res.update(images_in=ni, image_bytes_in=bi, images_out=no, image_bytes_out=bo,
                   image_bytes_ratio=round(bo / bi, 4) if bi else None,
                   original_image_kept=("likely (image bytes within 10% of input)" if bi and abs(bo / bi - 1) <= 0.10 else
                                        "re-encoded or recompressed"))
    return res, {k: v[0] for k, v in texts.items() if v[0] is not None}


def pairwise_cer(texts):
    """texts: {tool: text} for ONE input. Returns {(a, b): CER(a as ref, b as hyp)} for a != b."""
    out = {}
    for a, ta in texts.items():
        for b, tb in texts.items():
            if a != b:
                out[(a, b)] = round(cer(ta, tb), 4)
    return out


# ---------------------------------------------------------------- bg remove
def load_alpha(path):
    im = Image.open(path)
    im.load()
    if im.mode in ("RGBA", "LA", "PA") or (im.mode == "P" and "transparency" in im.info):
        return np.asarray(im.convert("RGBA").getchannel("A"))
    return None


def alpha_iou(a, b, thr=127):
    """a, b: paths or alpha arrays (uint8). Resize b to a's size, binarize at alpha>thr, return IoU."""
    A = load_alpha(a) if isinstance(a, (str, Path)) else a
    Bm = load_alpha(b) if isinstance(b, (str, Path)) else b
    if A is None or Bm is None:
        return None
    if Bm.shape != A.shape:
        Bm = np.asarray(Image.fromarray(Bm).resize((A.shape[1], A.shape[0]), Image.BILINEAR))
    x, y = A > thr, Bm > thr
    u = np.logical_or(x, y).sum()
    return float(np.logical_and(x, y).sum() / u) if u else 1.0


def checker(size, tile=24):
    w, h = size
    yy, xx = np.mgrid[0:h, 0:w]
    c = (((xx // tile) + (yy // tile)) % 2) * 40 + 200
    return Image.fromarray(np.stack([c] * 3, -1).astype(np.uint8))


def make_preview(input_rel, output, site, label_size=520):
    out = Path(output)
    rgba = Image.open(out)
    rgba.load()
    rgba = rgba.convert("RGBA")
    src = Image.open(INPUTS / input_rel).convert("RGB")
    h = label_size
    def fit(im):
        return im.resize((max(1, round(im.width * h / im.height)), h), Image.LANCZOS)
    src, rgba = fit(src), fit(rgba)
    matte = fit(Image.fromarray(np.asarray(rgba.getchannel("A")))).convert("RGB")
    gray = Image.new("RGBA", rgba.size, (128, 128, 128, 255)); gray.alpha_composite(rgba)
    chk = checker(rgba.size).convert("RGBA"); chk.alpha_composite(rgba)
    tiles = [src, matte, gray.convert("RGB"), chk.convert("RGB")]
    W = sum(t.width for t in tiles) + 8 * (len(tiles) - 1)
    canvas = Image.new("RGB", (W, h), (30, 30, 30))
    x = 0
    for t in tiles:
        canvas.paste(t, (x, 0)); x += t.width + 8
    pdir = B / site / "bg-remove" / "preview"
    pdir.mkdir(parents=True, exist_ok=True)
    dest = pdir / (out.stem + "__preview.png")
    canvas.save(dest)
    return str(dest.relative_to(B))


def bg_metrics(input_rel, output, site):
    inp = INPUTS / input_rel
    iw, ih = Image.open(inp).size
    im = Image.open(output); im.load()
    res = dict(in_size=[iw, ih], out_size=list(im.size), size_matches=(im.size == (iw, ih)), mode=im.mode)
    a = load_alpha(output)
    res["has_alpha"] = a is not None
    if a is not None:
        res["fg_coverage_gt0"] = round(float((a > 0).mean()), 4)
        res["fg_coverage_gt127"] = round(float((a > 127).mean()), 4)
        res["semi_transparent_frac"] = round(float(((a > 0) & (a < 255)).mean()), 4)
        res["edge_softness_note"] = "semi_transparent_frac = share of pixels with 0<alpha<255 (soft-edge proxy)"
    res["preview"] = soft(make_preview, input_rel, output, site)
    return res


# ---------------------------------------------------------------- pdf-merge
def merge_metrics(input_rel, output):
    from pypdf import PdfReader
    ins = [INPUTS / p for p in re.split(r"[+,]", input_rel)]
    res = {}
    n_ins = [pdf_pages(p) for p in ins]
    n_out = pdf_pages(output)
    res.update(pages_in=n_ins, pages_out=n_out, pages_match_sum=(sum(n_ins) == n_out))
    ref_pages = [t for p in ins for t in pdf_text_pages(p)]
    out_pages = pdf_text_pages(output)
    k = min(len(ref_pages), len(out_pages))
    sims = [text_sim(ref_pages[i], out_pages[i]) for i in range(k)]
    res.update(text_pages_compared=k, text_exact_pages=int(sum(s == 1.0 for s in sims)),
               text_sim_mean=round(float(np.mean(sims)), 5) if sims else None,
               text_sim_min=round(float(np.min(sims)), 5) if sims else None)
    r = PdfReader(str(output))
    try:
        res["outline_entries_out"] = len(r.outline)
    except Exception as e:
        res["outline_error"] = str(e)[:100]
    def outline_len(p):
        try:
            return len(PdfReader(str(p)).outline)
        except Exception:
            return None
    res["outline_entries_in"] = [outline_len(p) for p in ins]
    in_sizes = [tuple(round(float(x), 1) for x in (pg.mediabox.width, pg.mediabox.height)) for p in ins for pg in PdfReader(str(p)).pages]
    out_sizes = [tuple(round(float(x), 1) for x in (pg.mediabox.width, pg.mediabox.height)) for pg in r.pages]
    res["page_sizes_preserved"] = in_sizes == out_sizes
    # annotations survive? (pair B contains a highlight)
    def n_annots(p):
        n = 0
        for pg in PdfReader(str(p)).pages:
            a = pg.get("/Annots")
            n += len(a.get_object()) if a is not None else 0
        return n
    res["annots_in"] = sum(n_annots(p) for p in ins)
    res["annots_out"] = soft(n_annots, output) if True else None
    return res


# ---------------------------------------------------------------- dispatcher
IMAGE_TASKS = {"img-compress", "img-compress-200kb", "jpg-to-webp", "png-to-jpg"}


def compute(task, input_rel, output_rel, seconds=None, site=None, extra_outputs=()):
    """Returns (metrics dict, texts dict (ocr only))."""
    out = B / output_rel
    inputs = re.split(r"[+,]", input_rel)  # pair inputs: "a+b" (Panoply) or "a,b" (web log)
    m = {"task": task}
    texts = {}
    g = soft(basics, task, inputs, out, seconds)
    m.update(g if "error" not in g else {"basics_error": g["error"]})
    if task in IMAGE_TASKS:
        r = soft(image_metrics, inputs[0], out)
    elif task == "pdf-compress":
        r = soft(pdf_compress_metrics, inputs[0], out)
    elif task == "video-compress":
        r = soft(video_metrics, inputs[0], out)
    elif task == "ocr":
        rr = soft(ocr_metrics, inputs[0], out, extra_outputs)
        if isinstance(rr, tuple):
            r, texts = rr
        else:
            r = rr
    elif task == "bg-remove":
        r = soft(bg_metrics, inputs[0], out, site or "unknown")
    elif task == "pdf-merge":
        r = soft(merge_metrics, input_rel, out)
    else:
        r = {"error": f"unknown task {task}"}
    if "error" in r and len(r) == 1:
        m["metrics_error"] = r["error"]
    else:
        m.update(r)
    return m, texts
