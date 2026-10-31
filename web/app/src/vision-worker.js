// Background removal: the desktop IS-Net pipeline. ONNX Runtime Web runs the model; the desktop's own resize, guided-filter edge
// refinement and foreground estimation (wasm) do the rest.
import * as ort from 'onnxruntime-web/wasm';
import init, { isnet_prep, isnet_finish } from './viswasm/panoply_web_vision.js';
import visUrl from './viswasm/panoply_web_vision_bg.wasm';
import { loadModel } from './ai/models.js';

ort.env.logLevel = 'error';
ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
let session = null, ep = '';
const ready = init({ module_or_path: visUrl });
const MAX_PIXELS = 24_000_000;

async function getSession(progress) {
  if (session) return session;
  const bytes = await loadModel('isnet', progress);
  session = await ort.InferenceSession.create(bytes, { executionProviders: ['wasm'], graphOptimizationLevel: 'basic', enableCpuMemArena: false, enableMemPattern: false, logSeverityLevel: 3 }); ep = 'wasm' + (ort.env.wasm.numThreads > 1 ? ` x${ort.env.wasm.numThreads}` : '');
  return session;
}
async function decode(blob) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' });
  let w = bmp.width, h = bmp.height, k = Math.min(1, Math.sqrt(MAX_PIXELS / (w * h)));
  const rw = Math.max(1, Math.round(w * k)), rh = Math.max(1, Math.round(h * k));
  const c = new OffscreenCanvas(rw, rh), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, rw, rh); bmp.close();
  const d = g.getImageData(0, 0, rw, rh); return { w: rw, h: rh, data: new Uint8Array(d.data.buffer), shrunk: k < 1, fw: w, fh: h };
}
onmessage = async (e) => {
  const m = e.data, lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  try {
    await ready; const s = await getSession((p, cached) => postMessage({ id: m.id, progress: p, cached })); tick('model');
    const src = await decode(m.blob); tick('decode');
    const side = m.side || 1024, input = isnet_prep(src.data, src.w, src.h, side); tick('prep');
    const out = await s.run({ input: new ort.Tensor('float32', input, [1, 3, side, side]) }, ['output']); tick('infer');
    const map = out.output.data;
    const look = { transparent: 0, color: 1, blur: 2 }[m.look] ?? 0, c = m.bg || [255, 255, 255];
    const rgba = isnet_finish(map, side, src.data, src.w, src.h, look, c[0], c[1], c[2], m.blur ?? 30, m.refine !== false, m.decontaminate !== false); tick('refine+compose');
    postMessage({ id: m.id, w: src.w, h: src.h, data: rgba, lap, ep, shrunk: src.shrunk, fw: src.fw, fh: src.fh, ...(m.debug ? { input, map: new Float32Array(map) } : {}) }, [rgba.buffer]);
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
