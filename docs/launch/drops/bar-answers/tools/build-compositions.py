#!/usr/bin/env python3
"""Write the two compositions of the bar-answers drop from one template: 16:9 (1920x1080) and 4:5 (1080x1350).

The scenes, the plates and the timeline are the same; the layout is recomposed per format (type sizes, keycap stack,
camera crop, proof stack). Run from anywhere: python3 tools/build-compositions.py
"""
from pathlib import Path
from string import Template

ROOT = Path(__file__).resolve().parent.parent
FORMATS = {
    "bar-answers": dict(  # 16:9
        W=1920, H=1080, DUR="20.1",
        pain_size=150, pain_gap=6,
        keys_css="""
    .keygroup { position: absolute; top: 400px; height: 156px; }
    #kg-mac { left: 173px; width: 567px; }
    #kg-win { left: 880px; width: 867px; }
    .keyrow { display: flex; gap: 36px; }
    .key { height: 156px; border-radius: 39px; font-size: 78px; }
    .cap-label { position: absolute; left: 0; right: 0; top: 190px; line-height: 60px; font-size: 54px; }""",
        cap_css=".cap-line { top: 40px; font-size: 68px; } .cap-line .br { display: inline; } .cap-line .v { font-size: 68px; }",
        push="left: 110px; top: 190px; width: 1100px; height: 703px;", zoom=1.8, drift="1.025",
        plate_css="",
        proof_css="""
    .pcol { top: 290px; width: 600px; }
    #pnote { top: 760px; }""",
        proof_pos=("left:100px", "left:660px", "left:1220px"),
        end_css="""
    #fan { left: 790px; top: 70px; width: 340px; height: 340px; }
    #ename { top: 420px; font-size: 170px; } #etag { top: 660px; font-size: 60px; } #eurl { top: 800px; font-size: 56px; }""",
        etag="Free. Open source. Runs on your computer.",
    ),
    "bar-answers-4x5": dict(
        W=1080, H=1350, DUR="20.1",
        pain_size=98, pain_gap=10,
        keys_css="""
    .keygroup { position: absolute; left: 0; right: 0; height: 156px; display: flex; justify-content: center; }
    #kg-mac { top: 300px; }
    #kg-win { top: 680px; }
    .keyrow { display: flex; gap: 36px; }
    .key { height: 156px; border-radius: 39px; font-size: 78px; }
    .cap-label { position: absolute; left: 0; right: 0; top: 190px; line-height: 60px; font-size: 54px; }""",
        cap_css=".cap-line { top: 296px; font-size: 80px; line-height: 1.08; } .cap-line .br { display: block; } .cap-line .v { font-size: 80px; }",
        push="left: 44px; top: 526px; width: 1100px; height: 703px;", zoom=1.6, drift="1.012",
        plate_css="#plate { inset: auto; left: 0; top: 0; width: 473.6px; height: 252.2px; } /* a card of the render: top third, left 620 px of 1440 */",
        proof_css="""
    .pcol { left: 0 !important; width: 1080px; }
    #pc1 { top: 80px; } #pc2 { top: 400px; } #pc3 { top: 720px; }
    .pnum { font-size: 170px; } .pnum small { font-size: 94px; }
    .plab { font-size: 40px; margin-top: 18px; }
    #pnote { top: 1150px; font-size: 62px; }""",
        proof_pos=("", "", ""),
        end_css="""
    #fan { left: 370px; top: 200px; width: 340px; height: 340px; }
    #ename { top: 560px; font-size: 150px; } #etag { top: 790px; font-size: 62px; line-height: 1.15; white-space: normal; padding: 0 60px; }
    #eurl { top: 1000px; font-size: 50px; }""",
        etag="Free. Open source.<br>Runs on your computer.",
    ),
}

