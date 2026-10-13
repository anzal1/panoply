import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
H = taskdir(__file__)
out = os.path.join(H, "work", "holiday_small.mp4")
if not os.path.exists(out):
    verdict(False, "work/holiday_small.mp4 not found")
size = os.path.getsize(out)
if size >= 8_000_000:
    verdict(False, f"{size} bytes, not under 8,000,000")
try:
    info = probe(out)
except Exception as e:
    verdict(False, str(e))
v = [s for s in info["streams"] if s["codec_type"] == "video"]
a = [s for s in info["streams"] if s["codec_type"] == "audio"]
if not v:
    verdict(False, "no video")
if (v[0]["width"], v[0]["height"]) != (1280, 720):
    verdict(False, f"resolution {v[0]['width']}x{v[0]['height']}, expected 1280x720")
if not a:
    verdict(False, "audio missing")
dur = float(info["format"]["duration"])
if abs(dur - 12.0) > 0.3:
    verdict(False, f"duration {dur:.2f}s, expected 12")
ok, err = decodes_clean(out)
if not ok:
    verdict(False, "decode errors: " + err)
kbps = size * 8 / dur / 1000
if kbps < 1000:
    verdict(False, f"overall bitrate only {kbps:.0f} kbps, quality gutted")
verdict(True, f"{size} bytes, 1280x720, {dur:.1f}s, {kbps:.0f} kbps")
