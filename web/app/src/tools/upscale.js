import { tool, $, baseName } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import { MODELS, isCached } from '../ai/models.js';
const up = aiWorker(() => new URL('./upscale-worker.js', import.meta.url));
window.__upscale = up;
tool({
  id: 'upscale', title: 'Upscale a photo', sub: 'Real-ESRGAN makes small or soft pictures 2 or 4 times bigger and sharper, tile by tile with no seams.', cta: 'Upscale',
  options: `<div class="field"><label for="scale">Scale</label><select id="scale"><option value="0">Automatic (4x for small pictures, 2x otherwise)</option><option value="2">2x</option><option value="4">4x</option></select></div>
    <div class="field"><label for="fmt">Save as</label><select id="fmt"><option value="auto">Same as the original (PNG for anything else)</option><option value="png">PNG</option><option value="jpeg">JPEG</option></select></div>
    <p class="hint" id="mnote">First use downloads the ${MODELS.esrgan.name} (${(MODELS.esrgan.size / 1e6).toFixed(0)} MB, ${MODELS.esrgan.license}) once and keeps it on this device.</p>`,
  onReady() { isCached('esrgan').then((c) => { if (c) $('#mnote').textContent = 'The model is already on this device. Nothing is uploaded.'; }); },
  async run(f) {
    const btn = $('#go'); const r = await up({ blob: f, scale: +$('#scale').value || 0 }, (p, c) => { if (!c) btn.innerHTML = `<span class="spin"></span>Downloading model ${Math.round(p * 100)}%`; }, (p) => { btn.innerHTML = `<span class="spin"></span>Upscaling ${Math.round(p * 100)}%`; });
    let fmt = $('#fmt').value; if (fmt === 'auto') fmt = f.type === 'image/jpeg' ? 'jpeg' : 'png'; if (r.hasAlpha) fmt = 'png';
    const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt, q: 92, level: 1, bg: [255, 255, 255], fast: true } });
    return { blob: new Blob([e.bytes], { type: e.type }), name: `${baseName(f.name)}-${r.scale}x.${e.ext}`, delta: false, title: `${r.w} x ${r.h} px`, lines: [`from ${r.srcW} x ${r.srcH}`, `${r.scale}x`, `${r.tiles} tiles in ${(r.lap.tiles / 1000).toFixed(1)} s`, r.hasAlpha ? 'transparency kept' : ''].filter(Boolean) };
  },
});
