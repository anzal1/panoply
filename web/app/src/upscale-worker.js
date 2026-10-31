// Upscale: Real-ESRGAN general x4v3, the desktop tiling (128 px tiles, 96 px cores, 16 px mirrored context, no seams), a 2x result
// as exact 2 x 2 averages of the 4x tile, and transparency scaled separately by bilinear sampling.
import * as ort from 'onnxruntime-web/wasm';
import { loadModelFiles } from './ai/models.js';
ort.env.logLevel = 'error'; ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
const TILE = 128, MARGIN = 16, CORE = TILE - 2 * MARGIN, MS = 4;
let session = null;
async function getSession(progress) {
  if (session) return session;
  const f = await loadModelFiles('esrgan', progress);
  return (session = await ort.InferenceSession.create(f['real_esrgan_general_x4v3.onnx'], { executionProviders: ['wasm'], graphOptimizationLevel: 'all', logSeverityLevel: 3, externalData: [{ path: 'real_esrgan_general_x4v3.data', data: f['real_esrgan_general_x4v3.data'] }] }));
}
const reflect = (i, n) => { if (n === 1) return 0; const p = 2 * (n - 1); let m = ((i % p) + p) % p; if (m >= n) m = p - m; return m; };
const sampleA = (a, w, h, x, y) => { x = Math.min(Math.max(x, 0), w - 1); y = Math.min(Math.max(y, 0), h - 1); const x0 = x | 0, y0 = y | 0, x1 = Math.min(x0 + 1, w - 1), y1 = Math.min(y0 + 1, h - 1), fx = x - x0, fy = y - y0; return ((a[y0 * w + x0] * (1 - fx) + a[y0 * w + x1] * fx) * (1 - fy) + (a[y1 * w + x0] * (1 - fx) + a[y1 * w + x1] * fx) * fy + 0.5) | 0; };
onmessage = async (e) => {
  const m = e.data, lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  try {
    const s = await getSession((p, c) => postMessage({ id: m.id, progress: p, cached: c })); tick('model');
    const bmp = await createImageBitmap(m.blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' }), w = bmp.width, h = bmp.height;
    if (w * h > 16e6) throw new Error(`This picture is already ${w} x ${h}. Upscaling it gains little and would not fit in memory.`);
    const long = Math.max(w, h); let scale = m.scale;
    if (!scale) { if (long > 2000) throw new Error(`This picture is already ${w} x ${h}. Upscaling it gains little, so nothing was done. Choose 2x to do it anyway.`); scale = long < 1000 ? 4 : 2; }
    if (w * scale * h * scale > 50e6) throw new Error(`The result would be ${w * scale} x ${h * scale}, over this page's limit of 50 megapixels. Choose 2x, or make the picture smaller first.`);
    const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(bmp, 0, 0); bmp.close();
    const px = g.getImageData(0, 0, w, h).data, alpha = new Uint8Array(w * h); let hasA = false;
    for (let i = 0; i < w * h; i++) { alpha[i] = px[i * 4 + 3]; if (alpha[i] < 255) hasA = true; }
    tick('decode');
    const ow = w * scale, oh = h * scale, out = new Uint8ClampedArray(ow * oh * 4), sw = w * MS, tx = Math.ceil(w / CORE), ty = Math.ceil(h / CORE);
    const strip = new Uint8Array(CORE * MS * sw * 3), plane = TILE * TILE, side = TILE * MS, up_plane = side * side, off = MARGIN * MS;
    for (let yy = 0; yy < ty; yy++) {
      const cy = yy * CORE, coreH = Math.min(CORE, h - cy);
      for (let xx = 0; xx < tx; xx++) {
        const cx = xx * CORE, coreW = Math.min(CORE, w - cx), input = new Float32Array(3 * plane);
        for (let y = 0; y < TILE; y++) { const sy = reflect(cy - MARGIN + y, h); for (let x = 0; x < TILE; x++) { const sx = reflect(cx - MARGIN + x, w), p = (sy * w + sx) * 4, o = y * TILE + x; input[o] = px[p] / 255; input[plane + o] = px[p + 1] / 255; input[2 * plane + o] = px[p + 2] / 255; } }
        const r = await s.run({ image: new ort.Tensor('float32', input, [1, 3, TILE, TILE]) }, ['upscaled_image']), up = r.upscaled_image.data;
        for (let y = 0; y < coreH * MS; y++) { const drow = y * sw + cx * MS; for (let x = 0; x < coreW * MS; x++) { const si = (off + y) * side + off + x, d = (drow + x) * 3; const a = up[si], b = up[up_plane + si], cc = up[2 * up_plane + si]; strip[d] = (a < 0 ? 0 : a > 1 ? 1 : a) * 255 + 0.5; strip[d + 1] = (b < 0 ? 0 : b > 1 ? 1 : b) * 255 + 0.5; strip[d + 2] = (cc < 0 ? 0 : cc > 1 ? 1 : cc) * 255 + 0.5; } }
      }
      const rows4 = coreH * MS, rowsOut = coreH * scale;
      for (let oy = 0; oy < rowsOut; oy++) {
        const [y0, y1] = scale === 4 ? [oy, oy + 1] : [oy * 2, oy * 2 + 2], gy = cy * scale + oy;
        for (let ox = 0; ox < ow; ox++) {
          const [x0, x1] = scale === 4 ? [ox, ox + 1] : [ox * 2, ox * 2 + 2]; let a0 = 0, a1 = 0, a2 = 0;
          for (let yv = y0; yv < Math.min(y1, rows4); yv++) for (let xv = x0; xv < x1; xv++) { const q = (yv * sw + xv) * 3; a0 += strip[q]; a1 += strip[q + 1]; a2 += strip[q + 2]; }
          const n = (Math.min(y1, rows4) - y0) * (x1 - x0), d = (gy * ow + ox) * 4;
          out[d] = ((a0 + (n >> 1)) / n) | 0; out[d + 1] = ((a1 + (n >> 1)) / n) | 0; out[d + 2] = ((a2 + (n >> 1)) / n) | 0;
          out[d + 3] = hasA ? sampleA(alpha, w, h, (ox + 0.5) / scale - 0.5, (gy + 0.5) / scale - 0.5) : 255;
        }
      }
      postMessage({ id: m.id, progress2: (yy + 1) / ty });
    }
    tick('tiles');
    postMessage({ id: m.id, w: ow, h: oh, srcW: w, srcH: h, scale, hasAlpha: hasA, data: out, lap, tiles: tx * ty }, [out.buffer]);
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
