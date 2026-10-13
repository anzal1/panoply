import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
out = os.path.join(taskdir(__file__), "work", "merged.pdf")
if not os.path.exists(out):
    verdict(False, "work/merged.pdf not found")
try:
    n = pdf_pages(out)
except Exception as e:
    verdict(False, str(e))
if n != 9:
    verdict(False, f"page count {n}, expected 9")
expect = [("CON", 4), ("IDP", 3), ("INV", 2)]
page = 1
for tag, cnt in expect:
    for i in range(1, cnt + 1):
        t = pdf_text(out, page)
        if f"{tag}-P{i}-5520" not in t:
            verdict(False, f"page {page} should be {tag} page {i}, found: {t.strip()[:60]!r}")
        page += 1
verdict(True, "9 pages in order contract, id_proof, invoice")
