#!/usr/bin/env python3
"""Run the Panoply side of the web-tool benchmark.

One process per file, default settings, `nice -n 15`, one heavy job at a time
(file lock shared with other agents). Appends one JSON line per run to
$B/runs.jsonl. Idempotent: skips (site, task, input) already logged unless --force.

Usage: python3 -I run_panoply.py [--force] [--only TASK[,TASK]]
"""
import argparse, datetime, fcntl, json, os, re, shutil, subprocess, sys, time
from pathlib import Path

B = Path("/Users/anzalhussainabidi/personal/panoply-bench-web.noindex")
PANOPLY = B / "panoply/x/panoply-cli-0.2.0-beta-macos-arm64/panoply"
INPUTS = B / "inputs"
RUNS = B / "runs.jsonl"
LOCK = Path("/private/tmp/claude-502/panoply-bench-heavy.lock")
SCRATCH = Path("/private/tmp/claude-502/panoply-bench-cwd")
SITE = "panoply"
TIMEOUT = 1200

# task -> list of (input spec, argv builder). argv gets {out} substituted.
def run_mod(module, *opts):
    def f(files, out):
        a = ["run", module, *files, "--out", out]
        for o in opts:
            a += ["--opt", o]
        return a
    return f

def sub(*words, flags=()):
    def f(files, out):
        return [*words, *files, *flags, "--out", out]
    return f

PDF3 = ["pdf/irs-p17-2022.pdf", "pdf/image-streams.pdf", "pdf/ia-scan-public-lands.pdf"]
TASKS = {
    "pdf-compress": (PDF3, sub("pdf", "compress")),
    "img-compress": (["img/antonelli.jpg", "img/an-teallach-panorama.jpg", "img/rgba-16bit.png"],
                     run_mod("image.compress")),
    "img-compress-200kb": (["img/antonelli.jpg", "img/an-teallach-panorama.jpg"],
                           run_mod("image.compress", "target=under_size", "max_size=200kb")),
    "jpg-to-webp": (["img/antonelli.jpg", "img/an-teallach-panorama.jpg"],
                    run_mod("image.convert", "to=webp")),
    "png-to-jpg": (["img/paris-icc-exif-xmp.png", "img/rgba-16bit.png"],
                   run_mod("image.convert", "to=jpg")),
    "bg-remove": (["img/antonelli.jpg", "img/borman.jpg"], sub("cutout")),
    "video-compress": (["video/sintel-720p-h264-10s.mp4", "video/sintel-trailer-480p.mp4"],
                       sub("video", "compress", flags=("--no-measure",))),
    "ocr": (["ocr/scan-lands-p15.pdf", "ocr/scan-claims-p9.pdf", "ocr/scan-en1.pdf"],
            sub("pdf", "ocr", flags=("--text-file",))),
    "pdf-merge": ([("merge/a1-irs-f1040-2022.pdf", "merge/a2-irs-p17-2022.pdf"),
                   ("merge/b1-linearized.pdf", "merge/b2-annotation-highlight.pdf")],
                  sub("pdf", "merge", flags=("--given-order",))),
}

def now():
    return datetime.datetime.now().astimezone().isoformat(timespec="seconds")

def load_runs():
    if not RUNS.exists():
        return []
    out = []
    for line in RUNS.read_text().splitlines():
        try:
            out.append(json.loads(line))
        except Exception:
            pass
    return out

def log(rec):
    with open(RUNS, "a") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        f.write(json.dumps(rec) + "\n")
        fcntl.flock(f, fcntl.LOCK_UN)

def snapshot(d):
    return {p: p.stat().st_mtime_ns for p in d.rglob("*") if p.is_file()}

def execute(task, spec, builder, outdir, extra_notes="", score=True):
    files = [spec] if isinstance(spec, str) else list(spec)
    input_key = "+".join(files)
    abs_files = [str(INPUTS / f) for f in files]
    outdir.mkdir(parents=True, exist_ok=True)
    before = snapshot(outdir)
    argv = ["nice", "-n", "15", str(PANOPLY), *builder(abs_files, str(outdir))]
    SCRATCH.mkdir(parents=True, exist_ok=True)
    LOCK.parent.mkdir(parents=True, exist_ok=True)
    with open(LOCK, "w") as lk:
        fcntl.flock(lk, fcntl.LOCK_EX)  # one heavy job at a time
        t0 = time.perf_counter()
        try:
            p = subprocess.run(argv, cwd=SCRATCH, stdin=subprocess.DEVNULL,
                               capture_output=True, text=True, timeout=TIMEOUT)
            rc, so, se = p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired as e:
            rc, so, se = -9, (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "timeout"
        secs = time.perf_counter() - t0
        fcntl.flock(lk, fcntl.LOCK_UN)
    after = snapshot(outdir)
    new = sorted(p for p, m in after.items() if before.get(p) != m)
    text = (so + "\n" + se).strip()
    status, output, extras = "failed", None, []
    low = text.lower()
    if rc == 0 and new:
        status = "ok"
        # primary = largest non-txt output; txt etc. as extras
        prim = [p for p in new if p.suffix.lower() not in (".txt", ".json")] or new
        prim.sort(key=lambda p: -p.stat().st_size)
        rel = lambda p: str(p.relative_to(B)) if B in p.parents else str(p)
        output = rel(prim[0])
        extras = [rel(p) for p in new if p != prim[0]]
    elif "download" in low and ("--yes" in low or "refus" in low or "ask" in low):
        status = "blocked"
    if not score:
        output, extras = None, []
    notes = " | ".join(l.strip() for l in text.splitlines() if l.strip())[:1500]
    rec = dict(site=SITE, task=task, input=input_key, output=output, seconds=round(secs, 3),
               status=status, notes=(extra_notes + " " + notes).strip(), date=now(),
               rc=rc, argv=argv[3:], extra_outputs=extras)
    log(rec)
    return rec

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--only", default="")
    ap.add_argument("--measured-video", action="store_true",
                    help="also run one video compress WITH Panoply's own quality measure (logged as video-compress-measured)")
    a = ap.parse_args()
    only = set(filter(None, a.only.split(",")))
    done = {(r["site"], r["task"], r["input"]) for r in load_runs()}
    # cold OCR first: order as given, ocr is first-in-process only if run alone; note it.
    for task, (inputs, builder) in TASKS.items():
        if only and task not in only:
            continue
        for i, spec in enumerate(inputs):
            key = "+".join([spec] if isinstance(spec, str) else spec)
            if (SITE, task, key) in done and not a.force:
                print("skip", task, key); continue
            notes = ""
            if task == "ocr" and i == 0:
                notes = "(first OCR run in this process; model/engine cache state unknown)"
            r = execute(task, spec, builder, B / SITE / task, notes)
            print(f"{r['status']:8s} {task:20s} {key:50s} {r['seconds']:8.2f}s -> {r['output']}")
    if a.measured_video:
        scratch_out = SCRATCH / "measured-video"
        shutil.rmtree(scratch_out, ignore_errors=True)
        rec = execute("video-compress-measured", "video/sintel-720p-h264-10s.mp4",
                      sub("video", "compress"), scratch_out, "with Panoply quality measure ON (timing not comparable).", score=False)
        print("measured video:", rec["status"], rec["seconds"], rec["notes"][:300])

if __name__ == "__main__":
    main()
