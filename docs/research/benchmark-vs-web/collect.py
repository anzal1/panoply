#!/usr/bin/env python3
"""Walk $B/runs.jsonl, compute metrics for every run with an output, write
$B/metrics.json and $B/metrics.csv. Rerunnable. Results are cached per output file
(path, size, mtime, METRICS_VERSION) in $B/metrics-cache.json; use --nocache to recompute.

Run with: venv/bin/python -I collect.py
"""
import csv, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # -I drops the script dir
from pathlib import Path
import metrics as M

METRICS_VERSION = 3
B = M.B
RUNS, CACHE = B / "runs.jsonl", B / "metrics-cache.json"
KNOWN = {"pdf-compress", "img-compress", "img-compress-200kb", "jpg-to-webp", "png-to-jpg",
         "bg-remove", "video-compress", "ocr", "pdf-merge"}


def load_runs():
    last = {}
    for line in RUNS.read_text().splitlines():
        try:
            r = json.loads(line)
        except Exception:
            continue
        last[(r.get("site"), r.get("task"), r.get("input"))] = r  # later lines win
    return list(last.values())


def flat(d, prefix=""):
    out = {}
    for k, v in d.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            out.update(flat(v, key + "."))
        elif isinstance(v, (list, tuple)):
            out[key] = json.dumps(v)
        else:
            out[key] = v
    return out


def main():
    nocache = "--nocache" in sys.argv
    cache = {} if nocache or not CACHE.exists() else json.loads(CACHE.read_text())
    rows, ocr_texts = [], {}
    for r in load_runs():
        if r.get("task") not in KNOWN:
            continue
        row = {k: r.get(k) for k in ("site", "task", "input", "output", "seconds", "status", "notes", "date")}
        out = r.get("output")
        if r.get("status") == "ok" and out and (B / out).exists():
            st = (B / out).stat()
            key = f"{out}|{st.st_size}|{st.st_mtime_ns}|{METRICS_VERSION}"
            texts = {}
            if key in cache:
                m, texts = cache[key]["m"], cache[key].get("texts", {})
            else:
                m, texts = M.compute(r["task"], r["input"], out, r.get("seconds"), site=r["site"],
                                     extra_outputs=r.get("extra_outputs") or ())
                cache[key] = {"m": m, "texts": texts}
            row["metrics"] = m
            if r["task"] == "ocr":
                for name, t in texts.items():
                    # only the primary output's text enters the pairwise matrix
                    if out.endswith(name):
                        ocr_texts.setdefault(r["input"], {})[r["site"]] = t
        elif r.get("status") == "ok" and out:
            row["metrics"] = {"metrics_error": "output file missing"}
        rows.append(row)
    CACHE.write_text(json.dumps(cache))
    rows.sort(key=lambda x: (x["task"], x["input"], x["site"]))
    (B / "metrics.json").write_text(json.dumps(rows, indent=1))
    flatrows = [{**{k: v for k, v in r.items() if k != "metrics"}, **flat(r.get("metrics", {}))} for r in rows]
    for fr in flatrows:
        fr.pop("metrics.texts", None)
    cols = []
    for fr in flatrows:
        for k in fr:
            if k not in cols:
                cols.append(k)
    with open(B / "metrics.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=cols)
        w.writeheader()
        for fr in flatrows:
            w.writerow(fr)
    # pairwise OCR CER matrices
    for inp, per_site in sorted(ocr_texts.items()):
        if len(per_site) < 2:
            continue
        sites = sorted(per_site)
        pc = M.pairwise_cer(per_site)
        print(f"\nOCR pairwise CER, input {inp} (row = reference, col = hypothesis)")
        print("%-12s" % "" + "".join("%-12s" % s for s in sites))
        for a in sites:
            print("%-12s" % a + "".join("%-12s" % ("-" if a == b else pc[(a, b)]) for b in sites))
    n_err = sum(1 for r in rows if any(k.endswith("error") for k in r.get("metrics", {})))
    print(f"\n{len(rows)} runs, {sum(1 for r in rows if 'metrics' in r)} scored, {n_err} with metric errors")
    print("wrote", B / "metrics.json", "and", B / "metrics.csv")


if __name__ == "__main__":
    main()
