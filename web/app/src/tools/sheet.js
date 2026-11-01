import { tool, $, baseName, esc } from '../shell.js';
import { process } from '../imgclient.js';
import { idCall } from '../idclient.js';
import { pdfJob } from '../pdfclient.js';
import { jpegDensity, pngDensity } from '../density.js';
const SIZES = [['35x45', '35 x 45 mm (India, UK, EU, Australia)'], ['50.8x50.8', '2 x 2 in (US passport and visa)'], ['33x48', '33 x 48 mm (China)'], ['25x35', '25 x 35 mm'], ['40x60', '40 x 60 mm'], ['custom', 'Custom size']];
tool({
  id: 'print-sheet', title: 'Passport photo print sheet', sub: 'Fits as many copies of your photo as the paper holds, with gaps and cut marks, at the exact print size. Crop the photo to the right shape first.', cta: 'Make the sheet',
  options: `<div class="trow" style="margin-top:0"><div><label for="paper">Paper</label><select id="paper"><option value="4x6">4 x 6 in</option><option value="5x7">5 x 7 in</option><option value="6x8">6 x 8 in</option><option value="a4">A4</option><option value="a5">A5</option><option value="a6">A6</option><option value="letter">Letter</option></select></div>
    <div><label for="size">Photo size</label><select id="size">${SIZES.map(([v, t]) => `<option value="${v}">${esc(t)}</option>`).join('')}</select></div></div>
    <div class="trow" id="cust" hidden><div><label for="cw">Width (mm)</label><input id="cw" type="number" min="10" max="300" step="0.1" value="35" inputmode="decimal"></div><div><label for="ch">Height (mm)</label><input id="ch" type="number" min="10" max="300" step="0.1" value="45" inputmode="decimal"></div></div>
    <div class="trow"><div><label for="margin">Margin (mm)</label><input id="margin" type="number" min="0" max="30" step="0.5" value="3" inputmode="decimal"></div><div><label for="gutter">Gap (mm)</label><input id="gutter" type="number" min="0" max="20" step="0.5" value="2" inputmode="decimal"></div></div>
    <div class="trow"><div><label for="copies">Copies (empty fills the sheet)</label><input id="copies" type="number" min="1" max="100" inputmode="numeric" placeholder="Fill"></div><div><label for="fmt">Save as</label><select id="fmt"><option value="jpg">JPEG</option><option value="png">PNG</option><option value="pdf">PDF</option></select></div></div>
    <label class="chk"><input type="checkbox" id="cut" checked>Cut marks</label><label class="chk"><input type="checkbox" id="rot" checked>Turn photos to fit more on the sheet</label>`,
  onReady() { $('#size').onchange = () => ($('#cust').hidden = $('#size').value !== 'custom'); },
  async run(f) {
    const [cw, ch] = $('#size').value === 'custom' ? [+$('#cw').value, +$('#ch').value] : $('#size').value.split('x').map(Number), dpi = 300;
    const bmp = await createImageBitmap(f, { imageOrientation: 'from-image' }), k = Math.min(1, 6000 / Math.max(bmp.width, bmp.height)), w = Math.round(bmp.width * k), h = Math.round(bmp.height * k);
    const c = new OffscreenCanvas(w, h), g = c.getContext('2d', { willReadFrequently: true }); g.fillStyle = '#fff'; g.fillRect(0, 0, w, h); g.drawImage(bmp, 0, 0, w, h); bmp.close(); const data = new Uint8Array(g.getImageData(0, 0, w, h).data.buffer);
    const r = await idCall({ op: 'sheet', photos: [{ data, w, h }], paper: $('#paper').value, cw, ch, margin: +$('#margin').value, gutter: +$('#gutter').value, copies: +$('#copies').value || 0, cutMarks: $('#cut').checked, rotate: $('#rot').checked, dpi }, [data.buffer]);
    const fmt = $('#fmt').value, jpg = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt: 'jpeg', q: 95, bg: [255, 255, 255] } });
    let blob, ext;
    if (fmt === 'png') { const e = await process({ raw: { data: r.data.buffer.slice(0), w: r.w, h: r.h }, out: { fmt: 'png', level: 1, fast: false } }).catch(() => null); blob = new Blob([pngDensity(e.bytes, dpi)], { type: 'image/png' }); ext = 'png'; }
    else if (fmt === 'pdf') { const o = await pdfJob({ kind: 'imagespdf', imgs: [{ name: 'sheet.jpg', bytes: jpegDensity(jpg.bytes, dpi).buffer }], page: 'image', margin: 0 }); blob = new Blob([o.outputs[0].bytes], { type: 'application/pdf' }); ext = 'pdf'; }
    else { blob = new Blob([jpegDensity(jpg.bytes, dpi)], { type: 'image/jpeg' }); ext = 'jpg'; }
    const i = r.info;
    return { blob, name: `${baseName(f.name)}-sheet.${ext}`, delta: false, preview: ext !== 'pdf', title: `${i.photos} photos on ${i.paper}`, lines: [`${i.cell_mm[0]} x ${i.cell_mm[1]} mm each`, `${dpi} DPI`, `size error under ${Math.max(0.01, i.size_error_mm).toFixed(2)} mm`], hint: (i.notes || []).join('. ') };
  },
});
