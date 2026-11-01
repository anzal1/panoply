// One worker for the image tools: decode with the browser, optional Lanczos3 resize, encode with the real codecs
// (MozJPEG, libwebp, libavif, oxipng). Codecs load on first use, so a page only pays for what it runs.
import { urls, png_, oxi_, oxiMt_, resize_ } from './wasmurls.js';
import initResize, { resize as sqResize } from '@jsquash/resize/lib/resize/pkg/squoosh_resize.js';

const locateFile = (n) => urls[n.split('/').pop()] || n;
const lazy = {};
const once = (k, f) => (lazy[k] ||= f());
const resizeReady = once('resize', () => initResize(resize_));

async function decode(blob, alpha) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image', premultiplyAlpha: alpha ? 'none' : 'default' });
  const c = new OffscreenCanvas(bmp.width, bmp.height), g = c.getContext('2d', { willReadFrequently: true });
  g.drawImage(bmp, 0, 0); bmp.close();
  return g.getImageData(0, 0, c.width, c.height);
}
function hasAlpha(d) { for (let i = 3; i < d.data.length; i += 4 * 7) if (d.data[i] < 255) return true; for (let i = 3; i < d.data.length; i += 4) if (d.data[i] < 255) return true; return false; }
function flatten(d, bg) { const p = d.data; for (let i = 0; i < p.length; i += 4) { const a = p[i + 3]; if (a < 255) { p[i] = (p[i] * a + bg[0] * (255 - a)) / 255; p[i + 1] = (p[i + 1] * a + bg[1] * (255 - a)) / 255; p[i + 2] = (p[i + 2] * a + bg[2] * (255 - a)) / 255; p[i + 3] = 255; } } }
async function lanczos(d, w, h, alpha) {
  await resizeReady; if (d.width === w && d.height === h) return d;
  const out = sqResize(new Uint8Array(d.data.buffer), d.width, d.height, w, h, 3, alpha, true);
  return new ImageData(new Uint8ClampedArray(out.buffer), w, h);
}
function plan(sw, sh, r) { // returns [outW, outH, drawMode]
  let { w, h, fit } = r;
  if (r.percent) { w = Math.round(sw * r.percent / 100); h = Math.round(sh * r.percent / 100); fit = 'stretch'; }
  if (!w && !h) return [sw, sh, 'stretch'];
  if (!w) w = Math.round(sw * h / sh); if (!h) h = Math.round(sh * w / sw);
  return [w, h, fit];
}
async function applyResize(d, r, alpha) {
  const [w, h, fit] = plan(d.width, d.height, r);
  if (fit === 'stretch') return lanczos(d, Math.max(1, w), Math.max(1, h), alpha);
  if (fit === 'inside') { const s = Math.min(w / d.width, h / d.height); return lanczos(d, Math.max(1, Math.round(d.width * s)), Math.max(1, Math.round(d.height * s)), alpha); }
  const s = Math.max(w / d.width, h / d.height), sw = Math.max(w, Math.round(d.width * s)), sh = Math.max(h, Math.round(d.height * s));
  const big = await lanczos(d, sw, sh, alpha), x0 = (sw - w) >> 1, y0 = (sh - h) >> 1, out = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y++) out.set(big.data.subarray(((y + y0) * sw + x0) * 4, ((y + y0) * sw + x0 + w) * 4), y * w * 4);
  return new ImageData(out, w, h);
}

async function encode(d, o) {
  if (o.fmt === 'jpeg') {
    const m = await once('moz', async () => (await import('@jsquash/jpeg/codec/enc/mozjpeg_enc.js')).default({ locateFile }));
    const b = m.encode(d.data, d.width, d.height, { quality: o.q, baseline: false, arithmetic: false, progressive: true, optimize_coding: true, smoothing: 0, color_space: 3, quant_table: 3, trellis_multipass: false, trellis_opt_zero: false, trellis_opt_table: false, trellis_loops: 1, auto_subsample: true, chroma_subsample: 2, separate_chroma_quality: false, chroma_quality: 75 });
    return { bytes: b.slice(), type: 'image/jpeg', ext: 'jpg' };
  }
  if (o.fmt === 'webp') {
    const { init, default: enc } = await once('webp', async () => { const m = await import('@jsquash/webp/encode.js'); await m.init({ locateFile }); return m; });
    return { bytes: new Uint8Array(await enc(d, { quality: o.q, lossless: o.lossless ? 1 : 0, method: 4 })), type: 'image/webp', ext: 'webp' };
  }
  if (o.fmt === 'avif') { // single-thread build on purpose: the multi-thread build's worker pool does not survive bundling
    const m = await once('avif', async () => (await import('@jsquash/avif/codec/enc/avif_enc.js')).default({ locateFile }));
    const { defaultOptions } = await import('@jsquash/avif/meta.js');
    const r = m.encode(d.data, d.width, d.height, { ...defaultOptions, quality: o.q, speed: o.speed ?? 6, subsample: 1 });
    if (!r) throw new Error('AVIF encoding failed.');
    return { bytes: new Uint8Array(r.buffer.slice(r.byteOffset, r.byteOffset + r.byteLength)), type: 'image/avif', ext: 'avif' };
  }
  // png: lossless encode, then oxipng
  const pe = await once('png', async () => { const m = await import('@jsquash/png/encode.js'); await m.init(png_); return m; });
  let buf = await pe.default(d);
  if (o.fast && d.width * d.height > 6e6) return { bytes: new Uint8Array(buf), type: 'image/png', ext: 'png' };
  const ox = await once('oxi', async () => { const m = await import('@jsquash/oxipng/codec/pkg/squoosh_oxipng.js'); await m.default(oxi_); return m; });
  const mp = d.width * d.height / 1e6, level = mp > 6 ? 1 : mp > 2 ? 2 : (o.level ?? 3);
  buf = ox.optimise(new Uint8Array(buf), level, false, true).buffer;
  return { bytes: new Uint8Array(buf), type: 'image/png', ext: 'png' };
}

onmessage = async (e) => {
  const m = e.data; const t0 = performance.now();
  try {
    let d = m.raw ? new ImageData(new Uint8ClampedArray(m.raw.data), m.raw.w, m.raw.h) : await decode(m.blob, true); const srcW = d.width, srcH = d.height, alpha = hasAlpha(d), lap = { decode: performance.now() - t0 };
    const t1 = performance.now();
    if (m.resize) d = await applyResize(d, m.resize, alpha);
    lap.resize = performance.now() - t1;
    if (m.out.fmt === 'jpeg') flatten(d, m.out.bg || [255, 255, 255]);
    const t2 = performance.now(), r = await encode(d, m.out); lap.encode = performance.now() - t2;
    postMessage({ id: m.id, ...r, width: d.width, height: d.height, srcW, srcH, alpha, lap, ms: performance.now() - t0 }, [r.bytes.buffer]);
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
