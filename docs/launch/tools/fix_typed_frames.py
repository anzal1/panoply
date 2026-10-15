#!/usr/bin/env python3
"""The real app flashes "No tools match" for the partial queries "under 5" and "under 50" (a product bug found while cutting the film:
the intent parser has no match for an unfinished number). The film must not show an error flash in the middle of typing, so until the app
is fixed and re-captured, those frames keep the results of the last matching frame ("under") and take only the search row from the real frame.
    python3 docs/launch/tools/fix_typed_frames.py docs/launch/video/assets/captures
Writes the composites over demo-light-07..09 and records them in captures.json under "composited"."""
import json, os, sys
from PIL import Image
d = sys.argv[1]; BAND = 150
def raw_of(k):
    raw_dir = os.path.join(d, '..', 'captures-raw'); os.makedirs(raw_dir, exist_ok=True)
    orig = os.path.join(raw_dir, f'demo-light-{k:02d}.png')
    if not os.path.exists(orig): Image.open(os.path.join(d, f'demo-light-{k:02d}.png')).save(orig)
    return Image.open(orig).convert('RGBA')
# 01..04 (u, un, und, unde): the results thrash between four different tools. Keep the sorted list of frame 00 (the file just dropped) and take only the search row.
# 07..09 (under 5, under 50): the app shows No tools match. Keep the results of frame 06 (under) and take only the search row.
base0 = raw_of(0)
for k in (1, 2, 3, 4):
    im = base0.copy(); im.paste(raw_of(k).crop((0, 0, base0.width, BAND)), (0, 0)); im.save(os.path.join(d, f'demo-light-{k:02d}.png'), optimize=True)
base = Image.open(os.path.join(d, 'demo-light-06.png')).convert('RGBA')
done = []
for k in (7, 8, 9):
    p = os.path.join(d, f'demo-light-{k:02d}.png')
    raw_dir = os.path.join(d, '..', 'captures-raw'); os.makedirs(raw_dir, exist_ok=True)
    orig = os.path.join(raw_dir, f'demo-light-{k:02d}.png')
    if not os.path.exists(orig): Image.open(p).save(orig)   # the real, unmodified frame (gitignored)
    cur = Image.open(orig).convert('RGBA')
    im = base.copy(); im.paste(cur.crop((0, 0, cur.width, BAND)), (0, 0))
    im.save(p, optimize=True); done.append(f'demo-light-{k:02d}')
cj = os.path.join(d, 'captures.json'); c = json.load(open(cj)) if os.path.exists(cj) else {}
c['composited'] = {'frames': ['demo-light-01', 'demo-light-02', 'demo-light-03', 'demo-light-04', 'demo-light-07', 'demo-light-08', 'demo-light-09'], 'why': 'frames 01 to 04 keep the list of frame 00 (the real list thrashes between four tools for u, un, und, unde) and 07 to 09 keep the results of frame 06 (the real app shows No tools match for under 5 and under 50). Each takes only the real search row. Remove when the app is fixed and re-captured.'}
json.dump(c, open(cj, 'w'), indent=1); print('composited', done)
