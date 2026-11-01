// Worker: the desktop page-building engine (panoply-pdf pageops, compiled to wasm) plus the job logic of merge, split and organize.
import init, { Pdfs, parse_ranges, label, compress, compress_report, ocr_pdf_plan, OcrPdf, ImagesPdf } from './pdfwasm/panoply_web_pdf.js';
import wasmUrl from './pdfwasm/panoply_web_pdf_bg.wasm';
const ready = init({ module_or_path: wasmUrl });

const natural = (a, b) => a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
const baseName = (n) => n.replace(/\.pdf$/i, '');
async function open(files) { const p = new Pdfs(); const counts = []; for (const f of files) counts.push(p.add(new Uint8Array(await f.arrayBuffer()), baseName(f.name))); return [p, counts]; }

const jobs = {
  async info({ files }) { const out = []; for (const f of files) { try { const [, c] = await open([f]); out.push({ name: f.name, pages: c[0] }); } catch (e) { out.push({ name: f.name, error: String(e.message || e) }); } } return { info: out }; },
  async merge({ files, opts }) {
    const [p, counts] = await open(files);
    const idx = (n) => Array.from({ length: n }, (_, i) => i);
    let pages = [];
    if (opts.interleave && files.length === 2) {
      const a = idx(counts[0]).map((i) => [0, i, 0]), b = idx(counts[1]).map((i) => [1, i, 0]); if (opts.reverseSecond) b.reverse();
      for (let i = 0; i < Math.max(a.length, b.length); i++) { if (a[i]) pages.push(a[i]); if (b[i]) pages.push(b[i]); }
    } else files.forEach((_, k) => { const l = idx(counts[k]).map((i) => [k, i, 0]); if (opts.reverseSecond && k === 1) l.reverse(); pages.push(...l); });
    const bytes = p.build(JSON.stringify({ mode: 'subset', bookmarks: !!opts.bookmarks, pages }));
    return { outputs: [{ name: 'merged.pdf', bytes }], report: JSON.parse(p.last_report()), counts };
  },
  async split({ files, opts }) {
    const [p, [n]] = await open(files); const outs = [], name = baseName(files[0].name);
    let groups; // [{label, pages: [indexes]}]
    if (opts.mode === 'ranges') { const r = JSON.parse(parse_ranges(opts.ranges || '', n)); groups = r.items; if (!groups.length) throw new Error('No pages matched those ranges.'); }
    else { const k = Math.max(1, opts.every | 0); groups = []; for (let s = 0; s < n; s += k) { const pg = Array.from({ length: Math.min(k, n - s) }, (_, i) => s + i); groups.push({ label: label(pg.map((x) => x)), pages: pg }); } }
    groups.forEach((g, i) => {
      const bytes = p.build(JSON.stringify({ mode: 'subset', pages: g.pages.map((x) => [0, x, 0]) }));
      const pat = (opts.pattern || '{name} {n}').replace('{name}', name).replace('{n}', String(i + 1)).replace('{pages}', g.label.replace(/[^\w.,-]+/g, ''));
      outs.push({ name: pat.replace(/[\\/:*?"<>|]/g, '_') + '.pdf', bytes });
    });
    return { outputs: outs, report: JSON.parse(p.last_report()), counts: [n] };
  },
  async ranges({ text, total }) { return { ranges: JSON.parse(parse_ranges(text || '', total)) }; },
  async compress({ files, opts }) {
    const bytes = compress(new Uint8Array(await files[0].arrayBuffer()), JSON.stringify(opts)); const report = JSON.parse(compress_report());
    return { outputs: [{ name: baseName(files[0].name) + '-small.pdf', bytes }], report };
  },
  async ocrplan({ files }) { return { plan: JSON.parse(ocr_pdf_plan(new Uint8Array(await files[0].arrayBuffer()))) }; },
  async ocrbuild({ files, pages, collectText }) {
    const o = new OcrPdf(new Uint8Array(await files[0].arrayBuffer()));
    for (const p of pages) o.add_page(p.i, p.imgW, JSON.stringify(p.page));
    const bytes = o.finish(!!collectText), report = JSON.parse(o.report()); o.free();
    return { outputs: [{ name: baseName(files[0].name) + '-searchable.pdf', bytes }], report };
  },
  async imagespdf({ imgs, page, margin }) {
    const p = new ImagesPdf(); for (const im of imgs) p.add(im.name, new Uint8Array(im.bytes));
    const bytes = p.build(page, margin); p.free(); return { outputs: [{ name: 'pictures.pdf', bytes }] };
  },
  async organize({ files, opts }) {
    const [p, [n]] = await open(files); const all = Array.from({ length: n }, (_, i) => i);
    const sel = (t) => (t && t.trim() ? JSON.parse(parse_ranges(t, n)).items.flatMap((i) => i.pages) : null);
    let order = sel(opts.keep) || all; const del = new Set(sel(opts.delete) || []); order = order.filter((i) => !del.has(i));
    if (opts.reverse) order = [...order].reverse(); if (!order.length) throw new Error('Nothing is left: every page was removed.');
    const rot = new Set(sel(opts.rotatePages) || all), deg = +opts.rotate || 0;
    const perm = order.length === n && new Set(order).size === n, changed = order.some((v, i) => v !== i);
    const bytes = p.build(JSON.stringify({ mode: perm ? 'full' : 'subset', drop_page_labels: perm && changed, pages: order.map((i) => [0, i, rot.has(i) ? deg : 0]) }));
    return { outputs: [{ name: baseName(files[0].name) + '-organized.pdf', bytes }], report: JSON.parse(p.last_report()), counts: [n] };
  },
};
onmessage = async (e) => {
  const { id, kind } = e.data; const t0 = performance.now();
  try { await ready; const r = await jobs[kind](e.data); postMessage({ id, ...r, ms: performance.now() - t0 }, (r.outputs || []).map((o) => o.bytes.buffer)); }
  catch (err) { postMessage({ id, error: String(err.message || err) }); }
};
