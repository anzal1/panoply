export function aiWorker(url, idleMs = 45000) { // like the desktop, the model is unloaded after a while: terminating the worker returns all of its wasm memory
  let w, seq = 0, timer; const pend = new Map();
  return (msg, onProgress, onWork) => {
    clearTimeout(timer);
    if (!w) { w = new Worker(url(), { type: 'module' }); w.onmessage = (e) => { const p = pend.get(e.data.id); if (!p) return; if (e.data.progress !== undefined) return p.onProgress && p.onProgress(e.data.progress, e.data.cached); if (e.data.progress2 !== undefined) return p.onWork && p.onWork(e.data.progress2); pend.delete(e.data.id); if (!pend.size) timer = setTimeout(() => { w.terminate(); w = null; }, idleMs); e.data.error ? p.rej(new Error(e.data.error)) : p.res(e.data); }; }
    return new Promise((res, rej) => { const id = ++seq; pend.set(id, { res, rej, onProgress, onWork }); w.postMessage({ ...msg, id }); });
  };
}
