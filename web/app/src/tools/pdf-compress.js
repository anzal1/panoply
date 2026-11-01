import { tool, $, slider, bindSlider, fmtBytes } from '../shell.js';
import { pdfJob } from '../pdfclient.js';
tool({
  auto: true,
  id: 'pdf-compress', title: 'Compress a PDF', sub: 'Make a PDF small enough to email or upload. Pictures are recoded only while they still look the same.', cta: 'Compress', pdf: true, accept: 'application/pdf',
  options: `<div class="field"><label for="target">Target</label><select id="target"><option value="smallest">Smallest that still looks good</option><option value="quality">Fixed picture quality</option><option value="under">Under a size</option></select></div>
    <div id="qw" hidden>${slider('q', 'JPEG quality', 1, 100, 75)}</div>
    <div class="field" id="uw" hidden><label for="under">At most (MB)</label><input id="under" type="number" min="0.01" step="0.1" value="1" inputmode="decimal"></div>
    <div class="field"><label for="dpi">Picture resolution, DPI (0 = automatic)</label><input id="dpi" type="number" min="0" max="1200" value="0" inputmode="numeric"></div>
    <label class="chk"><input type="checkbox" id="gray">Turn colour pictures gray</label>
    <label class="chk"><input type="checkbox" id="meta">Remove document information and XMP</label>
    <label class="chk"><input type="checkbox" id="att">Remove embedded files</label>
    <label class="chk"><input type="checkbox" id="forms">Remove the interactive form</label>`,
  onReady() { bindSlider('q'); $('#target').onchange = () => { const t = $('#target').value; $('#qw').hidden = t !== 'quality'; $('#uw').hidden = t !== 'under'; }; },
  async onFile(f) { const r = await pdfJob({ kind: 'info', files: [f] }); const x = r.info[0]; $('#finfo').textContent = x.error ? x.error : `${x.pages} pages, ${fmtBytes(f.size)}`; },
  async run(f) {
    const t = $('#target').value;
    const opts = { target: t === 'quality' ? 'quality' : 'smallest', quality: +$('#q').value, under_bytes: t === 'under' ? Math.round(parseFloat($('#under').value) * 1e6) : 0, grayscale: $('#gray').checked, strip_metadata: $('#meta').checked, strip_attachments: $('#att').checked, strip_forms: $('#forms').checked, max_dpi: +$('#dpi').value || 0 };
    const r = await pdfJob({ kind: 'compress', files: [f], opts }); const o = r.outputs[0], rep = r.report;
    const lines = [rep.images ? `${rep.images_lossy} of ${rep.images} pictures recoded` : 'no pictures', rep.min_image_ssim != null ? `lowest picture SSIM ${rep.min_image_ssim.toFixed(3)}` : '', rep.duplicates_removed ? `${rep.duplicates_removed} duplicates removed` : '', rep.signatures ? 'digital signatures are not kept' : ''].filter(Boolean);
    const keep = o.bytes.length >= f.size * 0.98;
    return keep ? { blob: f, name: f.name, lines, hint: 'This PDF is already about as small as it gets without changing how it looks.', delta: true, preview: false }
      : { blob: new Blob([o.bytes], { type: 'application/pdf' }), name: o.name, lines, hint: rep.target_reached === false ? 'The size you asked for could not be reached without ruining the pictures.' : '', delta: true };
  },
});
