// Remove watermark: the desktop's classical detector (wasm), the YuNet face guard (faces are never filled in), then the
// MI-GAN fill or a blur of the surroundings.
import * as ort from 'onnxruntime-web/wasm';
import init, { Marks, Eraser } from './viswasm/panoply_web_vision.js';
import visUrl from './viswasm/panoply_web_vision_bg.wasm';
import { loadModel } from './ai/models.js';
ort.env.logLevel = 'error'; ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
const ready = init({ module_or_path: visUrl }), S = {}, MAX_PIXELS = 24_000_000;
const sess = async (k, progress) => (S[k] ||= ort.InferenceSession.create(await loadModel(k, progress), { executionProviders: ['wasm'], graphOptimizationLevel: 'all', logSeverityLevel: 3 }));
async function decode(blob) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' });
  const k = Math.min(1, Math.sqrt(MAX_PIXELS / (bmp.width * bmp.height))), w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
  const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, w, h); const fw = bmp.width, fh = bmp.height; bmp.close();
  return { w, h, data: new Uint8Array(g.getImageData(0, 0, w, h).data.buffer), shrunk: k < 1, fw, fh };
}
onmessage = async (e) => {
  const m = e.data, lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  const prog = (p, c) => postMessage({ id: m.id, progress: p, cached: c });
  try {
    await ready; const yu = await sess('yunet', prog); tick('face model');
    const img = await decode(m.blob); tick('decode');
    const marks = new Marks(img.data, img.w, img.h), inp = marks.face_input();
    const out = await yu.run({ [yu.inputNames[0]]: new ort.Tensor('float32', inp, [1, 3, 640, 640]) });
    for (const st of [8, 16, 32]) marks.face_put(st, out['cls_' + st].data, out['obj_' + st].data, out['bbox_' + st].data, out['kps_' + st].data);
    const nFaces = marks.face_done(); tick('faces');
    const det = JSON.parse(marks.detect(m.edges)); tick('detect');
    if (!det.regions.length) { postMessage({ id: m.id, none: true, faces: nFaces, spared: det.spared, lap, w: img.w, h: img.h }); marks.free(); return; }
    if (det.covered > 0.35) throw new Error('The marks cover a large part of the picture. A fill cannot restore that faithfully, so nothing was changed.');
    let data, areas = 0, changed = 0;
    const expand = m.expand ?? 4;
    if (m.fill === 'blur') { data = marks.blur_fill(img.data, expand); }
    else {
      const mg = await sess('migan', prog); tick('fill model');
      const er = new Eraser(img.data, img.w, img.h, marks.mask(), expand); areas = er.count();
      for (let i = 0; i < areas; i++) { const [mw, mh] = er.dims(i); const o = await mg.run({ image: new ort.Tensor('uint8', er.image(i), [1, 3, mh, mw]), mask: new ort.Tensor('uint8', er.keep(i), [1, 1, mh, mw]) }, ['result']); er.put(i, o.result.data); postMessage({ id: m.id, progress2: (i + 1) / areas }); }
      data = er.finish(); changed = er.changed_percent(); er.free();
    }
    tick('fill'); marks.free();
    postMessage({ id: m.id, data, w: img.w, h: img.h, regions: det.regions, spared: det.spared, faces: nFaces, areas, changed, lap, workShrunk: img.shrunk, fw: img.fw, fh: img.fh }, [data.buffer]);
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
