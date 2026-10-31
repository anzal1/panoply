import { tool, $, baseName, esc } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import * as eng from '../engine.js';
import presets from '../presets.json';
const idw = aiWorker(() => new URL('./id-worker.js', import.meta.url), 60000);
const sig = presets.filter((p) => p.field === 'signature');
const dims = (p) => (p.width_px && p.height_px ? [p.width_px, p.height_px] : p.physical_mm && p.dpi ? [Math.round(p.physical_mm[0] / 25.4 * p.dpi), Math.round(p.physical_mm[1] / 25.4 * p.dpi)] : [0, 0]);
tool({
  id: 'signature', title: 'Clean up a signature', sub: 'Turn a photo of your signature into a clean transparent PNG, or a white JPEG at the exact pixels and KB a form asks for.', cta: 'Clean the signature',
  options: `<div class="field"><label for="preset">Size</label><select id="preset"><option value="">Keep the signature's own size</option>${sig.map((p) => { const [w, h] = dims(p); return `<option value="${p.id}">${esc(p.authority.split(',')[0])}: ${w} x ${h} px${p.max_bytes_claimed ? ', ' + (p.min_bytes_claimed || 0) + ' to ' + p.max_bytes_claimed + ' KB' : ''}</option>`; }).join('')}</select></div>
    <div class="field"><label for="ink">Ink colour</label><select id="ink"><option value="keep">Keep the original</option><option value="black">Black</option><option value="blue">Blue</option></select></div>
    <div class="field"><label for="bg">Background</label><select id="bg"><option value="auto">Transparent PNG (white JPEG for a size preset with a KB limit)</option><option value="transparent">Transparent PNG</option><option value="white">White JPEG</option></select></div>
    <label class="chk"><input type="checkbox" id="lines">Remove ruled lines</label>
    <label class="chk"><input type="checkbox" id="thick" checked>Thicken thin strokes at small sizes</label>`,
  async run(f) {
    const p = sig.find((x) => x.id === $('#preset').value), [tw, th] = p ? dims(p) : [0, 0], [lo, hi] = p ? eng.window_(p) : [0, 0];
    const bgc = $('#bg').value, white = bgc === 'white' || (bgc === 'auto' && p && p.format === 'jpg') || (bgc === 'auto' && p && hi > 0);
    const r = await idw({ op: 'signature', blob: f, ink: $('#ink').value, sw: tw, sh: th, white, removeLines: $('#lines').checked, thicken: $('#thick').checked });
    let blob, name, lines = [`${r.w} x ${r.h} px`, `stroke about ${r.info.stroke_px.toFixed(1)} px`];
    if (white) {
      const png = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt: 'png', level: 1, fast: true } });
      const out = await eng.run({ blob: new Blob([png.bytes], { type: 'image/png' }), w: r.w, h: r.h, lo, hi });
      blob = new Blob([out.bytes], { type: 'image/jpeg' }); name = `${baseName(f.name)}-signature.jpg`; lines.push(hi ? `${out.info.in_window ? 'inside' : 'outside'} ${lo.toLocaleString()} to ${hi.toLocaleString()} bytes` : 'white background');
    } else {
      const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt: 'png', level: 3, bg: [255, 255, 255] } });
      blob = new Blob([e.bytes], { type: 'image/png' }); name = `${baseName(f.name)}-signature.png`; lines.push('transparent background');
    }
    if (r.info.lines_found) lines.push(`${r.info.lines_found} ruled line${r.info.lines_found === 1 ? '' : 's'} ${$('#lines').checked ? 'removed' : 'found, not removed'}`);
    return { blob, name, checker: !white, delta: false, title: `${(blob.size / 1000).toFixed(2)} KB`, lines, hint: r.info.notes.join('. ') };
  },
});
