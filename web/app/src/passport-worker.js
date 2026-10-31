// Passport and visa photos: the desktop pipeline in wasm, with the YuNet face model (and, when the backdrop is too busy for the
// plain colour cut, the IS-Net cut-out) run by ONNX Runtime Web between steps.
import * as ort from 'onnxruntime-web/wasm';
import init, { Passport, passport_rules } from './idwasm/panoply_web_id.js';
import idUrl from './idwasm/panoply_web_id_bg.wasm';
import visInit, { isnet_prep, isnet_finish } from './viswasm/panoply_web_vision.js';
import visUrl from './viswasm/panoply_web_vision_bg.wasm';
import { loadModel } from './ai/models.js';
ort.env.logLevel = 'error'; ort.env.wasm.wasmPaths = new URL('./ort/', import.meta.url).href;
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1)) : 1;
const ready = init({ module_or_path: idUrl }), S = {}, MAX = 4096;
const sess = (k, progress) => (S[k] ||= loadModel(k, progress).then((b) => ort.InferenceSession.create(b, { executionProviders: ['wasm'], graphOptimizationLevel: 'all', logSeverityLevel: 3 })));
async function decode(blob) {
  const bmp = await createImageBitmap(blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' });
  const k = Math.min(1, MAX / Math.max(bmp.width, bmp.height)), w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
  const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, w, h); bmp.close();
  return { w, h, data: new Uint8Array(g.getImageData(0, 0, w, h).data.buffer) };
}
async function faces(p, yu) {
  for (let inp; (inp = p.face_next());) {
    const out = await yu.run({ [yu.inputNames[0]]: new ort.Tensor('float32', inp, [1, 3, 640, 640]) });
    for (const st of [8, 16, 32]) p.face_put(st, out['cls_' + st].data, out['obj_' + st].data, out['bbox_' + st].data, out['kps_' + st].data);
    p.face_pass_done();
  }
}
async function cutout(img, progress) {
  await visInit({ module_or_path: visUrl }); const s = await sess('isnet', progress);
  const input = isnet_prep(img.data, img.w, img.h, 1024), out = await s.run({ input: new ort.Tensor('float32', input, [1, 3, 1024, 1024]) }, ['output']);
  const rgba = isnet_finish(out.output.data, 1024, img.data, img.w, img.h, 0, 255, 255, 255, 30, true, true), a = new Uint8Array(img.w * img.h);
  for (let i = 0; i < a.length; i++) a[i] = rgba[i * 4 + 3]; return a;
}
onmessage = async (e) => {
  const m = e.data, lap = {}; let t = performance.now(); const tick = (k) => { const n = performance.now(); lap[k] = n - t; t = n; };
  const prog = (p, c) => postMessage({ id: m.id, progress: p, cached: c });
  try {
    await ready;
    if (m.op === 'rules') { postMessage({ id: m.id, rules: JSON.parse(passport_rules()) }); return; }
    const yu = await sess('yunet', prog); tick('model');
    if (m.op === 'faces-debug') { const img0 = await decode(m.blob), p0 = new Passport(img0.data, img0.w, img0.h), inp = p0.face_next(), out = await yu.run({ [yu.inputNames[0]]: new ort.Tensor('float32', inp, [1, 3, 640, 640]) }); for (const st of [8, 16, 32]) p0.face_put(st, out['cls_' + st].data, out['obj_' + st].data, out['bbox_' + st].data, out['kps_' + st].data); p0.face_pass_done(); const first = JSON.parse(p0.faces_json()); await faces(p0, yu); postMessage({ id: m.id, input: inp, first, refined: JSON.parse(p0.faces_json()) }); return; }
    const img = await decode(m.blob); tick('decode');
    const p = new Passport(img.data, img.w, img.h); await faces(p, yu); tick('faces');
    const opts = [m.rule, m.face || 0, m.fixTilt !== false, m.fixExposure !== false, m.whiten !== false, !!m.colorCutout, m.background || '', new Uint8Array(0)];
    let info = JSON.parse(p.prepare(...opts)), usedModel = false;
    if (m.whiten !== false && !m.colorCutout && info.reliable === false) { const alpha = await cutout(img, prog); tick('cutout model'); info = JSON.parse(p.prepare(...opts.slice(0, 7), alpha)); usedModel = true; }
    const sizes = m.sizes || [], renders = [];
    for (const [w, h] of sizes) { const data = p.render(w, h); renders.push({ data, w, h, report: JSON.parse(p.report()) }); }
    tick('render'); p.free();
    postMessage({ id: m.id, renders, info, usedModel, lap }, renders.map((r) => r.data.buffer));
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
