import { tool, $, esc, baseName, fmtBytes, nav } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { renderCall } from '../renderclient.js';
import { pdfJob } from '../pdfclient.js';
import { isCached } from '../ai/models.js';
const ocrCall = aiWorker(() => new URL('./ocr-worker.js', import.meta.url));
let total = 0, opened = null;
async function open(f) { if (opened === f) return; const r = await renderCall({ type: 'open', file: f }); total = r.pages; opened = f; }
tool({
  id: 'ocr-pdf', title: 'Make a scanned PDF searchable', sub: 'Adds an invisible text layer so you can search and select the text. The pages themselves are copied untouched.', cta: 'Make searchable', pdf: true, accept: 'application/pdf',
  options: `<div class="field"><label for="script">Language</label><select id="script"><option value="en">English</option><option value="latin">French, German, Spanish, Portuguese, Italian and other Latin scripts</option><option value="devanagari">Hindi, Marathi, Nepali (Devanagari) and English</option></select></div>
    <div class="field"><label for="dpi">Reading resolution (DPI)</label><input id="dpi" type="number" min="150" max="400" value="300" inputmode="numeric"><p class="hint">300 suits printed text. Lower is faster.</p></div>
    <label class="chk"><input type="checkbox" id="txt">Also save the text as a file</label>
    <p class="hint" id="mnote">First use downloads the text models once (about 16 MB, Apache-2.0) and keeps them on this device. Pages that already have text are copied and not read.</p>`,
  onReady() { Promise.all(['ppocr_det', 'ppocr_en'].map(isCached)).then((c) => { if (c.every(Boolean)) $('#mnote').textContent = 'The models are already on this device. Pages that already have text are copied and not read. Nothing is uploaded.'; }); },
  async onFile(f) { opened = null; try { await open(f); $('#finfo').textContent = `${total} pages, ${fmtBytes(f.size)}`; } catch (e) { $('#finfo').textContent = e.message; } },
  async run(f) {
    await open(f); const btn = $('#go'), dpi = Math.min(400, Math.max(150, +$('#dpi').value || 300));
    const plan = (await pdfJob({ kind: 'ocrplan', files: [f] })).plan;
    if (!plan.needs.length) return { blob: f, name: f.name, title: 'Already searchable', delta: false, preview: false, lines: [`${plan.pages} pages all have text`], hint: 'Nothing was read or changed.' };
    const pages = []; let k = 0, words = 0;
    for (const n of plan.needs) {
      btn.innerHTML = `<span class="spin"></span>Reading page ${n.i + 1} (${++k} of ${plan.needs.length})`;
      const img = await renderCall({ type: 'render', index: n.i, dpi });
      const r = await ocrCall({ raw: { data: img.data, w: img.w, h: img.h }, script: $('#script').value }, (p, c) => { if (!c) btn.innerHTML = `<span class="spin"></span>Downloading models ${Math.round(p * 100)}%`; });
      words += r.page.lines.reduce((a, l) => a + l.words.length, 0); pages.push({ i: n.i, imgW: img.w, page: r.page });
    }
    btn.innerHTML = '<span class="spin"></span>Writing the PDF';
    const out = await pdfJob({ kind: 'ocrbuild', files: [f], pages, collectText: $('#txt').checked }), rep = out.report, o = out.outputs[0];
    const txtUrl = $('#txt').checked && rep.text ? URL.createObjectURL(new Blob([rep.text.replace(/\f/g, '\n\n')], { type: 'text/plain' })) : '';
    return { blob: new Blob([o.bytes], { type: 'application/pdf' }), name: o.name, title: `${rep.read} page${rep.read === 1 ? '' : 's'} read`, delta: true,
      lines: [`${rep.words} words`, `${Math.round(rep.mean_confidence * 100)}% confidence`, rep.had_text ? `${rep.had_text} pages already had text` : '', rep.flagged.length ? `check pages ${rep.flagged.map((x) => x + 1).join(', ')}` : '', rep.signed ? 'digital signatures are not kept' : ''].filter(Boolean),
      html: txtUrl ? `<a class="dl" href="${txtUrl}" download="${esc(baseName(f.name))}.txt" style="margin-top:12px">Download the text</a>` : '' };
  },
});
