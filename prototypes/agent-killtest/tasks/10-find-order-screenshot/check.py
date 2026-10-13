import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
root = os.path.dirname(os.path.dirname(here(__file__)))
exp = json.load(open(os.path.join(root, "_private", "t10.json")))["answer"]
p = os.path.join(taskdir(__file__), "work", "answer.txt")
if not os.path.exists(p):
    verdict(False, "work/answer.txt not found")
got = open(p).read().strip()
if got == exp or os.path.splitext(got)[0] == os.path.splitext(exp)[0] and os.path.basename(got) == got:
    verdict(True, f"answered {got!r}")
verdict(False, f"answered {got[:80]!r}, expected {exp!r}")
