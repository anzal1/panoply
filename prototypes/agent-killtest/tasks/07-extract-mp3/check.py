import os, sys, tempfile, wave
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
from checklib import *
import numpy as np
out = os.path.join(taskdir(__file__), "work", "talk.mp3")
if not os.path.exists(out):
    verdict(False, "work/talk.mp3 not found")
try:
    info = probe(out)
except Exception as e:
    verdict(False, str(e))
s = info["streams"]
if any(x["codec_type"] == "video" and x.get("disposition", {}).get("attached_pic", 0) == 0 for x in s):
    verdict(False, "has a video stream")
au = [x for x in s if x["codec_type"] == "audio"]
if not au or au[0]["codec_name"] != "mp3":
    verdict(False, "audio codec is not mp3: " + str([x["codec_name"] for x in s]))
br = int(au[0].get("bit_rate") or info["format"].get("bit_rate", 0))
if not (120000 <= br <= 136000):
    verdict(False, f"bitrate {br}, expected about 128000")
dur = float(info["format"]["duration"])
if abs(dur - 15.0) > 0.25:
    verdict(False, f"duration {dur:.2f}, expected 15")
with tempfile.TemporaryDirectory() as t:
    wav = os.path.join(t, "a.wav")
    run([FF, "-v", "error", "-y", "-i", out, "-ac", "1", "-ar", "22050", wav])
    w = wave.open(wav)
    x = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32)
    sr = w.getframerate()
def peak(t0):
    seg = x[int(t0 * sr):int((t0 + 1) * sr)]
    f = np.abs(np.fft.rfft(seg * np.hanning(len(seg))))
    return np.argmax(f) * sr / len(seg)
for t0, hz in [(2, 440), (7, 550), (12, 660)]:
    p = peak(t0)
    if abs(p - hz) > 8:
        verdict(False, f"at {t0}s dominant tone {p:.0f} Hz, expected {hz}")
verdict(True, f"mp3, {br} bps, {dur:.2f}s, tones intact")
