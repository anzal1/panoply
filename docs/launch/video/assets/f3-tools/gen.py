import json, os
HERE=os.path.dirname(os.path.abspath(__file__))
slots=json.load(open(os.path.join(HERE,'slots.json')))
rows=",\n      ".join(json.dumps(s,separators=(',',':')) for s in slots)
html=r'''<!doctype html>
<html><head><meta charset="UTF-8"></head><body>
<template>
  <style>
    @font-face{font-family:"Geist";src:url("assets/fonts/Geist-Variable-latin.woff2") format("woff2");font-weight:100 900}
    @font-face{font-family:"Geist Mono";src:url("assets/fonts/GeistMono-latin.woff2") format("woff2");font-weight:100 900}

    #root { position: absolute; inset: 0; overflow: hidden; font-family: "Geist", system-ui, sans-serif; color: #fff; }
    #root * { box-sizing: border-box; }

    /* ground: its own child, identical to frame 2's blue (flat brand blue, same very faint centred lift) so the cut at beat 24 is invisible */
    #f3-ground { position: absolute; inset: 0; background: radial-gradient(ellipse 60% 55% at 50% 48%, rgba(255,255,255,.07) 0%, rgba(255,255,255,0) 100%), #2d6cdf; }

    /* frame 2's headline and Ctrl Alt Space keycaps, still up at its last frame; they leave over 0.25 s from beat 24 */
    #f3-intro { position: absolute; inset: 0; }
    #f3-intro-head { position: absolute; left: 0; right: 0; top: 84px; text-align: center; font-size: 116px; font-weight: 700; letter-spacing: -0.04em; line-height: 0.98; color: #fff; }
    .f3-caps { position: absolute; left: 0; right: 0; top: 232px; height: 104px; display: flex; justify-content: center; gap: 24px; }
    .f3-cap { position: absolute; left: 0; right: 0; top: -46px; line-height: 40px; text-align: center; font-size: 36px; font-weight: 500; letter-spacing: -0.01em; color: rgba(255,255,255,.9); }
    .f3-key { height: 104px; border-radius: 26px; display: flex; align-items: center; justify-content: center; font-size: 52px; font-weight: 600; letter-spacing: -0.02em; color: #fff;
      background: rgba(255,255,255,.16); border: 2px solid rgba(255,255,255,.45);
      box-shadow: inset 0 1px 0 rgba(255,255,255,.35), 0 18px 40px rgba(8,24,70,.28); }

    /* the command bar plate, the exact geometry frame 2 ends on: 920 wide, top 396, scale 1.04 about its centre */
    #f3-slide { position: absolute; inset: 0; }
    #f3-push { position: absolute; left: 500px; top: 396px; width: 920px; height: 588px; transform-origin: 50% 50%; }
    #f3-plate { position: absolute; inset: 0; border-radius: 24.1px; overflow: hidden; background: #f1f2f5;
      box-shadow: 0 0 0 1.5px rgba(255,255,255,.35), 0 40px 100px rgba(8,24,70,.5); }
    #f3-plate img { position: absolute; left: 0; top: 0; width: 100%; height: 100%; display: block; }
    #f3-res-img { opacity: 0; }

    /* the payoff number above the plate */
    #f3-result { position: absolute; left: 0; right: 0; top: 190px; height: 150px; display: flex; justify-content: center; align-items: baseline;
      font-weight: 700; font-size: 150px; line-height: 150px; letter-spacing: -0.04em; white-space: nowrap; opacity: 0; }
    #f3-pct { display: inline-block; width: 2ch; text-align: right; font-variant-numeric: tabular-nums; }
    #f3-pct-sfx { display: inline-block; margin-left: .02em; }

    /* the grid of tool cards: x 95 to 1825, y 312 to 958 */
    #f3-grid { position: absolute; left: 95px; top: 312px; width: 1730px; height: 646px; }
    .f3-pos { position: absolute; height: 82px; }
    .f3-card { position: absolute; inset: 0; display: flex; align-items: center; padding: 0 26px 0 13px; gap: 16px; border-radius: 24px; background: #fff;
      box-shadow: 0 14px 34px rgba(8,24,70,.24), 0 2px 6px rgba(8,24,70,.12);
      font-weight: 600; font-size: 38px; line-height: 1; letter-spacing: -0.02em; color: #14161c; white-space: nowrap; }
    .f3-tile { position: relative; flex: none; width: 56px; height: 56px; border-radius: 16px; display: grid; place-items: center; }
    .f3-fill { position: absolute; inset: 0; border-radius: 16px; opacity: 0; }
    .f3-ic { position: relative; width: 32px; height: 32px; display: block; }
    .f3-ic svg { width: 32px; height: 32px; display: block; stroke: currentColor; fill: none; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; }

    /* counter and category label, top centre */
    #f3-head { position: absolute; left: 0; right: 0; top: 0; height: 300px; }
    #f3-count { position: absolute; left: 0; right: 0; top: 70px; height: 160px; display: flex; justify-content: center; align-items: center;
      font-weight: 700; font-size: 168px; line-height: 160px; letter-spacing: -0.04em; white-space: nowrap; opacity: 0; }
    #f3-nb { position: relative; width: 184px; height: 160px; flex: none; }
    .f3-n { position: absolute; right: 0; top: 0; height: 160px; line-height: 160px; opacity: 0; }
    #f3-tools { display: block; margin-left: 24px; opacity: 0; }
    #f3-labels { position: absolute; left: 0; right: 0; top: 236px; height: 52px; }
    .f3-label { position: absolute; left: 0; right: 0; top: 0; text-align: center; opacity: 0;
      font-weight: 600; font-size: 44px; line-height: 52px; letter-spacing: -0.015em; white-space: nowrap; color: rgba(255,255,255,.9); }

    /* the real Home screen the grid resolves into */
    #f3-home { position: absolute; left: 50%; top: 50%; width: 1500px; height: 931px; margin-left: -750px; margin-top: -465.5px;
      border-radius: 15.75px; overflow: hidden; opacity: 0; background: #f1f2f5;
      box-shadow: 0 0 0 1.5px rgba(255,255,255,.35), 0 34px 90px rgba(8,24,70,.42); }
    #f3-home img { width: 100%; height: 100%; display: block; }
  </style>

  <div id="root" data-composition-id="f3-tools" data-width="1920" data-height="1080">
    <div id="f3-ground"></div>
    <div id="f3-intro" data-layout-allow-overflow data-layout-allow-overlap>
      <div id="f3-intro-head">One hotkey. Any file.</div>
      <div class="f3-caps"><div class="f3-cap">Windows and Linux</div><div class="f3-key" style="width:150px">Ctrl</div><div class="f3-key" style="width:130px">Alt</div><div class="f3-key" style="width:250px">Space</div></div>
    </div>
    <div id="f3-slide" data-layout-allow-overflow><div id="f3-push" data-layout-allow-overflow><div id="f3-plate">
      <img id="f3-opt-img" src="assets/captures/demo-light-12.png" alt="">
      <img id="f3-res-img" src="assets/captures/bar-result-light.png" alt="">
    </div></div></div>
    <div id="f3-result" data-layout-allow-overlap aria-label="35% smaller"><span id="f3-pct">0</span><span id="f3-pct-sfx">% smaller</span></div>
    <div id="f3-grid" data-layout-allow-overflow></div>
    <div id="f3-head" data-layout-allow-overflow>
      <div id="f3-count"><div id="f3-nb"></div><div id="f3-tools">tools.</div></div>
      <div id="f3-labels"></div>
    </div>
    <div id="f3-home"><img src="assets/captures/app-home-light.png" alt=""></div>
  </div>

  <script>
    const BEAT = 60 / 130, START = 24;
    const at = b => (b - START) * BEAT;              /* local seconds of an absolute beat */

    /* ---- constants the real captures / the real build can change ---- */
    /* The running frames (run-light-01..06) are dropped on purpose: at beat 24 the options sheet goes straight to the result card. */
    /* These MUST equal the text in the real bar-result-light.png capture ("67 KB to 43 KB, 35% smaller"). */
    const RESULT = { from: "67 KB", to: "43 KB", pct: 35 };
    const TOOL_COUNT = 31;                            /* = assets/data/tools.json "count" */
    const PLATE_SCALE_START = 1.04;                   /* frame 2 ends on this (plate 920 wide, top 396) */

    /* Tools of assets/data/tools.json with a baked, seeded layout (assets/f3-tools/layout.py, LCG seed 94, annealed row packing).
       [id, name, category, x, y, width, row] in a 1730 x 646 grid, row-major arrival order, rows of 4,4,4,5,5,5,4.
       Regenerate with layout.py + gen.py if tools.json changes. */
    const TOOLS = [
      ROWS_HERE
    ];
    const ROW_OF = TOOLS.map(t => t[6]);

    const CAT = {
      pdf:   { name: "PDF",          col: "#ff453a", rgb: "255,69,58",  icon: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M9 13h6M9 17h4"/>' },
      image: { name: "Images",       col: "#bf5af2", rgb: "191,90,242", icon: '<rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="1.6"/><path d="m21 16-5-5-8 9"/>' },
      media: { name: "Video",        col: "#ff7828", rgb: "255,120,40", icon: '<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M8 4v16M16 4v16M3 9h5M3 15h5M16 9h5M16 15h5"/>' },
      id:    { name: "ID and forms", col: "#ff9f0a", rgb: "255,159,10", icon: '<rect x="3" y="5" width="18" height="14" rx="3"/><circle cx="9" cy="11" r="2"/><path d="M6 16c.6-1.7 1.8-2.4 3-2.4s2.4.7 3 2.4M14.5 10h4M14.5 14h3"/>' },
      files: { name: "Files",        col: "#1a84f3", rgb: "26,132,243", icon: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>' },
    };
    const CAT_ORDER = ["pdf", "image", "media", "id", "files"];   /* the five lights, beats 34 to 38 (names are the app's sidebar names) */
    const catCount = c => TOOLS.filter(t => t[2] === c).length;
    if (TOOLS.length !== TOOL_COUNT) { throw new Error("f3-tools: TOOLS.length " + TOOLS.length + " != tools.count " + TOOL_COUNT); }

    /* ---- build the DOM ---- */
    const $ = id => document.getElementById(id);
    const grid = $("f3-grid");
    const posEls = [], cardEls = [], fillEls = [], icEls = [];
    TOOLS.forEach(t => {
      const c = CAT[t[2]];
      const pos = document.createElement("div");
      pos.className = "f3-pos";
      pos.style.cssText = "left:" + t[3] + "px;top:" + t[4] + "px;width:" + t[5] + "px";
      pos.innerHTML =
        '<div class="f3-card"><span class="f3-tile" style="background:rgba(' + c.rgb + ',.16)">' +
        '<i class="f3-fill" style="background:' + c.col + '"></i>' +
        '<span class="f3-ic" style="color:' + c.col + '"><svg viewBox="0 0 24 24">' + c.icon + '</svg></span></span>' +
        '<span class="f3-name">' + t[1] + '</span></div>';
      grid.appendChild(pos);
      posEls.push(pos);
      cardEls.push(pos.querySelector(".f3-card"));
      fillEls.push(pos.querySelector(".f3-fill"));
      icEls.push(pos.querySelector(".f3-ic"));
    });

    const EARLY = [26, 27, 28, 29, 30, 31, 31.5, 32];      /* beats of the first eight cards (rows 0 and 1) */
    const nb = $("f3-nb");
    const numEls = {};
    const NUMS = EARLY.map((_, i) => i + 1).concat([TOOL_COUNT]);
    NUMS.forEach(n => {
      const s = document.createElement("div");
      s.className = "f3-n"; s.textContent = String(n);
      nb.appendChild(s); numEls[n] = s;
    });
    const labels = $("f3-labels");
    const labelEls = CAT_ORDER.map(k => {
      const d = document.createElement("div");
      d.className = "f3-label"; d.textContent = CAT[k].name + "  ·  " + catCount(k);
      labels.appendChild(d); return d;
    });

    /* ---- timeline ---- */
    const tl = gsap.timeline({ paused: true });
    const OUT = "expo.out", FADE = "power1.out";
    const NF = { immediateRender: false };

    gsap.set("#f3-push", { scale: PLATE_SCALE_START, transformOrigin: "50% 50%" });
    gsap.set("#f3-grid", { rotation: -1.5, transformOrigin: "50% 50%" });
    gsap.set("#f3-head", { scale: 1, transformOrigin: "50% 0%" });
    gsap.set(posEls, { opacity: 0, y: 80 });                 /* every card is hidden until its own beat */

    /* beat 24.0: options sheet cross-fades to the real result card in the same plate geometry (0.12 s) */
    tl.fromTo("#f3-res-img", { opacity: 0 }, { opacity: 1, duration: 0.12, ease: FADE, ...NF }, at(24));
    tl.set("#f3-opt-img", { opacity: 0 }, at(24) + 0.12);
    /* frame 2's headline and keycaps leave over 0.25 s (opacity and y -20) */
    tl.fromTo("#f3-intro", { y: 0 }, { y: -20, duration: 0.25, ease: OUT, ...NF }, at(24));
    tl.fromTo("#f3-intro", { opacity: 1 }, { opacity: 0, duration: 0.25, ease: FADE, ...NF }, at(24));
    /* the number counts 0 to 35 in 0.35 s from beat 24, then rests with the card until beat 26 */
    const cnt = { v: 0 };
    tl.to(cnt, { v: RESULT.pct, duration: 0.35, ease: OUT,
      onUpdate: () => { $("f3-pct").textContent = String(Math.round(cnt.v)); } }, at(24));
    /* the number fades in a tenth of a second after the count starts, so it does not sit on top of the keycaps that are leaving */
    tl.fromTo("#f3-result", { opacity: 0 }, { opacity: 1, duration: 0.15, ease: FADE, ...NF }, at(24) + 0.1);
    /* the only motion in the hold: the plate's slow push-in continues (frame 2 ended at 1.04) */
    tl.fromTo("#f3-push", { scale: PLATE_SCALE_START }, { scale: 1.058, duration: at(26.4) - at(24), ease: "none", ...NF }, at(24));

    /* beat 26.0: the card and the counter leave completely, the plate slides down 120 px and fades over 0.4 s */
    tl.fromTo("#f3-slide", { y: 0 }, { y: 120, duration: 0.4, ease: OUT, ...NF }, at(26));
    tl.fromTo("#f3-slide", { opacity: 1 }, { opacity: 0, duration: 0.4, ease: FADE, ...NF }, at(26));
    tl.fromTo("#f3-result", { opacity: 1 }, { opacity: 0, duration: 0.2, ease: FADE, ...NF }, at(26));

    /* the big counter appears with the first card */
    tl.fromTo("#f3-count", { y: 24 }, { y: 0, duration: 0.4, ease: OUT, ...NF }, at(26) + 0.1);
    tl.fromTo("#f3-count", { opacity: 0 }, { opacity: 1, duration: 0.25, ease: FADE, ...NF }, at(26) + 0.1);
    tl.set(numEls[1], { opacity: 1 }, 0);

    /* cards: every move STARTS on its beat. Transform glide expo.out 0.4 s, opacity its own power1.out tween. */
    const CARD_DUR = 0.4, OP_DUR = 0.25;
    const glide = (el, beat, dur, opDur) => {
      tl.fromTo(el, { y: 80 }, { y: 0, duration: dur, ease: OUT, ...NF }, at(beat));
      tl.fromTo(el, { opacity: 0 }, { opacity: 1, duration: opDur, ease: FADE, ...NF }, at(beat));
    };
    /* the 8th-note cards are shortened so that nothing is still moving when the stop begins at 32.61 */
    const durOf = b => Math.min(CARD_DUR, at(32.6) - at(b));
    EARLY.forEach((b, i) => {
      const d = durOf(b);
      glide(posEls[i], b, d, Math.min(OP_DUR, d));
      if (i > 0) {                                                     /* the counter ticks as the card starts to land */
        tl.set(numEls[i], { opacity: 0 }, at(b));
        tl.set(numEls[i + 1], { opacity: 1 }, at(b));
      }
    });

    /* STOP, beats 32.61 to 32.93: nothing moves, the drift tweens below end at 32.61 and restart at 32.93.
       HIT at beat 33.0: ordered ripple, row by row, 40 ms steps, every move starts at 33.0 + row * 40 ms. */
    const STOP_A = 32.61, STOP_B = 32.93, HIT = 33;
    const RIPPLE_STEP = 0.04, RIPPLE_DUR = 0.19, RIPPLE_OP = 0.15;
    const rippleStart = EARLY.length;
    for (let i = rippleStart; i < TOOLS.length; i++) {
      const r = ROW_OF[i] - ROW_OF[rippleStart];
      tl.fromTo(posEls[i], { y: 80 }, { y: 0, duration: RIPPLE_DUR, ease: OUT, ...NF }, at(HIT) + r * RIPPLE_STEP);
      tl.fromTo(posEls[i], { opacity: 0 }, { opacity: 1, duration: RIPPLE_OP, ease: FADE, ...NF }, at(HIT) + r * RIPPLE_STEP);
    }
    /* the counter LOCKS on the real count exactly at 33.0 and reads "31 tools." */
    tl.set(numEls[EARLY.length], { opacity: 0 }, at(HIT));
    tl.set(numEls[TOOL_COUNT], { opacity: 1 }, at(HIT));
    tl.fromTo("#f3-tools", { y: 24 }, { y: 0, duration: 0.4, ease: OUT, ...NF }, at(HIT));
    tl.fromTo("#f3-tools", { opacity: 0 }, { opacity: 1, duration: 0.25, ease: FADE, ...NF }, at(HIT));

    /* slow 3 degree orbit of the grid and slow drift of the counter, in two segments around the stop */
    const SEGS = [[26, STOP_A], [STOP_B, 38.5]];
    const TOT = SEGS.reduce((s, g) => s + g[1] - g[0], 0);
    let acc = 0;
    SEGS.forEach(([a, b]) => {
      const f0 = acc / TOT; acc += b - a; const f1 = acc / TOT;
      tl.fromTo("#f3-grid", { rotation: -1.5 + 3 * f0 }, { rotation: -1.5 + 3 * f1, duration: at(b) - at(a), ease: "none", ...NF }, at(a));
      tl.fromTo("#f3-head", { scale: 1 + 0.03 * f0 }, { scale: 1 + 0.03 * f1, duration: at(b) - at(a), ease: "none", ...NF }, at(a));
    });

    /* beats 34 to 38: one category per beat lights, everything else dims to 55 percent. All changes start on the beat. */
    const LIGHT_DUR = 0.3, DIM = 0.55;
    let prev = null;
    CAT_ORDER.forEach((cat, k) => {
      const st = at(34 + k);
      TOOLS.forEach((t, i) => {
        const lit = t[2] === cat, wasLit = t[2] === prev;
        const fromOp = prev === null ? 1 : (wasLit ? 1 : DIM);
        if (lit) {
          tl.fromTo(cardEls[i], { opacity: fromOp }, { opacity: 1, duration: LIGHT_DUR, ease: FADE, ...NF }, st);
          tl.fromTo(fillEls[i], { opacity: 0 }, { opacity: 1, duration: LIGHT_DUR, ease: FADE, ...NF }, st);
          tl.fromTo(icEls[i], { color: CAT[t[2]].col }, { color: "#ffffff", duration: LIGHT_DUR, ease: FADE, ...NF }, st);
        } else if (prev === null || wasLit) {
          tl.fromTo(cardEls[i], { opacity: fromOp }, { opacity: DIM, duration: LIGHT_DUR, ease: FADE, ...NF }, st);
          if (wasLit) {
            tl.fromTo(fillEls[i], { opacity: 1 }, { opacity: 0, duration: LIGHT_DUR, ease: FADE, ...NF }, st);
            tl.fromTo(icEls[i], { color: "#ffffff" }, { color: CAT[t[2]].col, duration: LIGHT_DUR, ease: FADE, ...NF }, st);
          }
        }
      });
      /* label: the previous one is gone by the beat, the new one starts on it */
      if (k > 0) tl.fromTo(labelEls[k - 1], { opacity: 1 }, { opacity: 0, duration: 0.12, ease: FADE, ...NF }, st - 0.12);
      tl.fromTo(labelEls[k], { y: 12 }, { y: 0, duration: 0.4, ease: OUT, ...NF }, st);
      tl.fromTo(labelEls[k], { opacity: 0 }, { opacity: 1, duration: OP_DUR, ease: FADE, ...NF }, st);
      prev = cat;
    });

    /* beat 37.5 to 38.5: the grid resolves into the real Home screen. The plate is fully opaque (38.2) before the grid goes
       under 50 percent (it only starts to fade then), and everything is still from 38.5 to 40. */
    tl.fromTo("#f3-home", { opacity: 0 }, { opacity: 1, duration: at(38.2) - at(37.5), ease: FADE, ...NF }, at(37.5));
    tl.fromTo("#f3-home", { scale: 0.97, transformOrigin: "50% 50%" }, { scale: 1, duration: 0.45, ease: OUT, ...NF }, at(37.5));
    tl.fromTo("#f3-head", { opacity: 1 }, { opacity: 0, duration: at(38.2) - at(37.5), ease: FADE, ...NF }, at(37.5));
    tl.fromTo("#f3-grid", { opacity: 1 }, { opacity: 0, duration: at(38.5) - at(38.2), ease: FADE, ...NF }, at(38.2));

    window.__timelines["f3-tools"] = tl;
  </script>
</template>
</body></html>
'''
html=html.replace('ROWS_HERE',rows)
out=os.environ.get('F3_OUT','/Users/anzalhussainabidi/personal/panoply-video/docs/launch/video/compositions/frames/03-tools.html')
open(out,'w').write(html)
