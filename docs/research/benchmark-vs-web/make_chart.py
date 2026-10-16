#!/usr/bin/env python3
"""Summary chart from $B/metrics.json -> summary.png (small multiples, one panel per task).
Works with partial data. Run with: venv/bin/python -I make_chart.py
"""
import json, math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

B = Path("/Users/anzalhussainabidi/personal/panoply-bench-web.noindex")
OUT = Path(__file__).resolve().parent / "summary.png"
PANOPLY, OTHER = "#c4622d", "#8a8f98"
INK, GRID = "#1d1d1f", "#e4e4e7"

# task, title, getter(metrics row)->value, y label, log scale
PANELS = [
    ("pdf-compress", "PDF compress: size ratio", lambda r: r["metrics"].get("ratio"), "output / input (lower = smaller)", False),
    ("img-compress", "Image compress: size ratio", lambda r: r["metrics"].get("ratio"), "output / input", False),
    ("img-compress-200kb", "Image to under 200 KB: SSIMULACRA2", lambda r: r["metrics"].get("ssimulacra2"), "higher = closer to original", False),
    ("jpg-to-webp", "JPG to WebP: size ratio", lambda r: r["metrics"].get("ratio"), "output / input", False),
    ("png-to-jpg", "PNG to JPG: size ratio", lambda r: r["metrics"].get("ratio"), "output / input", False),
    ("bg-remove", "Background removal: seconds", lambda r: r.get("seconds"), "seconds (log)", True),
    ("video-compress", "Video compress: size ratio", lambda r: r["metrics"].get("ratio"), "output / input", False),
    ("ocr", "OCR: character error rate", lambda r: r["metrics"].get("cer"), "CER (lower = better)", False),
    ("pdf-merge", "PDF merge: seconds", lambda r: r.get("seconds"), "seconds (log)", True),
]


def main():
    rows = json.loads((B / "metrics.json").read_text())
    ok = [r for r in rows if r.get("status") == "ok" and r.get("metrics") and r["task"] in {p[0] for p in PANELS}]
    sites = sorted({r["site"] for r in rows}, key=lambda s: (s != "panoply", s))
    fig, axes = plt.subplots(4, 3, figsize=(16, 15), dpi=100)
    flat = axes.flatten()
    for ax, (task, title, get, ylab, log) in zip(flat, PANELS):
        ax.set_title(title, fontsize=12, color=INK, loc="left", fontweight="bold")
        have = {}
        for r in ok:
            if r["task"] != task:
                continue
            try:
                v = get(r)
            except Exception:
                v = None
            if isinstance(v, (int, float)) and not (isinstance(v, float) and math.isnan(v)):
                have.setdefault(r["site"], []).append(v)
        for i, s in enumerate(sites):
            vals = have.get(s)
            col = PANOPLY if s == "panoply" else OTHER
            if not vals:
                ax.text(i, 0.02, "no data", transform=ax.get_xaxis_transform(), ha="center", va="bottom", fontsize=9, color="#9a9aa2", rotation=90)
                continue
            m = sum(vals) / len(vals)
            ax.bar(i, m, width=0.55, color=col, alpha=0.28, zorder=2)
            n = len(vals)
            for j, v in enumerate(vals):
                ax.scatter(i + (j - (n - 1) / 2) * 0.12, v, s=36, color=col, edgecolor="white", linewidth=0.6, zorder=3)
            ax.annotate(f"{m:.3g}", (i, m), textcoords="offset points", xytext=(0, 7), ha="center", fontsize=9, color=INK)
        ax.set_xticks(range(len(sites)))
        ax.set_xticklabels(sites, fontsize=9, rotation=20, ha="right")
        for lab, s in zip(ax.get_xticklabels(), sites):
            if s == "panoply":
                lab.set_color(PANOPLY); lab.set_fontweight("bold")
        ax.set_ylabel(ylab, fontsize=9, color="#555")
        if log:
            ax.set_yscale("log")
        else:
            ax.set_ylim(bottom=0 if task != "img-compress-200kb" else min(0, min([v for vs in have.values() for v in vs] + [0])))
        ax.grid(axis="y", color=GRID, zorder=0)
        for sp in ("top", "right"):
            ax.spines[sp].set_visible(False)
        ax.tick_params(labelsize=9)
    # bottom row: seconds for the remaining tasks
    sec_tasks = [("pdf-compress", "PDF compress"), ("img-compress", "Image compress"), ("jpg-to-webp", "JPG to WebP"),
                 ("png-to-jpg", "PNG to JPG"), ("video-compress", "Video compress"), ("ocr", "OCR")]
    for ax in flat[9:]:
        ax.axis("off")
    gs = axes[3, 0].get_gridspec()
    axs = fig.add_subplot(gs[3, :])
    w = 0.8 / max(1, len(sites))
    for k, s in enumerate(sites):
        for t, (task, label) in enumerate(sec_tasks):
            vals = [r["seconds"] for r in ok if r["task"] == task and r["site"] == s and isinstance(r.get("seconds"), (int, float))]
            if vals:
                axs.bar(t + (k - (len(sites) - 1) / 2) * w, sum(vals) / len(vals), width=w * 0.9,
                        color=PANOPLY if s == "panoply" else OTHER, alpha=1.0 if s == "panoply" else 0.45 + 0.1 * (k % 3), zorder=2,
                        label=s if t == 0 else None)
    axs.set_yscale("log")
    axs.set_xticks(range(len(sec_tasks)))
    axs.set_xticklabels([l for _, l in sec_tasks])
    axs.set_title("Mean wall time per file (seconds, log; web = upload start to download complete, Panoply = local CLI process)", fontsize=11, loc="left", fontweight="bold", color=INK)
    axs.grid(axis="y", color=GRID, zorder=0)
    for sp in ("top", "right"):
        axs.spines[sp].set_visible(False)
    axs.legend(fontsize=9, frameon=False, ncol=len(sites))
    fig.suptitle("Panoply (on device, v0.2.0-beta CLI) vs web file tools", fontsize=15, x=0.01, ha="left", color=INK, fontweight="bold")
    fig.text(0.01, 0.005, "Dots = individual files, bar = mean. Panoply in copper. Blank = site not run for this task. Sources: metrics.json from the shared bench folder.", fontsize=9, color="#666")
    fig.tight_layout(rect=(0, 0.015, 1, 0.97))
    fig.savefig(OUT)
    print("wrote", OUT)


if __name__ == "__main__":
    main()
