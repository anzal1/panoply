import { tool, fold, $, esc, baseName } from '../shell.js';
import { strip } from '../strip.js';
tool({
  id: 'strip-metadata', auto: true, title: 'Remove photo metadata', sub: 'Strip location, camera and edit history. The picture itself is not touched or re-encoded.', cta: 'Remove metadata',
  options: `<label class="chk"><input type="checkbox" id="icc" checked>Keep the colour profile (colours stay accurate)</label><p class="hint">The orientation is kept, so the photo does not turn sideways.</p>`,
  async run(f) {
    const r = strip(new Uint8Array(await f.arrayBuffer()), $('#icc').checked);
    const found = [...r.facts, ...r.removed.filter((x) => x !== 'EXIF' || !r.facts.length)];
    return { blob: new Blob([r.bytes], { type: r.type }), name: `${baseName(f.name)}-clean.${r.ext}`, lines: [`${(f.size - r.bytes.length).toLocaleString()} bytes removed`, r.removed.length ? `Removed ${r.removed.join(', ')}` : 'No metadata found'], html: found.length ? fold('What was removed', `<ul class="found">${found.map((x) => `<li>${esc(x)}</li>`).join('')}</ul>`) : '', hint: r.orientationKept ? 'Orientation kept.' : '', delta: false, preview: false };
  },
});
