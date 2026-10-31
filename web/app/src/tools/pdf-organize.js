import { tool, $ } from '../shell.js';
import { pdfJob } from '../pdfclient.js';
let total = 0;
tool({
  id: 'pdf-organize', title: 'Organize PDF pages', sub: 'Reorder, delete, rotate or pull out pages. Bookmarks, forms and metadata stay when pages are only reordered.', cta: 'Apply', pdf: true, accept: 'application/pdf',
  options: `<div class="field"><label for="keep">Pages to keep, in the order you want</label><input id="keep" type="text" placeholder="5, 1-3, 9-" autocomplete="off" spellcheck="false"><p class="hint">Leave empty to keep every page. Words work: first, last, odd, even.</p></div>
    <div class="field"><label for="del">Pages to delete</label><input id="del" type="text" placeholder="2, 4-6" autocomplete="off" spellcheck="false"></div>
    <div class="trow"><div><label for="rot">Rotate (clockwise)</label><select id="rot"><option value="0">None</option><option value="90">90 degrees</option><option value="180">180 degrees</option><option value="270">270 degrees</option></select></div><div><label for="rp">Pages to rotate</label><input id="rp" type="text" placeholder="All" autocomplete="off" spellcheck="false"></div></div>
    <label class="chk"><input type="checkbox" id="rev">Reverse the order</label>`,
  async onFile(f) { const r = await pdfJob({ kind: 'info', files: [f] }); const x = r.info[0]; total = x.pages || 0; $('#finfo').textContent = x.error ? x.error : `${x.pages} pages`; },
  async run(f) {
    const r = await pdfJob({ kind: 'organize', files: [f], opts: { keep: $('#keep').value, delete: $('#del').value, rotate: $('#rot').value, rotatePages: $('#rp').value, reverse: $('#rev').checked } });
    const o = r.outputs[0];
    return { blob: new Blob([o.bytes], { type: 'application/pdf' }), name: o.name, title: `${r.report.pages} pages`, lines: [`from ${total} pages`, r.report.links_removed ? `${r.report.links_removed} links removed` : ''].filter(Boolean), delta: false };
  },
});