TEMPLATE = Template(r'''<!doctype html>
<html><head><meta charset="UTF-8"></head><body>
<template>
  <style>
    @font-face { font-family: "Geist"; src: url("assets/fonts/Geist-Variable-latin.woff2") format("woff2"); font-weight: 100 900; }
    @font-face { font-family: "Geist Mono"; src: url("assets/fonts/GeistMono-latin.woff2") format("woff2"); font-weight: 100 900; }

    #root { position: absolute; inset: 0; overflow: hidden; font-family: "Geist", system-ui, sans-serif; -webkit-font-smoothing: antialiased; color: #fff; }
    #ground { position: absolute; inset: 0; background: #0c0c0f; }
    #blue { position: absolute; inset: 0; background: #2d6cdf; opacity: 0; }
    #lift { position: absolute; inset: 0; background: radial-gradient(ellipse 70% 70% at 50% 46%, rgba(255,255,255,.09), rgba(255,255,255,0) 100%); opacity: 0; }
    #grain { position: absolute; inset: 0; width: 100%; height: 100%; opacity: .4; pointer-events: none; }

    /* 1. the pain, in Midnight. The first line is on frame 0. */
    #pain { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: ${pain_gap}px; }
    .pl { font-size: ${pain_size}px; font-weight: 700; letter-spacing: -0.04em; line-height: 1; white-space: nowrap; color: #ececf1; opacity: 0; }

    /* 2. both keycap sets at once, at 1.5x the launch film's */
    .keygroup { opacity: 0; }
    .key { display: flex; align-items: center; justify-content: center; font-weight: 600; letter-spacing: -0.02em; color: #fff;
      background: rgba(255,255,255,.16); border: 3px solid rgba(255,255,255,.45);
      box-shadow: inset 0 1px 0 rgba(255,255,255,.35), 0 27px 60px rgba(8,24,70,.28); }
    .cap-label { text-align: center; font-family: "Geist Mono", ui-monospace, monospace; font-weight: 500; color: rgba(255,255,255,.92); white-space: nowrap; }
    $keys_css

    /* captions above the bar */
    .cap-line { position: absolute; left: 0; right: 0; text-align: center; font-weight: 700; letter-spacing: -0.035em; line-height: 1.05; color: #fff; opacity: 0; }
    .cap-line .v { font-weight: 500; opacity: .82; }
    $cap_css

    /* the bar: real renders as plates, 1440 x 920 source. The camera shows its top third (query and answer row) at 1.8x. */
    #push { position: absolute; $push transform-origin: 0 0; }
    #open { position: absolute; inset: 0; visibility: hidden; }
    #plate { position: absolute; inset: 0; overflow: hidden; border-radius: 28.8px; background: #f1f2f5;
      box-shadow: 0 0 0 1.5px rgba(255,255,255,.35), 0 40px 100px rgba(8,24,70,.5); }
    #plate img { position: absolute; left: 0; top: 0; width: 1100px; height: 703px; display: block; opacity: 0; }
    $plate_css

    /* 3. proof */
    #proof { position: absolute; inset: 0; opacity: 1; }
    .pcol { position: absolute; text-align: center; opacity: 0; }
    .pnum { font-size: 200px; font-weight: 700; letter-spacing: -0.045em; line-height: 1; white-space: nowrap; }
    .pnum small { font-size: 108px; letter-spacing: -0.03em; font-weight: 600; margin-left: 10px; }
    .plab { margin-top: 30px; font-family: "Geist Mono", ui-monospace, monospace; font-size: 38px; font-weight: 500; line-height: 1.35; color: rgba(255,255,255,.9); }
    #pnote { position: absolute; left: 0; right: 0; text-align: center; font-size: 56px; font-weight: 600; letter-spacing: -0.02em; color: rgba(255,255,255,.92); opacity: 0; }
    $proof_css

    /* 4. end card (the launch film's) */
    #end { position: absolute; inset: 0; opacity: 0; }
    #fan svg { width: 100%; height: 100%; display: block; overflow: visible; }
    #fan { position: absolute; }
    .eline { position: absolute; left: 0; right: 0; text-align: center; color: #fff; white-space: nowrap; }
    #ename { font-weight: 700; letter-spacing: -0.04em; line-height: 0.98; }
    #etag { font-weight: 500; letter-spacing: -0.015em; line-height: 1.1; color: rgba(255,255,255,.9); }
    #eurl { font-family: "Geist Mono", ui-monospace, monospace; font-weight: 500; letter-spacing: -0.01em; line-height: 1.1; color: rgba(255,255,255,.92); }
    $end_css
  </style>

  <div id="root" data-composition-id="$cid" data-width="$W" data-height="$H">
    <div id="ground"></div>
    <div id="blue"></div>
    <div id="lift"></div>
    <img id="grain" src="assets/grain/grain.png" alt="">

    <div id="pain">
      <div class="pl" id="pl1">A quick sum.</div>
      <div class="pl" id="pl2">A unit conversion.</div>
      <div class="pl" id="pl3">Another browser tab.</div>
    </div>

    <div class="keygroup" id="kg-mac"><div class="keyrow"><div class="key" style="width:156px">&#x2325;</div><div class="key" style="width:375px">Space</div></div><div class="cap-label">macOS</div></div>
    <div class="keygroup" id="kg-win"><div class="keyrow"><div class="key" style="width:225px">Ctrl</div><div class="key" style="width:195px">Alt</div><div class="key" style="width:375px">Space</div></div><div class="cap-label">Windows and Linux</div></div>

    <div class="cap-line" id="capA">Type a sum.<span class="br"> Return copies <span class="v">360</span>.</span></div>
    <div class="cap-line" id="capB">Convert units.<span class="br"> Return copies <span class="v">22.2222</span>.</span></div>
    <div class="cap-line" id="capC">A tool search<span class="br"> stays a tool search.</span></div>

    <div id="push" data-layout-allow-overflow>
      <div id="open">
        <div id="plate" data-layout-allow-overflow>
          <img id="base" src="assets/plates/base.png" alt="">
          <img id="Ah" src="assets/plates/A-head.png" alt=""><img id="Ab" src="assets/plates/A-body.png" alt="">
          <img id="Bh" src="assets/plates/B-head.png" alt=""><img id="Bb" src="assets/plates/B-body.png" alt="">
          <img id="Ch" src="assets/plates/C-head.png" alt=""><img id="Cb" src="assets/plates/C-body.png" alt="">
        </div>
      </div>
    </div>

    <div id="proof">
      <div class="pcol" id="pc1" style="$p1"><div class="pnum">&lt;1<small>ms</small></div><div class="plab">per query,<br>asserted by a test</div></div>
      <div class="pcol" id="pc2" style="$p2"><div class="pnum">8</div><div class="plab">kinds of units,<br>length to temperature</div></div>
      <div class="pcol" id="pc3" style="$p3"><div class="pnum">29</div><div class="plab">calc and search<br>tests pass</div></div>
      <div id="pnote">Pure Rust. No network.</div>
    </div>

    <div id="end">
      <div id="fan"></div>
      <div class="eline" id="ename">Panoply</div>
      <div class="eline" id="etag">$etag</div>
      <div class="eline" id="eurl">github.com/anzal1/panoply</div>
    </div>
  </div>

  <script>
    (() => {
      const F = 1 / 60;
      const XO = 'expo.out';
      const tl = gsap.timeline({ paused: true });

      /* the fan mark, same path as docs/assets/panoply-mark.svg, three cards about one pivot (as the film's end card) */
      const CARD = 'M316.414 65.569C331.243 65.569 338.657 65.569 344.722 67.279A45.552 45.552 0 0 1 376.199 98.757C377.909 104.821 377.909 112.235 377.909 127.065L377.909 382.508C377.909 397.338 377.909 404.752 376.199 410.816A45.552 45.552 0 0 1 344.722 442.294C338.657 444.004 331.243 444.004 316.414 444.004L229.163 444.004C214.334 444.004 206.919 444.004 200.855 442.294A45.552 45.552 0 0 1 169.378 410.816C167.668 404.752 167.668 397.338 167.668 382.508L167.668 127.065C167.668 112.235 167.668 104.821 169.378 98.757A45.552 45.552 0 0 1 200.855 67.279C206.919 65.569 214.334 65.569 229.163 65.569Z';
      const PIVOT = '272.788 612.197';
      document.getElementById('fan').innerHTML =
        '<svg viewBox="0 0 544 544" fill="none">' +
        '<path id="cardL" d="' + CARD + '" fill="rgba(255,255,255,.42)"/>' +
        '<path id="cardR" d="' + CARD + '" fill="rgba(255,255,255,.62)"/>' +
        '<path id="cardF" d="' + CARD + '" fill="#ffffff"/></svg>';
      gsap.set('#cardL', { rotation: -12, svgOrigin: PIVOT });
      gsap.set('#cardR', { rotation: 12, svgOrigin: PIVOT });

      const land = (sel, t, o = {}) => {
        const ir = t === 0;
        tl.fromTo(sel, { x: o.x || 0, y: o.y === undefined ? 24 : o.y }, { x: 0, y: 0, duration: o.dur || 0.45, ease: XO, immediateRender: ir }, t);
        tl.fromTo(sel, { opacity: 0.5 }, { opacity: 1, duration: 0.08, ease: 'power1.out', immediateRender: ir }, t);
      };
      const hide = (sel, t) => tl.set(sel, { opacity: 0 }, t);
      /* a caption hands over to the next on one frame: the old one is out and the new one is in, never a gap or a double */
      const swapCap = (from, to, t) => { hide(from, t - F); land(to, t - F, { y: 14, dur: 0.35 }); };
      const fade = (sel, t, d, to) => tl.to(sel, { opacity: to, duration: d, ease: 'none', immediateRender: false }, t);

      /* ---------- the schedule, in seconds. X is the hand-over from Midnight to blue ---------- */
      const X = 2.9;

      /* 1. the pain: line 1 is on frame 0, the next two land at 0.5 and 1.0, line 3 is built by 1.45 and holds 1.45 s */
      gsap.set('#pl1', { opacity: 1 });
      land('#pl2', 0.5, { y: 30 });
      land('#pl3', 1.0, { y: 30 });
      tl.to('#pl1', { color: '#6f6f7a', duration: 0.3, ease: 'none' }, 0.5);
      tl.to('#pl2', { color: '#6f6f7a', duration: 0.3, ease: 'none' }, 1.0);
      tl.fromTo('#pain', { scale: 1 }, { scale: 1.03, duration: X + 0.35, ease: 'none', immediateRender: false }, 0);

      /* 2. crossfade to brand blue; both keycap sets land together and hold 1 s */
      fade('#blue', X, 0.35, 1);
      fade('#lift', X, 0.35, 1);
      fade('#pain', X, 0.2, 0);        /* the Midnight words are gone before the keycaps land: no double exposure */
      land('#kg-mac', X + 0.22, { y: 30 });
      land('#kg-win', X + 0.3, { y: 30 });
      const BAR = X + 1.55;            /* keys out, bar in: a crossfade. The keys are fully built 0.67 s after landing and hold 1.0 s. */
      fade(['#kg-mac', '#kg-win'], BAR, 0.3, 0);

      /* the bar, cropped to its top third (query and answer row) at 1.8x of the render. The camera drifts 2.5 percent over the bar's time. */
      const SC = $zoom / (1100 / 1440);
      gsap.set('#push', { scale: SC });
      tl.set('#open', { visibility: 'visible' }, BAR);
      tl.fromTo('#open', { opacity: 0 }, { opacity: 1, duration: 0.3, ease: 'none', immediateRender: false }, BAR);
      tl.fromTo('#push', { scale: SC }, { scale: SC * $drift, duration: 10.0, ease: 'none', immediateRender: false }, BAR);

      /* ---------- the typing: real glyphs of the real renders, one character per keystroke ---------- */
      const BOUNDS = {
        A: [115, 141, 176, 176, 208, 222, 222, 253, 278, 301, 1440],
        B: [122, 145, 145, 168, 168, 191, 212, 212, 1440],
        C: [112, 133, 155, 190, 213, 213, 245, 267, 1440]
      };
      const clip = (xb) => 'inset(0% ' + (100 - xb / 14.4).toFixed(3) + '% 0% 0%)';
      const type = (key, t0, step) => {
        const head = '#' + key + 'h';
        gsap.set(head, { clipPath: clip(95) });
        tl.set(head, { opacity: 1, clipPath: clip(95) }, t0 - 0.002);
        BOUNDS[key].forEach((xb, i) => tl.set(head, { clipPath: clip(xb) }, t0 + i * step));
        return t0 + (BOUNDS[key].length - 1) * step;     /* the last keystroke, when the answer lands */
      };
      const land_body = (key, t) => tl.fromTo('#' + key + 'b', { opacity: 0 }, { opacity: 1, duration: 0.12, ease: 'power1.out', immediateRender: false }, t);
      const clear = (key, t) => { hide('#' + key + 'h', t); hide('#' + key + 'b', t); };
      gsap.set('#base', { opacity: 1 });

      /* A: "15% of 2400". The caption holds 2.5 s. */
      land('#capA', BAR + 0.1, { y: 20, dur: 0.5 });
      land_body('A', type('A', BAR + 0.3, 0.085));
      const A_OUT = BAR + 2.6;
      clear('A', A_OUT - F);
      /* B: "72 f to c". The caption holds 2.7 s. */
      swapCap('#capA', '#capB', A_OUT);
      land_body('B', type('B', A_OUT + 0.15, 0.095));
      const B_OUT = A_OUT + 2.7;
      clear('B', B_OUT - F);
      /* C: "merge pdf", a tool and not a number. The caption holds 3.5 s. */
      swapCap('#capB', '#capC', B_OUT);
      land_body('C', type('C', B_OUT + 0.15, 0.095));
      const C_OUT = B_OUT + 3.45;

      /* ---------- the bar leaves, the proof lands and holds 2.1 s once complete ---------- */
      tl.to(['#open', '#capC'], { opacity: 0, duration: 0.3, ease: 'none', immediateRender: false }, C_OUT);
      const P = C_OUT + 0.2;
      land('#pc1', P, { y: 30 });
      land('#pc2', P + 0.3, { y: 30 });
      land('#pc3', P + 0.6, { y: 30 });
      land('#pnote', P + 0.9, { y: 20 });
      const P_OUT = P + 0.9 + 0.45 + 2.1;
      tl.fromTo('#proof', { scale: 1 }, { scale: 1.02, duration: P_OUT - P, ease: 'none', immediateRender: false }, P);
      fade('#proof', P_OUT, 0.25, 0);

      /* ---------- 4. end card: the URL is fully in 0.45 s after it lands and holds 2.1 s ---------- */
      const E = P_OUT + 0.25;
      tl.set('#end', { opacity: 1 }, E);
      gsap.set(['#etag', '#eurl'], { opacity: 0 });
      land('#ename', E, { y: 30 });
      land('#fan', E, { y: 30 });
      tl.fromTo('#cardL', { rotation: -12 }, { rotation: -20, duration: 0.6, ease: XO, svgOrigin: PIVOT, immediateRender: false }, E);
      tl.fromTo('#cardR', { rotation: 12 }, { rotation: 20, duration: 0.6, ease: XO, svgOrigin: PIVOT, immediateRender: false }, E);
      land('#etag', E + 0.25, { y: 24 });
      land('#eurl', E + 0.4, { y: 24 });
      window.__END_TIME = E + 0.4 + 0.45 + 2.1;

      window.__timelines["$cid"] = tl;
    })();
  </script>
</template>
</body></html>
''')

