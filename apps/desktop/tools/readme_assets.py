#!/usr/bin/env python3
"""Builds the README images in docs/assets from the app's own offscreen renders.

    panoply-desktop --selfcheck-frames --out FRAMES --appearance light   (needs --features snapshot)
    panoply-desktop --selfcheck-frames --out FRAMES --appearance dark
    apps/desktop/tools/readme_assets.py FRAMES [--geist PATH_TO_Geist.woff2] [--chrome PATH]

What it writes, with the same names, layouts and sizes as before:

  docs/assets/screens/*.png               2000 x 1280 for the bar, 2800 x 1760 for the app window
  docs/assets/panoply-demo-still-*.png    the bar with a PDF dropped on it
  docs/assets/panoply-demo-*.gif          1400 x 905, 8 frames, under 0.5 MB
  docs/assets/social-preview.png          2560 x 1280, the headline set in Geist, rendered by Chrome

The frames are raw RGBA with transparent corners and no backdrop; each one is set on a flat desktop
colour with a soft shadow (a blurred copy of its own outline, 30 points down). Needs Pillow and numpy;
the social preview needs Chrome and the Geist variable font (woff2).
"""
import argparse
import os
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageFilter

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../.."))
ASSETS = os.path.join(ROOT, "docs/assets")
BACKDROP = {"light": (226, 229, 234), "dark": (33, 36, 42)}
SHADOW = {"light": 0.38, "dark": 0.55}  # peak darkness at the edge of the window


