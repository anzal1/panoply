#!/usr/bin/env python3
"""Generate the 16:9 and 4:5 projects from one template (tools/mcp.template.html)."""
import shutil, sys
from pathlib import Path
here = Path(__file__).resolve().parent.parent          # docs/launch/drops/mcp
tpl = (here / "tools" / "mcp.template.html").read_text()
DUR = "23.2"
def project(root: Path, fmt: str, w: int, h: int):
    (root / "compositions").mkdir(parents=True, exist_ok=True)
    (root / "compositions" / "mcp.html").write_text(tpl.replace("__FORMAT__", fmt).replace("__W__", str(w)).replace("__H__", str(h)))
    idx = (here / "index.html").read_text()
    idx = idx.replace('data-duration="17.0"', f'data-duration="{DUR}"').replace('data-width="1920"', f'data-width="{w}"').replace('data-height="1080"', f'data-height="{h}"')
    idx = idx.replace("width=1920, height=1080", f"width={w}, height={h}").replace("width: 1920px; height: 1080px", f"width: {w}px; height: {h}px")
    (root / "index.html").write_text(idx)
    if root != here:
        for name in ("assets", "hyperframes.json", "package.json", "meta.json"):
            src, dst = here / name, root / name
            if src.is_dir():
                shutil.copytree(src, dst, dirs_exist_ok=True, ignore=shutil.ignore_patterns("vendor"))
            else:
                shutil.copy(src, dst)
project(here, "wide", 1920, 1080)
project(here.parent / "mcp-4x5", "tall", 1080, 1350)
