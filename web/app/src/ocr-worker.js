// OCR: the desktop PP-OCRv5 pipeline. ONNX Runtime Web runs the pinned detector and recognizer; the desktop's own strip
// stitching, DB box finding, recognizer inputs, CTC decoding and reading order (wasm) do the rest.
import * as ort from 'onnxruntime-web/wasm';
import init, { Ocr } from './ocrwasm/panoply_web_ocr.js';
import ocrUrl from './ocrwasm/panoply_web_ocr_bg.wasm';
import { loadModel } from './ai/models.js';
import enDict from './ai/dicts/en.dict';
import latinDict from './ai/dicts/latin.dict';
import devaDict from './ai/dicts/devanagari.dict';

ort.env.logLevel = 'error';
ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
const ready = init({ module_or_path: ocrUrl });
const REC = { en: ['ppocr_en', enDict], latin: ['ppocr_latin', latinDict], devanagari: ['ppocr_deva', devaDict] };
const sessions = {};
const opts = { executionProviders: ['wasm'], graphOptimizationLevel: 'all', logSeverityLevel: 3 };
async function session(key, progress) { return (sessions[key] ||= ort.InferenceSession.create(await loadModel(key, progress), opts)); }
const MAX_PIXELS = 24_000_000;

async function decode(blob) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image' });
  const k = Math.min(1, Math.sqrt(MAX_PIXELS / (bmp.width * bmp.height))), w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
  const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, w, h); bmp.close();
  return { w, h, data: new Uint8Array(g.getImageData(0, 0, w, h).data.buffer) };
}
onmessage = async (e) => {
  const m = e.data, lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  try {
    await ready; const [recKey, dict] = REC[m.script || 'en'], prog = (p, cached) => postMessage({ id: m.id, progress: p, cached });
    const det = await session('ppocr_det', prog), rec = await session(recKey, prog); tick('models');
    const img = m.raw ? { w: m.raw.w, h: m.raw.h, data: new Uint8Array(m.raw.data) } : await decode(m.blob); tick('decode');
    const ocr = new Ocr(img.data, img.w, img.h, dict); tick('prep');
    const dw = ocr.det_width(), strips = ocr.det_strips(), dName = det.inputNames[0];
    for (let i = 0; i < strips.length; i += 4) {
      const [y0, y1, k0, k1] = [strips[i], strips[i + 1], strips[i + 2], strips[i + 3]];
      const out = await det.run({ [dName]: new ort.Tensor('float32', ocr.det_input(y0, y1), [1, 3, y1 - y0, dw]) }, [det.outputNames[0]]);
      ocr.det_put(y0, k0, k1, out[det.outputNames[0]].data);
    }
    const n = ocr.det_finish(); tick('detect');
    const rName = rec.inputNames[0], rOut = rec.outputNames[0];
    for (let i = 0; i < n; i++) {
      const w = ocr.rec_width(i), out = await rec.run({ [rName]: new ort.Tensor('float32', ocr.rec_input(i), [1, 3, 48, w]) }, [rOut]);
      const o = out[rOut]; ocr.rec_put(i, o.data, o.dims[1], o.dims[2]);
    }
    tick('recognize');
    const page = JSON.parse(ocr.json()); const text = ocr.text(!!m.keepBreaks), markdown = ocr.markdown(); ocr.free();
    postMessage({ id: m.id, page, text, markdown, lap, pieces: n, w: img.w, h: img.h });
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