def stage(frame, tag, canvas, scale=1.0, offset=None, bg=None):
    """The window on a flat desktop colour, with its shadow. Returns an RGB image."""
    bg = bg or BACKDROP[tag]
    if scale != 1.0:
        frame = frame.resize((round(frame.width * scale), round(frame.height * scale)), Image.LANCZOS)
    w, h = canvas
    ox, oy = offset if offset else ((w - frame.width) // 2, (h - frame.height) // 2)
    alpha = Image.new("L", canvas, 0)
    alpha.paste(frame.getchannel("A"), (ox, oy + round(30 * scale)))
    alpha = alpha.filter(ImageFilter.GaussianBlur(40 * scale))
    m = np.asarray(alpha, dtype=np.float32) / 255.0 * SHADOW[tag]
    base = np.empty((h, w, 3), dtype=np.float32)
    base[:] = bg
    shadowed = base * (1.0 - m[..., None]) + np.array([10, 14, 24], dtype=np.float32) * m[..., None]
    out = Image.fromarray(np.clip(shadowed + 0.5, 0, 255).astype(np.uint8)).convert("RGBA")
    out.alpha_composite(frame, (ox, oy))
    return out.convert("RGB")


def load(frames, name):
    """A raw frame. render_to_image hands back premultiplied colour, so undo that first."""
    a = np.asarray(Image.open(os.path.join(frames, name + ".png")).convert("RGBA")).astype(np.float32)
    alpha = a[..., 3:4] / 255.0
    rgb = np.where(alpha > 0, a[..., :3] / np.maximum(alpha, 1e-6), 0.0)
    return Image.fromarray(np.dstack([np.clip(rgb + 0.5, 0, 255), a[..., 3]]).astype(np.uint8))


def save_png(img, path):
    img.save(path, optimize=True)
    print(path, img.size, os.path.getsize(path) // 1024, "KB")


def screens(frames):
    names = {
        "bar-rest": "bar-rest", "bar-pdf": "bar-pdf", "bar-words": "bar-words",
        "bar-result": "bar-result", "bar-consent": "bar-consent",
    }
    for tag in ("light", "dark"):
        for out_name, frame_name in names.items():
            img = stage(load(frames, f"{frame_name}-{tag}"), tag, (2000, 1280))
            save_png(img, os.path.join(ASSETS, f"screens/{out_name}-{tag}.png"))
        # The batch progress: the frame of the long job nearest 55% of the way through.
        runs = sorted(f for f in os.listdir(frames) if f.startswith(f"vrun-{tag}-"))
        pick = runs[int(len(runs) * 0.55)][:-4]
        save_png(stage(load(frames, pick), tag, (2000, 1280)), os.path.join(ASSETS, f"screens/bar-running-{tag}.png"))
        save_png(stage(load(frames, f"app-home-{tag}"), tag, (2800, 1760)), os.path.join(ASSETS, f"screens/app-home-{tag}.png"))
        still = stage(load(frames, f"bar-pdf-{tag}"), tag, (2000, 1280))
        save_png(still, os.path.join(ASSETS, f"panoply-demo-still-{tag}.png"))


def gifs(frames):
    # Typing, one frame per step; then the run and the result card.
    plan = [("demo-{t}-01", 130), ("demo-{t}-03", 130), ("demo-{t}-04", 130), ("demo-{t}-05", 130),
            ("demo-{t}-10", 1300), ("demo-{t}-11", 1300), ("run-{t}-13", 500), ("bar-result-{t}", 2800)]
    for tag in ("light", "dark"):
        imgs, durations = [], []
        for name, ms in plan:
            f = stage(load(frames, name.format(t=tag)), tag, (1400, 905), scale=0.851, offset=(87, 61))
            imgs.append(f.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
            durations.append(ms)
        path = os.path.join(ASSETS, f"panoply-demo-{tag}.gif")
        imgs[0].save(path, save_all=True, append_images=imgs[1:], duration=durations, loop=0, optimize=True, disposal=1)
        print(path, os.path.getsize(path) // 1024, "KB")
        if os.path.getsize(path) > 500_000:
            sys.exit(f"{path} is over 0.5 MB")


def social(frames, geist, chrome):
    tag = "light"
    work = tempfile.mkdtemp(prefix="panoply-social-")
    bar = stage(load(frames, f"bar-pdf-{tag}"), tag, (1440 + 320, 920 + 320), bg=(244, 245, 247))
    bar_path = os.path.join(work, "bar.png")
    bar.save(bar_path)
    html = f"""<!doctype html><meta charset=utf-8><style>
@font-face{{font-family:G;src:url(file://{geist});font-weight:100 900}}
html,body{{margin:0;background:#F4F5F7;width:2560px;height:1280px;overflow:hidden;font-family:G}}
img.logo{{position:absolute;left:160px;top:144px;height:88px}}
img.bar{{position:absolute;left:1023px;top:108px;width:{round(1760 * 0.86)}px}}
h1{{position:absolute;left:160px;top:400px;margin:0;font-weight:600;font-size:93.2px;line-height:101px;letter-spacing:-0.03em;color:#1F222A;width:900px}}
p.sub{{position:absolute;left:160px;top:777px;margin:0;font-size:39.6px;line-height:57px;color:#5B606C;width:900px}}
p.plat{{position:absolute;left:160px;top:1108px;margin:0;font-size:34.2px;color:#7A808C}}
p.plat span{{margin-right:37.5px}}
</style>
<img class=logo src="file://{ASSETS}/panoply-logo-light.svg">
<img class=bar src="file://{bar_path}">
<h1>Every file utility,<br>in one app on<br>your own device.</h1>
<p class=sub>PDF, images, video, audio and text from images.<br>Open source. Nothing leaves your computer.</p>
<p class=plat><span>macOS</span><span>Windows</span><span>Linux</span></p>"""
    page = os.path.join(work, "social.html")
    open(page, "w").write(html)
    out = os.path.join(ASSETS, "social-preview.png")
    subprocess.run([chrome, "--headless=new", "--hide-scrollbars", "--force-device-scale-factor=1",
                    "--window-size=2560,1280", f"--screenshot={out}", "file://" + page],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    img = Image.open(out).convert("RGB")
    img.save(out, optimize=True)
    print(out, img.size, os.path.getsize(out) // 1024, "KB")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("frames")
    ap.add_argument("--geist", default=os.environ.get("GEIST_FONT", ""))
    ap.add_argument("--chrome", default="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
    a = ap.parse_args()
    screens(a.frames)
    gifs(a.frames)
    if a.geist:
        social(a.frames, os.path.abspath(a.geist), a.chrome)
    else:
        print("no --geist given: social-preview.png left as it was")
