// Main-thread engine: presets, a worker pool, and a k-ary search over JPEG quality. Every probe is a real MozJPEG encode,
// so the bytes are identical to a sequential binary search; only the wall time changes.
import presets from './presets.json';

const N = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1));
const workers = Array.from({ length: N }, () => new Worker(new URL('./worker.js', import.meta.url), { type: 'module' }));
const pending = new Map(); let seq = 0, readyCount = 0, onReadyCb;
const ready = new Promise((r) => (onReadyCb = r));
workers.forEach((w) => (w.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'ready') { if (++readyCount === N) onReadyCb(); return; }
  const key = m.id ?? (m.type === 'error' ? { prep: 'prepped', img: 'imgok', shrink: 'shrunk' }[m.for] : m.type) + w.__tag, p = pending.get(key); if (!p) return;
  pending.delete(key); m.type === 'error' ? p.rej(new Error(m.message)) : p.res(m);
}));
workers.forEach((w, i) => (w.__tag = i));
const call = (i, msg, transfer = []) => new Promise((res, rej) => {
  const key = msg.type === 'enc' ? (msg.id = ++seq) : ({ prep: 'prepped', img: 'imgok', shrink: 'shrunk' }[msg.type]) + i;
  pending.set(key, { res, rej }); workers[i].postMessage(msg, transfer);
});

export function window_(p) {
  const f = (v, base) => Math.round(v * ({ MB: base * base, B: 1, BYTES: 1 }[(p.unit_claimed || 'KB').toUpperCase()] || base));
  const max = p.max_bytes_claimed || 0, min = p.min_bytes_claimed || 0, b = p.kb_basis;
  if (b === '1000') return [f(min, 1000), max ? f(max, 1000) : 0];
  if (b === '1024') return [f(min, 1024), max ? f(max, 1024) : 0];
  return [f(min, 1024), max ? f(max, 1000) : 0]; // unknown basis: inside the window under both readings
}
export function pixels(p) {
  if (p.width_px && p.height_px) return [p.width_px, p.height_px];
  if (p.physical_mm && p.dpi) return [Math.max(1, Math.round(p.physical_mm[0] / 25.4 * p.dpi)), Math.max(1, Math.round(p.physical_mm[1] / 25.4 * p.dpi))];
  return [0, 0];
}

function padComment(b, want) {
  let need = Math.max(0, want - b.length); const parts = [];
  while (need >= 4) { const n = Math.min(need - 4, 65533), seg = new Uint8Array(n + 4); seg[0] = 0xFF; seg[1] = 0xFE; seg[2] = (n + 2) >> 8; seg[3] = (n + 2) & 255; seg.fill(32, 4); parts.push(seg); need -= n + 4; }
  const out = new Uint8Array(b.length + parts.reduce((a, s) => a + s.length, 0)); out.set(b.subarray(0, 2), 0); let o = 2;
  for (const s of parts) { out.set(s, o); o += s.length; } out.set(b.subarray(2), o); return out;
}

const stats = { rounds: 0, probes: 0 };
// Largest q in 1..100 whose file is at most cap bytes, probing up to N qualities per round.
async function largest(sub, pr, cap, memo) {
  let lowFit = 0, high = 101;
  while (high - lowFit > 1) {
    const span = high - lowFit - 1, picks = new Set();
    for (let i = 1; i <= Math.min(N, span); i++) picks.add(lowFit + Math.max(1, Math.round((span * i) / (Math.min(N, span) + 1))));
    const todo = [...picks].filter((q) => !memo.has(q + ':' + sub + ':' + pr)); stats.rounds++; stats.probes += todo.length;
    const res = await Promise.all(todo.map((q, k) => call(k % N, { type: 'enc', q, sub, pr })));
    res.forEach((r) => memo.set(r.q + ':' + sub + ':' + pr, r.bytes));
    for (const q of [...picks].sort((a, b) => a - b)) { const b = memo.get(q + ':' + sub + ':' + pr); if (b.length <= cap) lowFit = Math.max(lowFit, q); else high = Math.min(high, q); }
  }
  return lowFit ? { q: lowFit, bytes: memo.get(lowFit + ':' + sub + ':' + pr) } : null;
}

async function solve(base, lo, hi, baselineOnly) {
  const cap = hi || Infinity, memo = new Map(); stats.rounds = stats.probes = 0;
  const copy = () => ({ type: 'img', w: base.w, h: base.h, data: base.data.slice() });
  await Promise.all(workers.map((_, i) => call(i, copy())));
  if (!hi && !lo) { const r = await call(0, { type: 'enc', q: 90, sub: true, pr: !baselineOnly }); return { q: 90, sub: true, b: r.bytes }; }
  let best = null, below = null;
  for (const sub of [true, false]) {
    const cands = [];
    for (const pr of baselineOnly ? [false] : [true, false]) {
      const r = await largest(sub, pr, cap, memo);
      if (r) { if (r.bytes.length >= lo) cands.push({ ...r, sub }); else if (!below || r.bytes.length > below.bytes.length) below = { ...r, sub }; }
    }
    best = cands.sort((x, y) => y.q - x.q || x.bytes.length - y.bytes.length)[0] || null;
    if (best) break;
  }
  if (best) return { q: best.q, sub: best.sub, b: best.bytes };
  if (below) { const margin = Math.min(64, hi > lo ? (hi - lo) >> 1 : 64), before = below.bytes.length; return { q: below.q, sub: below.sub, b: padComment(below.bytes, lo + margin), padded: lo + margin - before }; }
  return null;
}

export async function run(m) {
  await ready; const t0 = performance.now();
  let w, h, lo, hi, bo = true;
  if (m.preset) { const p = presets.find((x) => x.id === m.preset); [lo, hi] = window_(p); [w, h] = pixels(p); bo = p.baseline_only !== false; }
  else { w = m.w; h = m.h; lo = m.lo; hi = m.hi; }
  const prep = await call(0, { type: 'prep', blob: m.blob, w, h, noReduce: m.noReduce }), t1 = performance.now();
  let r = await solve(prep.base, lo, hi, bo), reducedFrom = null;
  const free = !(w && h);
  if (free && hi && (!r || r.q < 75)) { // quality first: a picture that only fits at a poor quality is made smaller instead, as on the desktop
    const sh = await call(0, { type: 'shrink', cap: hi, baseline: bo }).then((x) => x.shrunk);
    reducedFrom = sh.from; prep.base = { w: sh.w, h: sh.h, data: sh.data }; r = await solve(prep.base, lo, hi, bo);
  }
  const t2 = performance.now();
  if (!r) throw new Error(`At ${prep.base.w} x ${prep.base.h} px even the lowest quality is above ${hi.toLocaleString()} bytes. Try a plainer photo.`);
  const n = r.b.length;
  return { bytes: r.b, ms: t2 - t0, lap: { ...prep.lap, search: t2 - t1 }, heap: prep.heap, wasmMem: prep.wasmMem, stats: { ...stats }, workers: N,
    info: { width: prep.base.w, height: prep.base.h, quality: r.q, subsampled: r.sub, padded_shape: prep.padded, padded_by: r.padded || 0, lo, hi, in_window: n >= lo && (!hi || n <= hi), srcW: prep.srcW, srcH: prep.srcH, reduced: prep.reduced, reducedFrom } };
}
export { presets, ready };
