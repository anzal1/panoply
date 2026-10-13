import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
from PIL import Image
import numpy as np
p = os.path.join(taskdir(__file__), "work", "passport.jpg")
if not os.path.exists(p):
    verdict(False, "work/passport.jpg not found")
raw = open(p, "rb").read()
if raw[:2] != b"\xff\xd8":
    verdict(False, "not a JPEG")
im = Image.open(p)
if im.size != (600, 600):
    verdict(False, f"size {im.size}, expected 600x600")
a = np.asarray(im.convert("RGB"), np.float32)
r, g, b = a[..., 0], a[..., 1], a[..., 2]
H, W = 600, 600
top = a[:30].reshape(-1, 3).mean(0)
left = a[:400, :30].reshape(-1, 3).mean(0)
right = a[:400, -30:].reshape(-1, 3).mean(0)
for name, m in [("top", top), ("left", left), ("right", right)]:
    if m.min() < 235:
        verdict(False, f"{name} edge is not white (mean {m.round()})")
blue = ((b > r + 35) & (b > g + 15)).mean()
if blue > 0.015:
    verdict(False, f"{blue * 100:.1f}% of pixels still blue background")
skin = (np.abs(r - 224) < 30) & (np.abs(g - 172) < 30) & (np.abs(b - 140) < 30)
if skin.mean() < 0.05:
    verdict(False, f"subject missing, skin area only {skin.mean() * 100:.1f}%")
ys, xs = np.nonzero(skin)
if ys.min() < 15 or xs.min() < 15 or xs.max() > W - 15:
    verdict(False, "head touches or is cut by the frame edge")
cx = xs.mean() / W
if not (0.4 <= cx <= 0.6):
    verdict(False, f"head off center ({cx:.2f})")
red = (r > 110) & (g < 80) & (b < 90) & (r > g + 60)
if red.mean() < 0.015:
    verdict(False, "shirt lost or recolored")
verdict(True, "600x600 JPEG, white background, subject intact and centered")
