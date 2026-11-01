import { tool, $, esc, fmtBytes } from '../shell.js';
import { pdfJob } from '../pdfclient.js';
import { process } from '../imgclient.js';
let files = [], setFiles = () => {};
function render() {
  $('#flist').innerHTML = files.map((f, i) => `<li><span class="n">${esc(f.name)}</span><span class="p">${fmtBytes(f.size)}</span><button class="ic" data-up="${i}" aria-label="Move ${esc(f.name)} up" ${i === 0 ? 'disabled' : ''}>&uarr;</button><button class="ic" data-down="${i}" aria-label="Move ${esc(f.name)} down" ${i === files.length - 1 ? 'disabled' : ''}>&darr;</button><button class="ic" data-rm="${i}" aria-label="Remove ${esc(f.name)}">&times;</button></li>`).join(''); setFiles(files);
}
tool({
  id: 'images-to-pdf', title: 'Pictures to PDF', sub: 'One page for each picture, in the order you choose. JPEG and PNG go in as they are, other formats are converted first.', cta: 'Make the PDF', multiple: true, min: 1, accept: 'image/*',
  options: `<ol class="flist" id="flist" aria-label="Pictures"></ol>
    <div class="trow" style="margin-top:0"><div><label for="page">Page size</label><select id="page"><option value="image">As big as the picture</option><option value="a4">A4</option><option value="letter">Letter</option></select></div><div><label for="margin">Margin (mm)</label><input id="margin" type="number" min="0" max="40" value="10" inputmode="decimal"></div></div>`,
  onReady() { $('#flist').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; const d = b.dataset; if (d.rm !== undefined) files.splice(+d.rm, 1); else { const i = +(d.up ?? d.down), j = d.up !== undefined ? i - 1 : i + 1; [files[i], files[j]] = [files[j], files[i]]; } render(); }); },
  onFile(all, set) { setFiles = set; files = all; render(); },
  async run() {
    const imgs = [];
    for (const f of files) {
      const buf = new Uint8Array(await f.arrayBuffer()), jpeg = buf[0] === 0xFF && buf[1] === 0xD8, png = buf[0] === 0x89 && buf[1] === 0x50;
      if (jpeg || png) imgs.push({ name: f.name, bytes: buf.buffer });
      else { const e = await process({ blob: f, out: { fmt: 'png', level: 1, fast: true, bg: [255, 255, 255] } }); imgs.push({ name: f.name.replace(/\.[^.]+$/, '.png'), bytes: e.bytes.buffer }); }
    }
    const o = await pdfJob({ kind: 'imagespdf', imgs, page: $('#page').value, margin: +$('#margin').value || 0 });
    return { blob: new Blob([o.outputs[0].bytes], { type: 'application/pdf' }), name: 'pictures.pdf', delta: false, preview: false, title: `${imgs.length} page${imgs.length === 1 ? '' : 's'}`, lines: [$('#page').value === 'image' ? 'each page as big as its picture' : $('#page').value.toUpperCase()] };
  },
});
