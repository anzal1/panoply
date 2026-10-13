"""Shared helpers for the checkers. Standard library plus Pillow and numpy only."""
import json, os, subprocess, sys

FF = "/opt/homebrew/bin/ffmpeg"
FP = "/opt/homebrew/bin/ffprobe"
PDFINFO = "/opt/homebrew/bin/pdfinfo"
PDFTOTEXT = "/opt/homebrew/bin/pdftotext"
PDFIMAGES = "/opt/homebrew/bin/pdfimages"
PDFFONTS = "/opt/homebrew/bin/pdffonts"


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=kw.pop("text", True), **kw)


def probe(path):
    r = run([FP, "-v", "error", "-show_format", "-show_streams", "-of", "json", path])
    if r.returncode != 0:
        raise RuntimeError("ffprobe failed: " + r.stderr.strip()[:200])
    return json.loads(r.stdout)


def decodes_clean(path):
    r = run([FF, "-v", "error", "-i", path, "-f", "null", "-"])
    return r.returncode == 0 and r.stderr.strip() == "", r.stderr.strip()[:200]


def pdf_pages(path):
    r = run([PDFINFO, path])
    if r.returncode != 0:
        raise RuntimeError("not a readable PDF: " + r.stderr.strip()[:200])
    for line in r.stdout.splitlines():
        if line.startswith("Pages:"):
            return int(line.split()[1])
    raise RuntimeError("no page count")


def pdf_text(path, page=None):
    cmd = [PDFTOTEXT, "-layout"]
    if page:
        cmd += ["-f", str(page), "-l", str(page)]
    r = run(cmd + [path, "-"])
    return r.stdout


def verdict(ok, reason):
    print(("PASS: " if ok else "FAIL: ") + reason)
    sys.exit(0 if ok else 1)


def here(file):
    return os.path.dirname(os.path.abspath(file))


def taskdir(file):
    """Task directory to check. KT_TASKDIR points the checker at a clean arena copy
    (fixtures + work only) so agents never see the checkers."""
    return os.environ.get("KT_TASKDIR") or here(file)
