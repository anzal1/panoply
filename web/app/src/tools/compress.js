import { tool, $, slider, bindSlider, baseName, sameFormat } from '../shell.js';
import { process } from '../imgclient.js';
tool({
  id: 'compress', title: 'Compress photo', sub: 'Smaller files with the same pixels, using MozJPEG, oxipng and libwebp.', cta: 'Compress',
  options: `${slider('q', 'Quality (JPEG and WebP)', 30, 100, 75)}<p class="hint">PNG is always lossless: pixels do not change, only the file gets smaller. The format stays the same.</p>`,
  onReady() { bindSlider('q'); },
  async run(f) {
    const fmt = sameFormat(f);
    const r = await process({ blob: f, out: { fmt, q: +$('#q').value, level: 4 } });
    const keep = r.bytes.length >= f.size;
    return keep
      ? { blob: f, name: f.name, lines: [`${r.width} x ${r.height} px`], hint: 'This file is already as small as it gets at this quality. Lower the quality for a smaller one.', delta: true, preview: false }
      : { blob: new Blob([r.bytes], { type: r.type }), name: `${baseName(f.name)}-small.${r.ext}`, lines: [`${r.width} x ${r.height} px`, fmt.toUpperCase() + (fmt === 'png' ? ' lossless' : ` quality ${$('#q').value}`)], delta: true };
  },
});
