#!/usr/bin/env python3
"""Writes both teaser cuts from teaser.template.html (16:9 and 4:5). Run from anywhere."""
import pathlib
here = pathlib.Path(__file__).resolve().parent
drops = here.parent.parent
tpl = (here / "teaser.template.html").read_text()
host = (drops / "mcp" / "index.html").read_text()
for pid, w, h, wide in (("teaser", 1920, 1080, "true"), ("teaser-4x5", 1080, 1350, "false")):
    out = tpl.replace("__ID__", pid).replace("__W__", str(w)).replace("__H__", str(h)).replace("__WIDE__", wide)
    (drops / pid / "compositions" / f"{pid}.html").write_text(out)
    idx = f'''<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width={w}, height={h}" />
    <script src="assets/vendor/gsap.min.js"></script>
    <style>
      * {{ margin: 0; padding: 0; box-sizing: border-box; }}
      @font-face {{ font-family: "Inter"; src: url("assets/fonts/Inter-Variable-latin.woff2") format("woff2"); font-weight: 100 900; }}
      html, body {{ margin: 0; width: {w}px; height: {h}px; overflow: hidden; background: #0D0E12; }}
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="{pid}" data-start="0" data-duration="22" data-width="{w}" data-height="{h}">
    <div id="host-{pid}" class="clip" data-composition-id="{pid}" data-composition-src="compositions/{pid}.html"
         data-start="0.0" data-duration="22" data-track-index="1" data-width="{w}" data-height="{h}"></div>
    </div>
    <script>
      const tl = gsap.timeline({{ paused: true }});
      window.__timelines["{pid}"] = tl;
      tl.seek(0);
    </script>
  </body>
</html>
'''
    (drops / pid / "index.html").write_text(idx)
print("ok")
