#!/usr/bin/env python3
"""Turns the raw self-check frames (artifact of the Launch captures workflow) into the clean RGBA plates the film uses.

    python3 docs/launch/tools/prep_captures.py CAPTURES_DIR [--out docs/launch/video/assets/captures]

CAPTURES_DIR holds frames-dark/, frames-light/ and optionally review/ (the artifact, unzipped). Raw frames are premultiplied
RGBA with transparent corners (see apps/desktop/tools/readme_assets.py, load()): this undoes the premultiply exactly the same way.
Run frames are thinned to at most RUN_MAX evenly spaced frames and renumbered 01..K. Writes captures.json with the counts."""
import argparse, json, os, re, sys
import numpy as np
from PIL import Image

RUN_MAX = 6
ap = argparse.ArgumentParser(); ap.add_argument('src'); ap.add_argument('--out', default=os.path.join(os.path.dirname(__file__), '..', 'video', 'assets', 'captures'))
a = ap.parse_args(); os.makedirs(a.out, exist_ok=True)

def unpremul(path):
    im = np.asarray(Image.open(path).convert('RGBA')).astype(np.float32)
    al = im[..., 3:4] / 255.0
    rgb = np.where(al > 0, im[..., :3] / np.maximum(al, 1e-6), 0.0)
    return Image.fromarray(np.dstack([np.clip(rgb + 0.5, 0, 255), im[..., 3]]).astype(np.uint8))

def save(img, name):
    img.save(os.path.join(a.out, name), optimize=True); return name

info = {'demo_frames': 13, 'run_frames': {}, 'plates': []}
for tag in ('dark', 'light'):
    d = os.path.join(a.src, f'frames-{tag}')
    if not os.path.isdir(d): print('missing', d); continue
    files = sorted(os.listdir(d))
    keep = [f for f in files if re.match(rf'(bar-(rest|empty|pdf|words|options|result|consent)|app-(home|pdf)|demo)-{tag}(-\d+)?\.png$', f) or re.match(rf'(bar|app)-.*-{tag}\.png$', f)]
    for f in keep: info['plates'].append(save(unpremul(os.path.join(d, f)), f))
    runs = sorted(f for f in files if re.match(rf'run-{tag}-\d+\.png$', f))
    if runs:
        idx = np.linspace(0, len(runs) - 1, min(RUN_MAX, len(runs))).round().astype(int)
        for k, i in enumerate(sorted(set(idx)), 1): save(unpremul(os.path.join(d, runs[i])), f'run-{tag}-{k:02d}.png')
        info['run_frames'][tag] = len(set(idx)); info.setdefault('run_frames_raw', {})[tag] = len(runs)
    demos = sorted(f for f in files if re.match(rf'demo-{tag}-\d+\.png$', f))
    info.setdefault('demo_frames_raw', {})[tag] = len(demos)
r = os.path.join(a.src, 'review')
if os.path.isdir(r):
    for f in sorted(os.listdir(r)):
        if re.match(r'(app-(modules|tool-result|find|results|settings|nearby|history)|all-panels)-(dark|light)\.png$', f):
            im = Image.open(os.path.join(r, f)).convert('RGBA'); save(im, f); info['plates'].append(f)
json.dump(info, open(os.path.join(a.out, 'captures.json'), 'w'), indent=1)
marker = os.path.join(a.out, 'PLACEHOLDER.txt')
if os.path.exists(marker) and info['plates']: os.remove(marker)
print(json.dumps({k: v for k, v in info.items() if k != 'plates'}), len(info['plates']), 'plates')
