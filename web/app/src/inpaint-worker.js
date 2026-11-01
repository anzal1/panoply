// Erase: the MI-GAN fill model. The desktop planning (clusters, crops, grown masks, blending, the untouched-outside check) runs in
// wasm; ONNX Runtime Web runs the model on each crop.
import * as ort from 'onnxruntime-web/wasm';
import init, { Eraser } from './viswasm/panoply_web_vision.js';
import visUrl from './viswasm/panoply_web_vision_bg.wasm';
import { loadModel } from './ai/models.js';
ort.env.logLevel = 'error'; ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
const ready = init({ module_or_path: visUrl });
let session = null;
const MAX_PIXELS = 24_000_000;
async function getSession(progress) { return session || (session = await ort.InferenceSession.create(await loadModel('migan', progress), { executionProviders: ['wasm'], graphOptimizationLevel: 'all', logSeverityLevel: 3 })); }
async function decode(blob) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' });
  const k = Math.min(1, Math.sqrt(MAX_PIXELS / (bmp.width * bmp.height))), w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
  const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, w, h); bmp.close();
  return { w, h, data: new Uint8Array(g.getImageData(0, 0, w, h).data.buffer), shrunk: k < 1, fw: bmp.width, fh: bmp.height };
}
export async function erase(m, post) {
  const lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  await ready; const s = await getSession((p, c) => post({ id: m.id, progress: p, cached: c })); tick('model');
  const img = await decode(m.blob); tick('decode');
  const mask = new Uint8Array(img.w * img.h); // nearest-neighbour from the drawn mask
  if (m.fullMask) mask.set(m.fullMask); else for (let y = 0; y < img.h; y++) { const my = Math.min(m.mh - 1, Math.floor((y + 0.5) * m.mh / img.h)); for (let x = 0; x < img.w; x++) mask[y * img.w + x] = m.mask[my * m.mw + Math.min(m.mw - 1, Math.floor((x + 0.5) * m.mw / img.w))]; }
  const er = new Eraser(img.data, img.w, img.h, mask, m.expand ?? 8); const n = er.count(); tick('plan');
  for (let i = 0; i < n; i++) {
    const [mw, mh] = er.dims(i);
    const out = await s.run({ image: new ort.Tensor('uint8', er.image(i), [1, 3, mh, mw]), mask: new ort.Tensor('uint8', er.keep(i), [1, 1, mh, mw]) }, ['result']);
    er.put(i, out.result.data); post({ id: m.id, progress2: (i + 1) / n });
  }
  tick('fill');
  const data = er.finish(), res = { w: img.w, h: img.h, data, lap, areas: n, changed: er.changed_percent(), shrunk: er.shrunk(), workShrunk: img.shrunk, fw: img.fw, fh: img.fh }; er.free(); return res;
}
onmessage = async (e) => { try { const r = await erase(e.data, postMessage); postMessage({ id: e.data.id, ...r }, [r.data.buffer]); } catch (err) { postMessage({ id: e.data.id, error: String(err.message || err) }); } };
