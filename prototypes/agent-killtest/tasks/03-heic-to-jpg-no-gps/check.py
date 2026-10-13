import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
from PIL import Image
import numpy as np, tempfile
root = os.path.dirname(os.path.dirname(here(__file__)))
ref = json.load(open(os.path.join(root, "_private", "t3.json")))
out = os.path.join(taskdir(__file__), "work", "out")
fx = os.path.join(taskdir(__file__), "fixtures", "trip_photos")
if not os.path.isdir(out):
    verdict(False, "work/out not found")
files = {os.path.splitext(f)[0]: f for f in os.listdir(out) if f.lower().endswith((".jpg", ".jpeg"))}
for base, size in ref.items():
    if base not in files:
        verdict(False, f"missing JPG for {base} (found {sorted(os.listdir(out))})")
    p = os.path.join(out, files[base])
    raw = open(p, "rb").read()
    if raw[:2] != b"\xff\xd8":
        verdict(False, f"{files[base]} is not a real JPEG")
    im = Image.open(p)
    if list(im.size) != size:
        verdict(False, f"{base} size {im.size}, expected {tuple(size)}")
    ex = im.getexif()
    if ex.get_ifd(0x8825) or 0x8825 in ex:
        verdict(False, f"{base} still has GPS EXIF")
    # scan only the metadata segments (before the image data starts), never the pixel stream
    i, hdr = 2, b""
    while i + 4 <= len(raw) and raw[i] == 0xFF and raw[i + 1] != 0xDA:
        ln = int.from_bytes(raw[i + 2:i + 4], "big")
        hdr += raw[i:i + 2 + ln]
        i += 2 + ln
    if b"GPS" in hdr or b"gps" in hdr or b"GPSLatitude" in hdr:
        verdict(False, f"{base} contains GPS metadata in its header segments")
    # content sanity against the HEIC
    with tempfile.TemporaryDirectory() as t:
        png = os.path.join(t, "r.png")
        run(["sips", "-s", "format", "png", os.path.join(fx, base + ".HEIC"), "--out", png])
        a = np.asarray(Image.open(png).convert("RGB"), np.float32).mean((0, 1))
    b = np.asarray(im.convert("RGB"), np.float32).mean((0, 1))
    if np.abs(a - b).max() > 8:
        verdict(False, f"{base} content differs from source (mean color {b} vs {a})")
verdict(True, f"{len(ref)} JPGs, correct sizes, no GPS")
