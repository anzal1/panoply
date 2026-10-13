import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
W = os.path.join(taskdir(__file__), "work")
out = os.path.join(W, "compressed.pdf")
if not os.path.exists(out):
    verdict(False, "work/compressed.pdf not found")
size = os.path.getsize(out)
if size >= 1_000_000:
    verdict(False, f"{size} bytes is not under 1,000,000")
try:
    n = pdf_pages(out)
except Exception as e:
    verdict(False, str(e))
if n != 4:
    verdict(False, f"page count {n}, expected 4")
for i in range(1, 5):
    if f"ALPHA-{i}-7731" not in pdf_text(out, i):
        verdict(False, f"text not selectable or missing on page {i}")
r = run([PDFIMAGES, "-list", out])
imgs = max(0, len(r.stdout.strip().splitlines()) - 2)
if imgs < 4:
    verdict(False, f"images were dropped (only {imgs} left, expected 4)")
verdict(True, f"{size} bytes, 4 pages, text selectable, {imgs} images kept")
