#!/usr/bin/env python3
"""Print markdown result tables from $B/metrics.json. Run: venv/bin/python -I make_tables.py"""
import json, os, sys
B = os.environ.get("BENCH", "/Users/anzalhussainabidi/personal/panoply-bench-web.noindex")
rows = json.load(open(B + "/metrics.json"))
rows = rows if isinstance(rows, list) else rows["rows"]
SITES = ["panoply", "ilovepdf", "iloveimg", "smallpdf", "ihatepdf.cv", "ihatepdf.com", "tinypng", "remove.bg", "squoosh", "vert"]
def f(x, nd=3):
    if x is None: return "n/a"
    return f"{x:.{nd}f}" if isinstance(x, float) else str(x)
def kb(b): return f"{b/1024:,.0f} KB" if b is not None else "n/a"
COLS = {
 "pdf-compress": [("out", lambda m: kb(m.get("output_bytes"))), ("ratio", lambda m: f(m.get("ratio"), 2)), ("pages", lambda m: f"{m.get('pages_in')}>{m.get('pages_out')}"), ("text kept", lambda m: f(m.get("text_similarity"), 3)), ("render SSIM", lambda m: f(m.get("render_ssim_mean"), 4)), ("render SSIM min", lambda m: f(m.get("render_ssim_min"), 4))],
 "_img": [("out", lambda m: kb(m.get("output_bytes"))), ("ratio", lambda m: f(m.get("ratio"), 2)), ("WxH", lambda m: "x".join(map(str, m.get("out_size") or []))), ("SSIMULACRA2", lambda m: f(m.get("ssimulacra2"), 1)), ("SSIM", lambda m: f(m.get("ssim"), 3)), ("PSNR dB", lambda m: f(m.get("psnr"), 1))],
 "bg-remove": [("out", lambda m: kb(m.get("output_bytes"))), ("WxH", lambda m: "x".join(map(str, m.get("out_size") or []))), ("same size as input", lambda m: str(m.get("size_matches"))), ("alpha>0 coverage", lambda m: f(m.get("fg_coverage_gt0"), 3)), ("semi-transparent px", lambda m: f(m.get("semi_transparent_frac"), 3))],
 "video-compress": [("out", lambda m: kb(m.get("output_bytes"))), ("ratio", lambda m: f(m.get("ratio"), 2)), ("VMAF", lambda m: f(m.get("vmaf_mean"), 1)), ("VMAF min", lambda m: f(m.get("vmaf_min"), 1))],
 "ocr": [("out", lambda m: kb(m.get("output_bytes"))), ("CER", lambda m: f(m.get("cer"), 4)), ("WER", lambda m: f(m.get("wer"), 4)), ("orig image kept", lambda m: str(m.get("original_image_kept"))), ("render SSIM", lambda m: f(m.get("render_ssim_mean"), 4))],
 "pdf-merge": [("out", lambda m: kb(m.get("output_bytes"))), ("size vs inputs", lambda m: f(m.get("ratio"), 2)), ("pages", lambda m: f"{m.get('pages_out')} (sum ok: {m.get('pages_match_sum')})"), ("text sim", lambda m: f(m.get("text_sim_mean"), 3)), ("outline entries", lambda m: str(m.get("outline_entries_out"))), ("page sizes kept", lambda m: str(m.get("page_sizes_preserved")))],
}
def cols(task): return COLS.get(task) or COLS["_img"]
TASKS = ["pdf-compress", "img-compress", "img-compress-200kb", "jpg-to-webp", "png-to-jpg", "bg-remove", "video-compress", "ocr", "pdf-merge"]
for t in TASKS:
    print(f"\n### {t}\n")
    inputs = sorted({r["input"] for r in rows if r["task"] == t})
    c = cols(t)
    print("| input | site | status | s | " + " | ".join(n for n, _ in c) + " |")
    print("|---|---|---|---|" + "---|" * len(c))
    for i in inputs:
        for s in SITES:
            for r in rows:
                if r["task"] == t and r["input"] == i and r["site"] == s:
                    m = r.get("metrics") or {}
                    cells = [fn(m) if m and "error" not in m else "" for _, fn in c]
                    st = r["status"]
                    sec = f(r.get("seconds"), 1) if r.get("seconds") is not None else ""
                    print(f"| {os.path.basename(i.split(',')[0]) if t!='pdf-merge' else 'pair '+('A' if 'a1' in i else 'B')} | {s} | {st} | {sec} | " + " | ".join(cells) + " |")
