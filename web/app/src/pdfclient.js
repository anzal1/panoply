let w, seq = 0; const pend = new Map();
export function pdfJob(msg) {
  if (!w) { w = new Worker(new URL('./pdf-worker.js', import.meta.url), { type: 'module' }); w.onmessage = (e) => { const p = pend.get(e.data.id); pend.delete(e.data.id); e.data.error ? p.rej(new Error(e.data.error)) : p.res(e.data); }; }
  return new Promise((res, rej) => { const id = ++seq; pend.set(id, { res, rej }); w.postMessage({ ...msg, id }); });
}
