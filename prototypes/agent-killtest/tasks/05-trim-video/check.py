import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
import numpy as np
H = taskdir(__file__)
src = os.path.join(H, "fixtures", "lecture.mp4")
out = os.path.join(H, "work", "clip.mp4")
if not os.path.exists(out):
    verdict(False, "work/clip.mp4 not found")
try:
    info = probe(out)
except Exception as e:
    verdict(False, str(e))
v = [s for s in info["streams"] if s["codec_type"] == "video"]
a = [s for s in info["streams"] if s["codec_type"] == "audio"]
if not v:
    verdict(False, "no video stream")
if not a:
    verdict(False, "audio stream missing")
dur = float(info["format"]["duration"])
if abs(dur - 7.0) > 0.15:
    verdict(False, f"duration {dur:.2f}s, expected 7.00 +/- 0.15")
ok, err = decodes_clean(out)
if not ok:
    verdict(False, "decode errors: " + err)

def frames(path, limit=None):
    cmd = [FF, "-v", "error", "-i", path, "-vf", "fps=30,scale=4:4", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
    r = run(cmd, text=False)
    arr = np.frombuffer(r.stdout, np.uint8).astype(np.float32)
    return arr.reshape(-1, 48)

S = frames(src)
O = frames(out)
if len(O) < 150:
    verdict(False, f"only {len(O)} frames decoded")
def match(f, lo, hi):
    seg = S[int(lo * 30):int(hi * 30)]
    d = np.abs(seg - f).mean(1)
    return (np.argmin(d) + int(lo * 30)) / 30.0, d.min()
t0, e0 = match(O[0], 3, 8)
t1, e1 = match(O[-1], 9, 14)
if abs(t0 - 5.0) > 0.12 or e0 > 12:
    verdict(False, f"first frame matches source at {t0:.2f}s (err {e0:.1f}), expected 5.00s")
if abs(t1 - 12.0) > 0.2 or e1 > 12:
    verdict(False, f"last frame matches source at {t1:.2f}s (err {e1:.1f}), expected about 12.00s")
# no frozen or black start: first 10 frames must keep changing
if np.abs(np.diff(O[:10], axis=0)).mean() < 0.5:
    verdict(False, "first frames are frozen")
adur = float(a[0].get("duration", dur))
if abs(adur - 7.0) > 0.25:
    verdict(False, f"audio duration {adur:.2f}s")
verdict(True, f"duration {dur:.2f}s, starts at {t0:.2f}s, ends at {t1:.2f}s, clean decode")
