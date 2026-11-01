// pdfium compiled to wasm (BSD-3 and Apache-2), the engine desktop Panoply draws pages with.
import { PDFiumLibrary } from '@hyzyla/pdfium';
import wasmUrl from '../node_modules/@hyzyla/pdfium/dist/pdfium.wasm';
let lib, doc = null;
const ready = PDFiumLibrary.init({ wasmUrl }).then((l) => (lib = l));
onmessage = async (e) => {
  const m = e.data; await ready;
  try {
    if (m.type === 'open') {
      if (doc) doc.destroy();
      try { doc = await lib.loadDocument(new Uint8Array(await m.file.arrayBuffer()), m.password || undefined); }
      catch (err) { throw new Error(/password/i.test(String(err.message)) ? 'This PDF is password protected. Enter the password.' : 'This PDF could not be opened: ' + (err.message || err)); }
      postMessage({ id: m.id, pages: doc.getPageCount() });
    } else if (m.type === 'render') {
      const page = doc.getPage(m.index), t0 = performance.now();
      const r = await page.render({ scale: m.dpi / 72, render: 'bitmap' });
      const data = new Uint8Array(r.data); // RGBA
      postMessage({ id: m.id, w: r.width, h: r.height, data, ms: performance.now() - t0 }, [data.buffer]);
    }
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
