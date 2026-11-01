// ID tools: the desktop signature cleaner pipeline in wasm.
import init, { Signature, SheetMaker } from './idwasm/panoply_web_id.js';
import idUrl from './idwasm/panoply_web_id_bg.wasm';
const ready = init({ module_or_path: idUrl });
const MAX_PIXELS = 24_000_000;
onmessage = async (e) => {
  const m = e.data; await ready;
  try {
    if (m.op === 'signature') {
      const bmp = await createImageBitmap(m.blob, { imageOrientation: 'from-image', premultiplyAlpha: 'none' });
      const k = Math.min(1, Math.sqrt(MAX_PIXELS / (bmp.width * bmp.height))), w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
      const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.imageSmoothingQuality = 'high'; g.drawImage(bmp, 0, 0, w, h); bmp.close();
      const px = new Uint8Array(g.getImageData(0, 0, w, h).data.buffer), s = new Signature(), t = performance.now();
      const out = s.run(px, w, h, m.ink, m.sw || 0, m.sh || 0, !!m.white, !!m.removeLines, m.thicken !== false);
      const info = JSON.parse(s.notes()); const res = { id: m.id, data: out, w: s.width(), h: s.height(), info, ms: performance.now() - t }; s.free(); postMessage(res, [out.buffer]);
    }
    else if (m.op === 'sheet') {
      const sm = new SheetMaker(); for (const ph of m.photos) sm.add_photo(new Uint8Array(ph.data), ph.w, ph.h);
      const out = sm.make(m.paper, m.cw, m.ch, m.margin, m.gutter, m.copies || 0, !!m.cutMarks, m.rotate !== false, m.dpi || 300), info = JSON.parse(sm.info()), res = { id: m.id, data: out, w: sm.width(), h: sm.height(), info }; sm.free(); postMessage(res, [out.buffer]);
    }
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
