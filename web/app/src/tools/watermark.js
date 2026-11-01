import { tool, fold, $, baseName, esc } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import { MODELS, isCached } from '../ai/models.js';
const wm = aiWorker(() => new URL('./watermark-worker.js', import.meta.url));
window.__wm = wm;
tool({
  id: 'remove-watermark', title: 'Remove a watermark from your own photo', sub: 'For pictures you own or may edit: a camera date stamp, a logo you added, a badge on your own image. Finds the mark, then fills it in.', cta: 'Find and remove',
  options: `<label class="chk"><input type="checkbox" id="own">These are my pictures, and I have the right to edit them</label>
    <div class="field"><label for="search">Where to look</label><select id="search"><option value="any">Anywhere in the picture</option><option value="edges">Only near an edge</option></select></div>
    <div class="field"><label for="fill">Fill with</label><select id="fill"><option value="auto">The MI-GAN model (best for detail)</option><option value="blur">A blur of the surroundings (flat areas)</option></select></div>
    <p class="hint" id="mnote">First use downloads the face detector (${(MODELS.yunet.size / 1e3).toFixed(0)} KB, ${MODELS.yunet.license}) and the ${MODELS.migan.name} (${(MODELS.migan.size / 1e6).toFixed(0)} MB, ${MODELS.migan.license}) once. Faces are never filled in. Invisible watermarks and signed provenance are not looked for or touched.</p>`,
  onReady() { Promise.all([isCached('yunet'), isCached('migan')]).then((c) => { if (c.every(Boolean)) $('#mnote').textContent = 'The models are already on this device. Faces are never filled in, and nothing is uploaded.'; }); },
  async run(f) {
    if (!$('#own').checked) throw new Error('Confirm that these are your pictures first.');
    const btn = $('#go'); const r = await wm({ blob: f, edges: $('#search').value === 'edges', fill: $('#fill').value, expand: 4 }, (p, c) => { if (!c) btn.innerHTML = `<span class="spin"></span>Downloading model ${Math.round(p * 100)}%`; }, (p) => { btn.innerHTML = `<span class="spin"></span>Filling ${Math.round(p * 100)}%`; });
    if (r.none) return { blob: f, name: f.name, title: 'Nothing found', delta: false, preview: false, bad: true, lines: [`${r.w} x ${r.h} px`], hint: r.spared ? `${r.spared} spot${r.spared === 1 ? '' : 's'} on a face ${r.spared === 1 ? 'was' : 'were'} left alone. Panoply never fills in a face.` : 'No watermark, logo or date stamp was found, so nothing changed. For a mark that is hard to find, try "Erase objects" and paint over it.' };
    const fmt = f.type === 'image/jpeg' ? 'jpeg' : f.type === 'image/webp' ? 'webp' : 'png';
    const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt, q: 92, level: 1, bg: [255, 255, 255], fast: true } });
    const list = `<ul class="found">${r.regions.map((x) => `<li>${esc(x.kind === 'stamp' ? 'date stamp' : x.kind)} at the ${esc(x.where)} (${Math.round(100 * x.w / r.w)}% x ${Math.round(100 * x.h / r.h)}% of the picture)</li>`).join('')}</ul>`;
    return { blob: new Blob([e.bytes], { type: e.type }), name: `${baseName(f.name)}-clean.${e.ext}`, delta: false, title: `${r.regions.length} mark${r.regions.length === 1 ? '' : 's'} removed`, html: fold("What was found", list), lines: [`${r.changed ? r.changed.toFixed(2) + '% of the picture changed' : 'blur fill'}`, 'pixels outside the mask are unchanged', r.faces ? `${r.faces} face${r.faces === 1 ? '' : 's'} protected` : ''].filter(Boolean), hint: r.spared ? `${r.spared} spot${r.spared === 1 ? '' : 's'} on a face ${r.spared === 1 ? 'was' : 'were'} left alone.` : '' };
  },
});