INDEX = Template('''<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=$W, height=$H" />
    <script src="assets/vendor/gsap.min.js"></script>
    <style>
      * { margin: 0; padding: 0; box-sizing: border-box; }
      @font-face { font-family: "Geist"; src: url("assets/fonts/Geist-Variable-latin.woff2") format("woff2"); font-weight: 100 900; }
      @font-face { font-family: "Geist Mono"; src: url("assets/fonts/GeistMono-latin.woff2") format("woff2"); font-weight: 100 900; }
      html, body { margin: 0; width: ${W}px; height: ${H}px; overflow: hidden; background: #0c0c0f; }
    </style>
  </head>
  <body>
    <div id="root" data-composition-id="main" data-start="0" data-duration="$DUR" data-width="$W" data-height="$H">
    <div id="host-$cid" class="clip" data-composition-id="$cid" data-composition-src="compositions/$cid.html"
         data-start="0" data-duration="$DUR" data-track-index="1" data-width="$W" data-height="$H"></div>
    </div>
    <script>
      const tl = gsap.timeline({ paused: true });
      window.__timelines["main"] = tl;
      tl.seek(0);
    </script>
  </body>
</html>
''')

if __name__ == "__main__":
    import json, shutil
    for cid, c in FORMATS.items():
        d = ROOT if cid == "bar-answers" else ROOT.parent / cid
        (d / "compositions").mkdir(parents=True, exist_ok=True)
        p1, p2, p3 = c["proof_pos"]
        (d / "compositions" / f"{cid}.html").write_text(TEMPLATE.substitute(cid=cid, p1=p1, p2=p2, p3=p3, **{k: v for k, v in c.items() if k != "proof_pos"}))
        (d / "index.html").write_text(INDEX.substitute(cid=cid, **c))
        if d != ROOT:
            for sub in ("assets/plates", "assets/fonts", "assets/grain"):
                (d / sub).mkdir(parents=True, exist_ok=True)
                for f in (ROOT / sub).iterdir():
                    shutil.copy(f, d / sub / f.name)
            for f in ("hyperframes.json", "package.json"):
                shutil.copy(ROOT / f, d / f)
            (d / "meta.json").write_text(json.dumps({"id": cid, "name": cid, "createdAt": "2026-10-18T09:00:00.000Z"}, indent=2) + "\n")
            pk = json.loads((d / "package.json").read_text()); pk["name"] = cid
            (d / "package.json").write_text(json.dumps(pk, indent=2) + "\n")
    print("ok")
