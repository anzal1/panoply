import { tool, $, slider, bindSlider, baseName, fmtBytes } from '../shell.js';
import { renderCall } from '../renderclient.js';
import { process } from '../imgclient.js';
import { pdfJob } from '../pdfclient.js';
import { zip } from '../zip.js';
let total = 0, opened = null;
async function open(f, pw) { if (opened === f && !pw) return total; const r = await renderCall({ type: 'open', file: f, password: pw }); total = r.pages; opened = f; return total; }
tool({
  id: 'pdf-to-images', title: 'PDF to pictures', sub: 'Draw each page with pdfium, the engine Chrome uses, then save PNG or JPEG. Nothing is uploaded.', cta: 'Make pictures', pdf: true, accept: 'application/pdf',
  options: `<div class="trow" style="margin-top:0"><div><label for="fmt">Format</label><select id="fmt"><option value="png">PNG</option><option value="jpeg">JPEG</option></select></div><div><label for="dpi">Resolution (DPI)</label><input id="dpi" type="number" min="36" max="600" value="150" inputmode="numeric"></div></div>
    <div id="qw" hidden>${slider('q', 'JPEG quality', 1, 100, 90)}</div>
    <div class="field"><label for="pages">Pages</label><input id="pages" type="text" placeholder="All, or 1-3, 7, 9-" autocomplete="off" spellcheck="false"></div>
    <div class="field" id="pww" hidden><label for="pw">Password</label><input id="pw" type="text" autocomplete="off"></div>`,
  onReady() { bindSlider('q'); $('#fmt').onchange = () => ($('#qw').hidden = $('#fmt').value !== 'jpeg'); },
  async onFile(f) { opened = null; try { await open(f); $('#finfo').textContent = `${total} pages, ${fmtBytes(f.size)}`; } catch (e) { $('#finfo').textContent = e.message; if (/password/i.test(e.message)) $('#pww').hidden = false; } },
  async run(f) {
    await open(f, $('#pw').value || undefined);
    const dpi = Math.min(600, Math.max(36, +$('#dpi').value || 150)), fmt = $('#fmt').value, text = $('#pages').value.trim();
    let idx = Array.from({ length: total }, (_, i) => i);
    if (text) { const r = await pdfJob({ kind: 'ranges', text, total }); idx = r.ranges.items.flatMap((i) => i.pages); if (!idx.length) throw new Error('No pages matched.'); }
    const outs = [], t0 = performance.now(); let px = 0;
    for (const i of idx) {
      const r = await renderCall({ type: 'render', index: i, dpi }); px = r.w * r.h;
      const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt, q: +$('#q').value, level: 1, bg: [255, 255, 255] } });
      outs.push({ name: `${baseName(f.name)}-${String(i + 1).padStart(String(total).length, '0')}.${e.ext}`, bytes: e.bytes, w: r.w, h: r.h });
    }
    const one = outs.length === 1;
    return { blob: one ? new Blob([outs[0].bytes], { type: fmt === 'png' ? 'image/png' : 'image/jpeg' }) : zip(outs), name: one ? outs[0].name : baseName(f.name) + '-pages.zip', title: `${outs.length} picture${one ? '' : 's'}`, lines: [`${outs[0].w} x ${outs[0].h} px each`, `${dpi} DPI`, fmt.toUpperCase(), `${((performance.now() - t0) / outs.length).toFixed(0)} ms a page`], delta: false, preview: one, dlNote: one ? '' : 'zip' };
  },
});
