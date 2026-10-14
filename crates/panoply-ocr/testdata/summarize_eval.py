"""Turns the JSON lines of run_eval.sh into the tables in docs/research/ocr-results.md.

Usage: python3 summarize_eval.py results.jsonl
"""
import json
import os
import sys
from collections import defaultdict

rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]


def group(r):
    name = os.path.basename(r["input"].split("#")[0])
    stem = name.rsplit(".", 1)[0]
    if "corpus" in r["input"] or "/cache/pdf/" in r["input"]:
        return "corpus scans"
    lang = {"en": "English", "hi": "Hindi", "hien": "Hindi + English", "bn": "Bengali"}[stem.split("-")[0]]
    kind = "scan" if stem.endswith("-scan") else "low-res" if stem.endswith("-low") else "clean"
    return f"{lang}, {kind}"


label = {
    "vision": "Apple Vision (auto language)",
    "ppocr": "PP-OCRv5",
    "tesseract-plain": "Tesseract (defaults)",
    "tesseract": "Tesseract + PP-OCR lines",
}
cells = defaultdict(list)
for r in rows:
    if "cer" not in r:
        continue
    cells[(group(r), r["engine_asked"])].append(r)

groups = sorted({g for g, _ in cells})
engines = ["vision", "ppocr", "tesseract-plain", "tesseract"]
print("| set | n | " + " | ".join(label[e] for e in engines) + " |")
print("|---|---|" + "---|" * len(engines))
for g in groups:
    n = max(len(cells.get((g, e), [])) for e in engines)
    out = []
    for e in engines:
        c = cells.get((g, e))
        out.append("n/a" if not c else f"{100 * sum(x['cer'] for x in c) / len(c):.2f}%")
    print(f"| {g} | {n} | " + " | ".join(out) + " |")

print()
print("Per page: cold = first read in a fresh process (models and runtime loaded), warm = second read; peak = maximum resident set size of the process.")
print()
print("| engine | pages | median cold ms | median warm ms | worst warm ms | max peak MB |")
print("|---|---|---|---|---|---|")
for e in engines:
    rs = [r for (g, en), v in cells.items() if en == e for r in v]
    if not rs:
        continue
    med = lambda xs: sorted(xs)[len(xs) // 2]
    print(f"| {label[e]} | {len(rs)} | {med([r['cold_ms'] for r in rs]):.0f} | {med([r['warm_ms'] for r in rs]):.0f} | {max(r['warm_ms'] for r in rs):.0f} | {max(r['peak_rss_mb'] for r in rs):.0f} |")
print()
print("Full pages (the two Internet Archive scans, 300 DPI render):")
print()
print("| page | engine | CER | WER | words | cold ms | warm ms | peak MB |")
print("|---|---|---|---|---|---|---|---|")
for r in rows:
    if "cer" in r and "ia-scan" in r["input"]:
        p = r["input"].split("/")[-1]
        print(f"| {p} | {r['engine']} | {100*r['cer']:.2f}% | {100*r['wer']:.2f}% | {r['words']} | {r['cold_ms']:.0f} | {r['warm_ms']:.0f} | {r['peak_rss_mb']:.0f} |")
errs = [r for r in rows if "error" in r]
if errs:
    print()
    print("Runs that did not read:")
    for r in errs:
        print(f"- {os.path.basename(r['input'])} with {r['engine_asked']}: {r['error']}")
