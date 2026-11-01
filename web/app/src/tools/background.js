import { tool, $, slider, bindSlider, baseName } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import { MODELS, isCached } from '../ai/models.js';
const cutout = aiWorker(() => new URL('./vision-worker.js', import.meta.url));
window.__cutout = cutout;
tool({
  id: 'remove-background', auto: true, title: 'Remove a photo background', sub: 'Runs on your device, nothing is uploaded. Hair and fur stay soft.', cta: 'Remove background',
  options: `<div class="field"><label for="look">Result</label><select id="look"><option value="transparent">Transparent</option><option value="color">Solid colour</option><option value="blur">Blurred original</option></select></div>
    <div class="field" id="bgw" hidden><label for="bg">Background colour</label><input id="bg" type="color" value="#ffffff" ></div>
    <div id="blw" hidden>${slider('blur', 'Blur strength', 1, 100, 30)}</div>
    <label class="chk"><input type="checkbox" id="refine" checked>Refine edges at full resolution</label>
    <label class="chk"><input type="checkbox" id="decon" checked>Remove the old background colour from the edge</label>
    <p class="hint" id="mnote">First use downloads the ${MODELS.isnet.name} (${(MODELS.isnet.size / 1e6).toFixed(0)} MB, ${MODELS.isnet.license}) once and keeps it on this device.</p>`,
  onReady() { bindSlider('blur'); $('#look').onchange = () => { const l = $('#look').value; $('#bgw').hidden = l !== 'color'; $('#blw').hidden = l !== 'blur'; }; isCached('isnet').then((c) => { if (c) $('#mnote').textContent = 'The model is already on this device. Nothing is uploaded.'; }); },
  async run(f) {
    const look = $('#look').value, hex = $('#bg').value, bg = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
    const btn = $('#go'); const side = (navigator.deviceMemory || 8) <= 4 ? 768 : 1024; const r = await cutout({ blob: f, key: `${f.name}:${f.size}:${f.lastModified}`, side, look, bg, blur: +$('#blur').value, refine: $('#refine').checked, decontaminate: $('#decon').checked }, (p, cached) => { if (!cached) btn.innerHTML = `<span class="spin"></span>Downloading model ${Math.round(p * 100)}%`; });
    btn.innerHTML = '<span class="spin"></span>Working';
    const fmt = look === 'transparent' ? 'png' : 'jpeg';
    const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt, q: 92, level: 1, bg: bg, fast: true } });
    const lap = r.lap, ms = (k) => `${(lap[k] / 1000).toFixed(2)} s`;
    return { blob: new Blob([e.bytes], { type: e.type }), name: `${baseName(f.name)}-${look === 'transparent' ? 'cutout' : 'new-bg'}.${e.ext}`, checker: look === 'transparent', single: true, lines: [`${r.w} x ${r.h} px`, `model run ${ms('infer')} (${r.ep})`, `edges ${ms('refine+compose')}`, lap.model > 800 ? `model ready in ${ms('model')}` : ''].filter(Boolean), hint: side < 1024 ? 'This device reports little memory, so the model ran at a smaller size.' : r.shrunk ? `Worked at ${r.w} x ${r.h} to stay inside the memory limit (the photo is ${r.fw} x ${r.fh}).` : '', delta: false };
  },
});
