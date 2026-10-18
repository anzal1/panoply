#!/usr/bin/env python3
"""Build the bar plates for the bar-answers drop from real renders of the real app.

Input: the three flattened light renders of the command bar in ../source (bar-calc, bar-calc-convert,
bar-calc-skip; 1440 x 920, written by `panoply-desktop --selfcheck-frames`). Nothing here draws UI: every glyph
on the output plates is a pixel of those renders. What this script does:

  1. gives the renders their transparent rounded corners back. The renders are flattened onto a desktop colour;
     the window's own alpha (source/window-alpha.png, the alpha channel of the launch film's bar-rest-light capture, same 1440 x 920 window) says which pixels are corner,
     and the flatten is undone on the edge pixels.
  2. for "72 f to c": the render predates commit 55db76d ("at most four decimals"), so it reads 22.22222222.
     The shipped app reads 22.2222. The row is rebuilt from the same glyphs: the first seven characters of the
     bold title are kept, the four extra digits are cut out, and the unit and the subtitle slide left by exactly
     those four digit advances. (Drop this step when the render is redone at 55db76d or later.)
  3. writes, per scene, a head layer (search field with the typed words), a body layer (answer row or tool row
     and footer) and one shared base (the same window with the typed words, the rows and the footer hints
     removed, filled with the window's flat background). The film types by revealing the head layer left to right
     and lands the body layer on the last keystroke.
  4. measures the glyph boundaries in the head layer, so each keystroke reveals exactly one character.

Usage: make-plates.py [source_dir] [out_dir]
"""
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

HERE = Path(__file__).resolve().parent
SRC = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE.parent / "source"
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else HERE.parent / "assets" / "plates"

W, H = 1440, 920
BG = np.array([241, 242, 245], dtype=float)  # the window's flat background (light)
ROW = np.array([199, 210, 223], dtype=float)  # the highlighted row
DESK = np.array([201, 208, 219], dtype=float)  # the desktop colour the renders were flattened onto
HEAD_H = 112  # search field, down to and including its divider (y 111)
FOOT_Y = 840  # footer divider


def corner_alpha() -> np.ndarray:
    rest = np.array(Image.open(SRC / "window-alpha.png").convert("L")).astype(float)
    return np.clip(rest / 229.0, 0, 1)


def real(name: str, m: np.ndarray) -> np.ndarray:
    """The render with its corners transparent again, float RGBA."""
    f = np.array(Image.open(SRC / f"{name}.png").convert("RGB")).astype(float)
    a = np.zeros((H, W, 4))
    ok = m > 0.02
    mm = np.where(ok, m, 1.0)[..., None]
    rgb = (f - (1 - mm) * DESK) / mm
    a[..., :3] = np.clip(rgb, 0, 255)
    a[..., 3] = m * 255
    return a


def ink_runs(a: np.ndarray, y0, y1, x0, x1, bg, th=30):
    reg = a[y0:y1, x0:x1, :3]
    cols = (np.abs(reg - bg).sum(axis=2) > th).any(axis=0)
    runs, s = [], None
    for i, v in enumerate(cols):
        if v and s is None:
            s = i
        if not v and s is not None:
            runs.append((x0 + s, x0 + i - 1))
            s = None
    if s is not None:
        runs.append((x0 + s, x1 - 1))
    return runs


