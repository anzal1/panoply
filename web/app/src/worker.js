// Worker: role 1 'prep' decodes and resizes (Lanczos3); role 2 'img'+'enc' encodes with the real MozJPEG.
import mozjpeg from '@jsquash/jpeg/codec/enc/mozjpeg_enc.js';
import initResize, { resize as sqResize } from '@jsquash/resize/lib/resize/pkg/squoosh_resize.js';
import mozWasmUrl from '@jsquash/jpeg/codec/enc/mozjpeg_enc.wasm';
import resizeWasmUrl from '@jsquash/resize/lib/resize/pkg/squoosh_resize_bg.wasm';

let moz, img = null, held = null;
const ready = (async () => { moz = await mozjpeg({ locateFile: () => mozWasmUrl }); await initResize(resizeWasmUrl); postMessage({ type: 'ready' }); })();

const opts = (q, sub420, progressive) => ({
  quality: q, baseline: !progressive, arithmetic: false, progressive, optimize_coding: true, smoothing: 0,
  color_space: 3, quant_table: 3, trellis_multipass: false, trellis_opt_zero: false, trellis_opt_table: false,
  trellis_loops: 1, auto_subsample: false, chroma_subsample: sub420 ? 2 : 1, separate_chroma_quality: false, chroma_quality: 75,
});

// Largest shrink the browser does on its own before Lanczos: stay at 2x the final size, so Lanczos still does the last halving.
async function decode(blob, plan, noReduce) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image' });
  const w = bmp.width, h = bmp.height, p = plan(w, h);
  const k = noReduce ? 1 : Math.min(1, (2 * p.sw) / w, (2 * p.sh) / h);
  const rw = k < 0.5 ? Math.max(1, Math.round(w * k)) : w, rh = k < 0.5 ? Math.max(1, Math.round(h * k)) : h;
  const c = new OffscreenCanvas(rw, rh), g = c.getContext('2d', { willReadFrequently: true });
  g.imageSmoothingQuality = 'high'; g.fillStyle = '#fff'; g.fillRect(0, 0, rw, rh); g.drawImage(bmp, 0, 0, rw, rh); bmp.close();
  const d = g.getImageData(0, 0, rw, rh);
  return { img: { w: rw, h: rh, data: new Uint8Array(d.data.buffer) }, srcW: w, srcH: h, plan: p, reduced: rw !== w };
}
const lanczos = (im, w, h) => (im.w === w && im.h === h) ? im : { w, h, data: new Uint8Array(sqResize(im.data, im.w, im.h, w, h, 3, false, true).buffer) };

function planFor(bw, bh) {
  return (w, h) => {
    if (bw && bh) {
      const crop = Math.abs(w / h - bw / bh) / (bw / bh) <= 0.15;
      if (crop) { const s = Math.max(bw / w, bh / h); return { mode: 'crop', sw: Math.max(bw, Math.round(w * s)), sh: Math.max(bh, Math.round(h * s)) }; }
      const s = Math.min(bw / w, bh / h); return { mode: 'pad', sw: Math.min(bw, Math.max(1, Math.round(w * s))), sh: Math.min(bh, Math.max(1, Math.round(h * s))) };
    }
    const s = Math.min(1, 4000 / Math.max(w, h)); return { mode: 'free', sw: Math.max(1, Math.round(w * s)), sh: Math.max(1, Math.round(h * s)) };
  };
}
function compose(d, bw, bh) {
  const r = lanczos(d.img, d.plan.sw, d.plan.sh), { mode, sw, sh } = d.plan;
  if (mode === 'free') return r;
  if (mode === 'crop') { const x0 = (sw - bw) >> 1, y0 = (sh - bh) >> 1, out = new Uint8Array(bw * bh * 4); for (let y = 0; y < bh; y++) out.set(r.data.subarray(((y + y0) * sw + x0) * 4, ((y + y0) * sw + x0 + bw) * 4), y * bw * 4); return { w: bw, h: bh, data: out }; }
  const out = new Uint8Array(bw * bh * 4).fill(255), x0 = (bw - sw) >> 1, y0 = (bh - sh) >> 1;
  for (let y = 0; y < sh; y++) out.set(r.data.subarray(y * sw * 4, (y + 1) * sw * 4), ((y + y0) * bw + x0) * 4);
  return { w: bw, h: bh, data: out };
}

onmessage = async (e) => {
  await ready; const m = e.data;
  try {
    if (m.type === 'prep') {
      const t = performance.now(), d = await decode(m.blob, planFor(m.w, m.h), m.noReduce), t1 = performance.now();
      const base = compose(d, m.w, m.h);
      held = !(m.w && m.h) ? { w: base.w, h: base.h, data: base.data.slice() } : null; // free size may need to shrink
      postMessage({ type: 'prepped', base, srcW: d.srcW, srcH: d.srcH, padded: d.plan.mode === 'pad', reduced: d.reduced, lap: { decode: t1 - t, resize: performance.now() - t1 }, heap: performance.memory ? performance.memory.usedJSHeapSize : null, wasmMem: moz.HEAP8.length }, [base.data.buffer]);
    } else if (m.type === 'shrink') {
      // The desktop's shrink_to_fit: the largest scale at which quality 75 (4:2:0) fits under the cap, found by bisection.
      const at = (sc) => lanczos(held, Math.max(8, Math.round(held.w * sc)), Math.max(8, Math.round(held.h * sc)));
      const fits = (im) => moz.encode(im.data, im.w, im.h, opts(75, true, !m.baseline)).length <= m.cap;
      let lo = 0.02, hi = 1.0;
      if (!fits(at(lo))) throw new Error('This picture could not be brought under ' + m.cap.toLocaleString() + ' bytes. Choose a larger limit.');
      for (let i = 0; i < 9; i++) { const mid = (lo + hi) / 2; if (fits(at(mid))) lo = mid; else hi = mid; }
      const small = at(lo); postMessage({ id: m.id, type: 'shrunk', shrunk: { w: small.w, h: small.h, data: small.data, from: [held.w, held.h] } }, [small.data.buffer]);
    } else if (m.type === 'img') { img = m; postMessage({ type: 'imgok' }); }
    else if (m.type === 'enc') {
      const b = moz.encode(img.data, img.w, img.h, opts(m.q, m.sub, m.pr)).slice();
      postMessage({ type: 'encd', id: m.id, q: m.q, bytes: b }, [b.buffer]);
    }
  } catch (err) { postMessage({ type: 'error', id: m.id, for: m.type, message: String(err.message || err) }); }
};
