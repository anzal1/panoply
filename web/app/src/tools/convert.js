import { tool, $, slider, bindSlider, baseName } from '../shell.js';
import { process } from '../imgclient.js';
tool({
  id: 'convert', title: 'Convert photo format', sub: 'JPEG, PNG, WebP or AVIF, encoded with the real codecs in your browser.', cta: 'Convert',
  options: `<div class="field"><label for="fmt">Convert to</label><select id="fmt"><option value="jpeg">JPEG</option><option value="png">PNG (lossless)</option><option value="webp">WebP</option><option value="avif">AVIF (slower, smallest)</option></select></div>
    ${slider('q', 'Quality (JPEG, WebP, AVIF)', 30, 100, 80)}
    <div class="field"><label for="bg">Background for transparent areas (JPEG)</label><input id="bg" type="color" value="#ffffff" ></div>`,
  onReady() { bindSlider('q'); },
  async run(f) {
    const fmt = $('#fmt').value, bg = $('#bg').value, bgc = [1, 3, 5].map((i) => parseInt(bg.slice(i, i + 2), 16));
    const r = await process({ blob: f, out: { fmt, q: +$('#q').value, level: 3, bg: bgc, speed: 6 } });
    return { blob: new Blob([r.bytes], { type: r.type }), name: `${baseName(f.name)}.${r.ext}`, lines: [`${r.width} x ${r.height} px`, fmt.toUpperCase()], hint: fmt === 'jpeg' && r.alpha ? 'Transparent areas were filled with the background colour.' : '', delta: true };
  },
});
