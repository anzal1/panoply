import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
from PIL import Image
import numpy as np
p = os.path.join(taskdir(__file__), "work", "exam_photo.jpg")
if not os.path.exists(p):
    verdict(False, "work/exam_photo.jpg not found")
raw = open(p, "rb").read()
if raw[:2] != b"\xff\xd8":
    verdict(False, "not a JPEG")
im = Image.open(p)
if im.format != "JPEG":
    verdict(False, f"format {im.format}")
if im.size != (200, 230):
    verdict(False, f"dimensions {im.size}, expected (200, 230)")
n = len(raw)
if not (20480 <= n <= 50000):
    verdict(False, f"{n} bytes, must be 20480 to 50000")
a = np.asarray(im.convert("RGB"), np.float32)
if a.std() < 15:
    verdict(False, "image looks blank")
verdict(True, f"200x230 JPEG, {n} bytes")
