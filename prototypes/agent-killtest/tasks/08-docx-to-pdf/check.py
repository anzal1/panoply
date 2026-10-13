import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
out = os.path.join(taskdir(__file__), "work", "report.pdf")
if not os.path.exists(out):
    verdict(False, "work/report.pdf not found")
try:
    n = pdf_pages(out)
except Exception as e:
    verdict(False, str(e))
if not (1 <= n <= 5):
    verdict(False, f"{n} pages, expected 1 to 5")
t = pdf_text(out)
need = ["Quarterly Operations Report", "Summary", "Key numbers", "Metric", "Q3 revenue", "42,500", "38,900",
        "Headcount", "Priorities", "Launch the Pune warehouse", "Close the vendor audit",
        "Hire two support leads", "Details", "ZX-REP-9047"]
pos = -1
for k in need:
    i = t.find(k)
    if i < 0:
        verdict(False, f"missing text {k!r}")
    if k in ("Quarterly Operations Report", "Priorities", "Details", "ZX-REP-9047"):
        if i < pos:
            verdict(False, f"{k!r} out of order")
        pos = i
# table layout: the Q3 revenue row must keep its numbers on the same line
line = [l for l in t.splitlines() if "Q3 revenue" in l]
if not line or "38,900" not in line[0] or "42,500" not in line[0]:
    verdict(False, "table row not preserved on one line: " + repr(line[:1]))
f = run([PDFFONTS, out]).stdout
fonts = [l.split()[0] for l in f.splitlines()[2:] if l.strip()]
if not any("bold" in x.lower() for x in fonts):
    verdict(False, "no bold font in PDF, formatting lost (fonts: %s)" % fonts)
if len(set(fonts)) < 2:
    verdict(False, "only one font, formatting lost")
verdict(True, f"{n} pages, all text, table row intact, bold present")
