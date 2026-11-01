import { tool, $, esc, baseName } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { MODELS, isCached } from '../ai/models.js';
const ocrCall = aiWorker(() => new URL('./ocr-worker.js', import.meta.url));
window.__ocr = ocrCall;
let last = '';
document.addEventListener('click', (e) => { const b = e.target.closest('[data-copy]'); if (!b) return; navigator.clipboard.writeText(last).then(() => { b.textContent = 'Copied'; setTimeout(() => (b.textContent = 'Copy text'), 1500); }); });
const need = ['ppocr_det', 'ppocr_en'];
tool({
  id: 'ocr', auto: true, title: 'Read text from a photo', sub: 'PP-OCRv5 runs on your device and finds every line of text, in reading order.', cta: 'Read the text',
  options: `<div class="field"><label for="script">Language</label><select id="script"><option value="en">English</option><option value="latin">French, German, Spanish, Portuguese, Italian and other Latin scripts</option><option value="devanagari">Hindi, Marathi, Nepali (Devanagari) and English</option></select></div>
    <label class="chk"><input type="checkbox" id="brk">Keep line breaks as they are in the picture</label>
    <p class="hint" id="mnote">First use downloads the text models once (about 16 MB for English, Apache-2.0) and keeps them on this device.</p>`,
  onReady() { Promise.all(need.map(isCached)).then((c) => { if (c.every(Boolean)) $('#mnote').textContent = 'The models are already on this device. Nothing is uploaded.'; }); },
  async run(f) {
    const btn = $('#go'); const r = await ocrCall({ blob: f, script: $('#script').value, keepBreaks: $('#brk').checked }, (p, cached) => { if (!cached) btn.innerHTML = `<span class="spin"></span>Downloading models ${Math.round(p * 100)}%`; });
    last = r.text; const words = r.page.lines.flatMap((l) => l.words), low = words.filter((w) => w.c < 0.6).length;
    if (!r.page.lines.length) return { blob: new Blob([''], { type: 'text/plain' }), name: baseName(f.name) + '.txt', title: 'No text found', bad: true, delta: false, preview: false, lines: [`${r.w} x ${r.h} px`], hint: 'Try a sharper photo, or another language.' };
    const ms = (k) => `${(r.lap[k] / 1000).toFixed(2)} s`;
    return { blob: new Blob([r.text], { type: 'text/plain' }), name: baseName(f.name) + '.txt', title: `${r.page.lines.length} lines, ${words.length} words`, delta: false, preview: false,
      lines: [`detect ${ms('detect')}`, `read ${ms('recognize')}`, low ? `${low} words are low confidence` : 'all words are confident'],
      html: `<textarea readonly rows="10" aria-label="Recognized text" style="width:100%;margin-top:12px;font:13px/18px ui-monospace,monospace;color:var(--ink);background:var(--field);border:1px solid var(--hair);border-radius:8px;padding:8px 12px;resize:vertical">${esc(r.text)}</textarea><button class="ic" data-copy style="width:auto;padding:0 12px;margin-top:8px">Copy text</button>` };
  },
});
