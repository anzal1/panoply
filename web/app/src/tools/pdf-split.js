import { tool, $, esc } from '../shell.js';
import { pdfJob } from '../pdfclient.js';
import { zip } from '../zip.js';
let total = 0;
tool({
  id: 'pdf-split', title: 'Split a PDF', sub: 'Cut a PDF into parts by page ranges or every N pages. Nothing is uploaded.', cta: 'Split', pdf: true, accept: 'application/pdf',
  options: `<div class="field"><label for="mode">Split by</label><select id="mode"><option value="every_n">Every N pages</option><option value="ranges">Page ranges</option></select></div>
    <div class="field" id="evw"><label for="every">Pages per file</label><input id="every" type="number" min="1" value="1" inputmode="numeric"></div>
    <div class="field" id="rgw" hidden><label for="ranges">Ranges (each one is a file)</label><input id="ranges" type="text" placeholder="1-3, 7, 9-" autocomplete="off" spellcheck="false"><p class="hint">Words work too: first, last, odd, even.</p></div>
    <div class="field"><label for="pat">File names</label><input id="pat" type="text" value="{name} {n}" autocomplete="off"><p class="hint">{name} the PDF name, {n} the part, {pages} its pages.</p></div>`,
  onReady() { $('#mode').onchange = () => { const r = $('#mode').value === 'ranges'; $('#rgw').hidden = !r; $('#evw').hidden = r; }; },
  async onFile(f) { const r = await pdfJob({ kind: 'info', files: [f] }); const x = r.info[0]; total = x.pages || 0; $('#finfo').textContent = x.error ? x.error : `${x.pages} pages`; },
  async run(f) {
    const r = await pdfJob({ kind: 'split', files: [f], opts: { mode: $('#mode').value, every: +$('#every').value, ranges: $('#ranges').value, pattern: $('#pat').value } });
    const o = r.outputs; const one = o.length === 1;
    const used = new Set(); o.forEach((x) => { let n = x.name, k = 2; while (used.has(n)) n = x.name.replace(/\.pdf$/, ` (${k++}).pdf`); used.add(n); x.name = n; });
    return { blob: one ? new Blob([o[0].bytes], { type: 'application/pdf' }) : zip(o), name: one ? o[0].name : f.name.replace(/\.pdf$/i, '') + '-parts.zip', title: `${o.length} file${one ? '' : 's'}`, lines: [`from ${total} pages`, ...o.slice(0, 3).map((x) => x.name), o.length > 3 ? `and ${o.length - 3} more` : ''].filter(Boolean), delta: false, dlNote: one ? '' : 'zip' };
  },
});
