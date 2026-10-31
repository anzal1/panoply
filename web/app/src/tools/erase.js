import { tool, $, slider, bindSlider, baseName, esc } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import { MODELS, isCached } from '../ai/models.js';
const fill = aiWorker(() => new URL('./inpaint-worker.js', import.meta.url));
window.__erase = fill;
let bmp = null, strokes = [], cur = null, scale = 1, drawing = false;
const view = () => $('#paint'), ov = () => $('#paint').getContext('2d');
let maskCanvas = null;
function redraw() {
  const c = view(), g = ov(); g.clearRect(0, 0, c.width, c.height); g.drawImage(bmp, 0, 0, c.width, c.height);
  const m = maskCanvas.getContext('2d'); m.clearRect(0, 0, c.width, c.height); m.lineCap = m.lineJoin = 'round'; m.strokeStyle = m.fillStyle = '#fff';
  for (const s of strokes) { m.lineWidth = s.size; m.beginPath(); s.pts.forEach(([x, y], i) => (i ? m.lineTo(x, y) : m.moveTo(x, y))); if (s.pts.length === 1) { m.beginPath(); m.arc(s.pts[0][0], s.pts[0][1], s.size / 2, 0, 7); m.fill(); } else m.stroke(); }
  g.save(); g.globalAlpha = 0.5; g.drawImage(tint(), 0, 0); g.restore();
  $('#undo').disabled = !strokes.length; $('#clear').disabled = !strokes.length;
}
function tint() { const c = document.createElement('canvas'); c.width = maskCanvas.width; c.height = maskCanvas.height; const g = c.getContext('2d'); g.fillStyle = '#ff2d55'; g.fillRect(0, 0, c.width, c.height); g.globalCompositeOperation = 'destination-in'; g.drawImage(maskCanvas, 0, 0); return c; }
function pos(e) { const r = view().getBoundingClientRect(); return [(e.clientX - r.left) * view().width / r.width, (e.clientY - r.top) * view().height / r.height]; }
tool({
  id: 'erase', title: 'Erase objects', sub: 'Paint over what should go. The MI-GAN model fills it in, and every pixel outside the painted area stays exactly as it was.', cta: 'Erase',
  options: `<div id="editor" hidden><div class="paintwrap"><canvas id="paint" aria-label="Photo. Paint over what you want to remove." style="touch-action:none"></canvas></div>
    ${slider('brush', 'Brush size', 4, 120, 36)}
    <div class="trow" style="margin-top:12px"><button class="ic" id="undo" type="button" style="width:auto" disabled>Undo</button><button class="ic" id="clear" type="button" style="width:auto" disabled>Clear</button></div></div>
    <p class="hint" id="mnote">First use downloads the ${MODELS.migan.name} (${(MODELS.migan.size / 1e6).toFixed(0)} MB, ${MODELS.migan.license}) once and keeps it on this device.</p>`,
  onReady() {
    bindSlider('brush'); isCached('migan').then((c) => { if (c) $('#mnote').textContent = 'The model is already on this device. Nothing is uploaded.'; });
    const c = view();
    c.addEventListener('pointerdown', (e) => { if (!bmp) return; c.setPointerCapture(e.pointerId); drawing = true; cur = { size: +$('#brush').value * c.width / c.getBoundingClientRect().width, pts: [pos(e)] }; strokes.push(cur); redraw(); });
    c.addEventListener('pointermove', (e) => { if (!drawing) return; cur.pts.push(pos(e)); redraw(); });
    ['pointerup', 'pointercancel'].forEach((t) => c.addEventListener(t, () => { drawing = false; }));
    $('#undo').onclick = () => { strokes.pop(); redraw(); }; $('#clear').onclick = () => { strokes = []; redraw(); };
  },
  async onFile(f) {
    strokes = []; bmp = await createImageBitmap(f, { imageOrientation: 'from-image' }); const k = Math.min(1, 1200 / Math.max(bmp.width, bmp.height)), c = view();
    c.width = Math.max(1, Math.round(bmp.width * k)); c.height = Math.max(1, Math.round(bmp.height * k)); maskCanvas = document.createElement('canvas'); maskCanvas.width = c.width; maskCanvas.height = c.height; $('#editor').hidden = false; redraw();
  },
  async run(f) {
    if (!strokes.length) throw new Error('Paint over what you want to erase first.');
    const c = view(), md = maskCanvas.getContext('2d').getImageData(0, 0, c.width, c.height).data, mask = new Uint8Array(c.width * c.height); for (let i = 0; i < mask.length; i++) mask[i] = md[i * 4 + 3] > 127 ? 1 : 0;
    const btn = $('#go'); const r = await fill({ blob: f, mask, mw: c.width, mh: c.height, expand: 8 }, (p, cached) => { if (!cached) btn.innerHTML = `<span class="spin"></span>Downloading model ${Math.round(p * 100)}%`; }, (p) => { btn.innerHTML = `<span class="spin"></span>Filling ${Math.round(p * 100)}%`; });
    const fmt = f.type === 'image/jpeg' ? 'jpeg' : f.type === 'image/webp' ? 'webp' : 'png';
    const e = await process({ raw: { data: r.data, w: r.w, h: r.h }, out: { fmt, q: 92, level: 1, bg: [255, 255, 255], fast: true } });
    return { blob: new Blob([e.bytes], { type: e.type }), name: `${baseName(f.name)}-erased.${e.ext}`, delta: false, title: `${r.areas} area${r.areas === 1 ? '' : 's'} filled`, lines: [`${r.changed.toFixed(1)}% of the picture changed`, 'the rest is untouched', `fill ${(r.lap.fill / 1000).toFixed(1)} s`], hint: r.shrunk ? 'A large area was filled at reduced detail. Paint smaller areas for finer results.' : r.workShrunk ? `Worked at ${r.w} x ${r.h} to stay inside the memory limit (the photo is ${r.fw} x ${r.fh}).` : '' };
  },
});
