import { tool, $, esc, fmtBytes } from '../shell.js';
import { pdfJob } from '../pdfclient.js';
const natural = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
let files = [], pages = new Map(), setFiles = () => {};
function render() {
  $('#flist').innerHTML = files.map((f, i) => `<li><span class="n">${esc(f.name)}</span><span class="p">${pages.has(f) ? (pages.get(f) === null ? 'unreadable' : pages.get(f) + ' p') : '...'}, ${fmtBytes(f.size)}</span>
    <button class="ic" data-up="${i}" aria-label="Move ${esc(f.name)} up" ${i === 0 ? 'disabled' : ''}>&uarr;</button><button class="ic" data-down="${i}" aria-label="Move ${esc(f.name)} down" ${i === files.length - 1 ? 'disabled' : ''}>&darr;</button><button class="ic" data-rm="${i}" aria-label="Remove ${esc(f.name)}">&times;</button></li>`).join('');
  $('#two').hidden = files.length !== 2; setFiles(files);
}
tool({
  id: 'pdf-merge', title: 'Merge PDF files', sub: 'Put PDFs in one file. Bookmarks, links and forms survive. Nothing is uploaded.', cta: 'Merge', pdf: true, multiple: true, min: 2, accept: 'application/pdf',
  options: `<ol class="flist" id="flist" aria-label="Files to merge"></ol>
    <div class="field"><label for="order">Order</label><select id="order"><option value="natural">Natural (file2 before file10)</option><option value="given">As added</option></select></div>
    <label class="chk"><input type="checkbox" id="bm" checked>Bookmark each file</label>
    <div id="two" hidden><label class="chk"><input type="checkbox" id="inter">Interleave the two files (fronts and backs)</label><label class="chk"><input type="checkbox" id="rev">Reverse the second file</label></div>`,
  onReady() {
    $('#flist').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; const d = b.dataset;
      if (d.rm !== undefined) files.splice(+d.rm, 1); else { const i = +(d.up ?? d.down), j = d.up !== undefined ? i - 1 : i + 1; [files[i], files[j]] = [files[j], files[i]]; }
      render(); });
    $('#order').onchange = () => { if ($('#order').value === 'natural') files.sort(natural); render(); };
  },
  async onFile(all, set) {
    setFiles = set; files = all; if ($('#order').value === 'natural') files.sort(natural); render();
    const fresh = files.filter((f) => !pages.has(f)); if (!fresh.length) return;
    const r = await pdfJob({ kind: 'info', files: fresh }); r.info.forEach((x, k) => pages.set(fresh[k], x.error ? null : x.pages)); render();
  },
  async run(fs) {
    const r = await pdfJob({ kind: 'merge', files, opts: { bookmarks: $('#bm').checked, interleave: $('#inter').checked && files.length === 2, reverseSecond: $('#rev').checked && files.length === 2 } });
    const o = r.outputs[0], rep = r.report, notes = [];
    if (rep.links_removed) notes.push(`${rep.links_removed} links removed`); if (rep.fields_renamed) notes.push(`${rep.fields_renamed} form fields renamed`);
    return { blob: new Blob([o.bytes], { type: 'application/pdf' }), name: o.name, title: `${rep.pages} pages`, lines: [`${files.length} files`, rep.bookmarks ? `${rep.bookmarks} bookmarks` : 'no bookmarks', ...notes], delta: false };
  },
});
