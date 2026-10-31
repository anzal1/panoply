import { tool, $, slider, bindSlider, baseName, sameFormat } from '../shell.js';
import { process } from '../imgclient.js';
const sel = (id, label, opts) => `<div class="field"><label for="${id}">${label}</label><select id="${id}">${opts.map(([v, t]) => `<option value="${v}">${t}</option>`).join('')}</select></div>`;
tool({
  id: 'resize', title: 'Resize photo', sub: 'Set the pixel size or a percentage. Lanczos resampling, nothing uploaded.', cta: 'Resize',
  options: `<div class="trow" style="margin-top:0"><div><label for="w">Width (px)</label><input id="w" type="number" min="1" max="16000" inputmode="numeric" placeholder="Auto"></div><div><label for="h">Height (px)</label><input id="h" type="number" min="1" max="16000" inputmode="numeric" placeholder="Auto"></div></div>
    <p class="hint" id="dims">Leave one empty to keep the shape.</p>
    ${sel('fit', 'When both are set', [['inside', 'Fit inside, keep the whole photo'], ['cover', 'Fill and crop to the exact size'], ['stretch', 'Stretch to the exact size']])}
    ${sel('fmt', 'Save as', [['same', 'Same as the original'], ['jpeg', 'JPEG'], ['png', 'PNG'], ['webp', 'WebP']])}
    ${slider('q', 'Quality (JPEG and WebP)', 40, 100, 90)}`,
  onReady() { bindSlider('q'); },
  onFile(f) { createImageBitmap(f, { imageOrientation: 'from-image' }).then((b) => { $('#dims').textContent = `Original ${b.width} x ${b.height} px. Leave one empty to keep the shape.`; b.close(); }).catch(() => {}); },
  async run(f) {
    const fmt = $('#fmt').value === 'same' ? sameFormat(f) : $('#fmt').value;
    const w = parseInt($('#w').value) || 0, h = parseInt($('#h').value) || 0;
    if (!w && !h) throw new Error('Enter a width, a height, or both.');
    const r = await process({ blob: f, resize: { w, h, fit: $('#fit').value }, out: { fmt, q: +$('#q').value, level: 3 } });
    return { blob: new Blob([r.bytes], { type: r.type }), name: `${baseName(f.name)}-${r.width}x${r.height}.${r.ext}`, lines: [`${r.srcW} x ${r.srcH} to ${r.width} x ${r.height} px`, fmt.toUpperCase()], delta: true };
  },
});
