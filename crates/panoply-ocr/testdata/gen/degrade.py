"""Makes the scan-like and low-resolution variants of the clean generated images.

scan: a slight rotation, blur, grain, lower contrast and JPEG compression, like a poor
flatbed scan. low: half the resolution, like a screenshot or a phone photo of a screen.
Fixed seeds, so the set is reproducible.
"""
import io
import shutil
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

out = Path(sys.argv[1])
rng = np.random.default_rng(7)
for n, png in enumerate(sorted(out.glob("*.png"))):
    if png.stem.endswith(("-scan", "-low")):
        continue
    im = Image.open(png).convert("L")
    # scan
    angle = 1.3 if n % 2 == 0 else -1.1
    s = im.rotate(angle, resample=Image.BICUBIC, expand=True, fillcolor=255)
    s = s.filter(ImageFilter.GaussianBlur(0.9))
    a = np.asarray(s, dtype=np.float32)
    a = 255 - (255 - a) * 0.82  # lower contrast
    a += rng.normal(0, 9, a.shape)
    a = np.clip(a, 0, 255).astype(np.uint8)
    buf = io.BytesIO()
    Image.fromarray(a).save(buf, "JPEG", quality=55)
    Image.open(io.BytesIO(buf.getvalue())).save(out / f"{png.stem}-scan.png", optimize=True)
    shutil.copy(out / f"{png.stem}.txt", out / f"{png.stem}-scan.txt")
    # low resolution
    w, h = im.size
    lo = im.resize((w // 2, h // 2), Image.LANCZOS)
    buf = io.BytesIO()
    lo.save(buf, "JPEG", quality=70)
    Image.open(io.BytesIO(buf.getvalue())).save(out / f"{png.stem}-low.png", optimize=True)
    shutil.copy(out / f"{png.stem}.txt", out / f"{png.stem}-low.txt")