def fix_convert(a: np.ndarray) -> np.ndarray:
    """22.22222222 to 22.2222: keep the first seven glyphs, cut four digits, slide the unit and the subtitle left."""
    runs = ink_runs(a, 205, 255, 110, 500, ROW)
    # glyph runs of the bold title, in order: 2 2 . 2 2 2 2 2 2 2 2 then the degree sign, C, then the subtitle
    digit_ends = [r[1] for r in runs[:11]]
    assert len(runs) >= 13 and (digit_ends[10] - digit_ends[6]) > 60, runs
    cut = digit_ends[10] - digit_ends[6]  # four digit advances
    keep_to = digit_ends[6] + 2  # first column after the seventh glyph's gap: the eighth digit starts 3 px later
    unit_from = keep_to + cut  # the unit moves left by exactly four digit advances, so the gap before it is unchanged
    assert digit_ends[10] < unit_from < runs[11][0]
    out = a.copy()
    y0, y1 = 190, 267
    seg = a[y0:y1, unit_from:500].copy()
    out[y0:y1, keep_to:500, :3] = ROW
    out[y0:y1, keep_to : keep_to + seg.shape[1]] = seg
    # the pixels between the seventh glyph and the unit are the row colour in the source too, so the join is seamless
    return out


def erase(a: np.ndarray, y0, y1, x0, x1, color) -> None:
    a[y0:y1, x0:x1, :3] = color


def layer(a: np.ndarray, y0, y1, solid: np.ndarray) -> Image.Image:
    """Only rows y0..y1 of the plate, with no corner pixels (the base carries those)."""
    o = np.zeros_like(a)
    o[y0:y1] = a[y0:y1]
    o[..., 3] = np.where(solid, o[..., 3], 0)
    return Image.fromarray(np.clip(o, 0, 255).astype(np.uint8))


def main() -> None:
    m = corner_alpha()
    solid = m >= 0.999
    scenes = {
        "A": ("bar-calc-light", "15% of 2400"),
        "B": ("bar-calc-convert-light", "72 f to c"),
        "C": ("bar-calc-skip-light", "merge pdf"),
    }
    info = {}
    base = None
    for key, (name, text) in scenes.items():
        a = real(name, m)
        if key == "B":
            a = fix_convert(a)
        runs = ink_runs(a, 20, 100, 90, 760, BG)
        # per typed character, the column up to which the head layer is shown after that keystroke
        chars = list(text)
        nonspace = [c for c in chars if c != " "]
        if len(runs) < len(nonspace):
            # touching glyphs share a run: split a run at its emptiest column
            fixed = []
            need = len(nonspace) - len(runs)
            ink = (np.abs(a[20:100, :, :3] - BG).sum(axis=2) > 30).sum(axis=0)
            for r in runs:
                width = r[1] - r[0] + 1
                if need > 0 and width > 26 and key == "C":
                    lo, hi = r[0] + 6, r[0] + 20
                    cut = lo + int(np.argmin(ink[lo:hi]))
                    fixed += [(r[0], cut - 1), (cut, r[1])]
                    need -= 1
                else:
                    fixed.append(r)
            runs = fixed
        assert len(runs) == len(nonspace), (key, runs, nonspace)
        bounds, k = [], 0
        for c in chars:
            if c == " ":
                bounds.append(bounds[-1] if bounds else 0)
            else:
                bounds.append(runs[k][1] + 2)  # include the glyph's antialiased edge
                k += 1
        bounds[-1] = W
        info[key] = {"text": text, "bounds": bounds, "runs": runs}
        # head and body layers from the finished plate
        head = layer(a, 0, HEAD_H, solid)
        body = layer(a, HEAD_H, H, solid)
        head.save(OUT / f"{key}-head.png", optimize=True)
        body.save(OUT / f"{key}-body.png", optimize=True)
        if base is None:
            b = a.copy()
            erase(b, 14, 106, 92, 760, BG)  # the typed words
            erase(b, HEAD_H, FOOT_Y, 2, W - 2, BG)  # the rows
            erase(b, FOOT_Y + 2, H - 8, 205, W - 2, BG)  # the footer hints (the Panoply mark and name stay)
            base = Image.fromarray(np.clip(b, 0, 255).astype(np.uint8))
            base.save(OUT / "base.png", optimize=True)
    (OUT / "plates.json").write_text(json.dumps(info, indent=1))
    print(json.dumps({k: v["bounds"] for k, v in info.items()}))


if __name__ == "__main__":
    main()
