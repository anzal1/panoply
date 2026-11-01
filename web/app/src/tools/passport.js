import { tool, fold, $, baseName, esc } from '../shell.js';
import { aiWorker } from '../aiclient.js';
import { process } from '../imgclient.js';
import { idCall } from '../idclient.js';
import { jpegDensity } from '../density.js';
import * as eng from '../engine.js';
import presets from '../presets.json';
import { MODELS, isCached } from '../ai/models.js';
const pw = aiWorker(() => new URL('./passport-worker.js', import.meta.url), 60000);
window.__passport = pw;
let rules = [];
const ADVICE = { face: 'No clear face was found. Try a front-facing photo in good light.', head_size: 'The head is not the right size for this rule. Try a photo taken a bit closer or further away.', eye_line: 'The eyes sit too high or low for this rule. Try a photo with the head a little higher or lower in the frame.', centered: 'The face sits to one side of the picture. Try a photo with the face in the middle.', tilt: 'The head is tilted. Try a photo with the head level.', pose: 'The face is turned away. Try a photo looking straight at the camera.', eyes_open: 'The eyes look closed. Try a photo with both eyes open.', expression: 'The expression is not neutral. Try a photo with a relaxed face and a closed mouth.', background: 'The backdrop is not plain. Try a photo against a plain wall.', sharpness: 'The photo looks soft. Try a sharper one.', color: 'The rule asks for a colour photo.', resolution: 'The photo is small for this size. Try a larger original.', lighting: 'The light is uneven. Try a photo with even light on the face.', file: 'The file could not be fitted inside the size window.' };
const tint = { pass: 'tok', warn: '', fail: 'bad' };
function window_(r) {
  if (r.size_preset) { const p = presets.find((x) => x.id === r.size_preset); if (p) return eng.window_(p); }
  if (r.min_kb != null || r.max_kb != null) return eng.window_({ min_bytes_claimed: r.min_kb || 0, max_bytes_claimed: r.max_kb || 0, unit_claimed: 'KB', kb_basis: r.kb_basis || 'unknown' });
  return [0, 0];
}
tool({
  id: 'passport-photo', title: 'Passport and visa photo', sub: 'Crops, straightens and whitens your photo to the rules of a country, measures it against those rules, and lands the file inside the portal\'s size limit. Pass, warn or fail for every check.', cta: 'Make the photo',
  options: `<div class="field"><label for="rule">Country and document</label><select id="rule" disabled><option>Loading...</option></select><p class="hint" id="rhint"></p></div>
    <div class="field"><label for="face">Which face, if there is more than one</label><select id="face"><option value="0">Choose for me</option><option value="1">1, from the left</option><option value="2">2</option><option value="3">3</option><option value="4">4</option></select></div>
    <label class="chk"><input type="checkbox" id="whiten" checked>Set the background to the rule's colour</label>
    <label class="chk"><input type="checkbox" id="colorcut">Use only the plain backdrop cut (never the AI cut-out)</label>
    <label class="chk"><input type="checkbox" id="tilt" checked>Straighten the head</label>
    <label class="chk"><input type="checkbox" id="expo" checked>Correct a dark or bright face</label>
    <div class="field"><label for="out">Output</label><select id="out"><option value="single">The photo file</option><option value="both">The photo file and a print sheet</option></select></div>
    <div class="field" id="paperw" hidden><label for="paper">Sheet paper</label><select id="paper"><option value="4x6">4 x 6 in</option><option value="5x7">5 x 7 in</option><option value="a4">A4</option><option value="letter">Letter</option></select></div>
    <p class="hint" id="mnote">First use downloads the face detector (${(MODELS.yunet.size / 1e3).toFixed(0)} KB, ${MODELS.yunet.license}). If the backdrop is too busy for a plain cut, the ${(MODELS.isnet.size / 1e6).toFixed(0)} MB cut-out model is fetched once as well.</p>`,
  async onReady() {
    $('#out').onchange = () => ($('#paperw').hidden = $('#out').value === 'single');
    const r = await pw({ op: 'rules' }); rules = r.rules;
    $('#rule').innerHTML = rules.map((x) => `<option value="${x.id}">${esc(x.country_name || x.country)}: ${esc(x.document)} (${x.width_mm} x ${x.height_mm} mm)</option>`).join('');
    const want = document.querySelector('[data-rule]'), guess = (navigator.language || '').split('-')[1] || ''; const g = want ? rules.find((x) => x.id === want.dataset.rule) : rules.find((x) => x.country === guess.toUpperCase() && x.document === 'passport'); if (g) $('#rule').value = g.id;
    $('#rule').disabled = false;
    const upd = () => { const x = rules.find((y) => y.id === $('#rule').value); $('#rhint').textContent = x ? `${x.width_px ? x.width_px + ' x ' + x.height_px + ' px, ' : ''}${x.min_kb != null || x.max_kb != null || x.size_preset ? 'size limit applies, ' : ''}background ${x.background}. ${x.status === 'conflict' ? 'Sources disagree on this rule; the stricter reading is used.' : x.status === 'stale' ? 'This rule may be out of date. Check the authority.' : ''}` : ''; }; $('#rule').onchange = upd; upd();
    Promise.all([isCached('yunet')]).then((c) => { if (c.every(Boolean)) $('#mnote').textContent = 'The face model is already on this device. Nothing is uploaded.'; });
  },
  async run(f) {
    const rule = rules.find((x) => x.id === $('#rule').value), btn = $('#go'), both = $('#out').value === 'both';
    const dpi = rule.dpi || 300;
    const [w, h] = rule.width_px && rule.height_px ? [rule.width_px, rule.height_px] : [Math.round(rule.width_mm / 25.4 * dpi), Math.round(rule.height_mm / 25.4 * dpi)];
    const [pwid, phei] = [Math.round(rule.width_mm / 25.4 * dpi), Math.round(rule.height_mm / 25.4 * dpi)];
    const sizes = both && (pwid !== w || phei !== h) ? [[w, h], [pwid, phei]] : [[w, h]];
    const r = await pw({ blob: f, rule: rule.id, face: +$('#face').value, whiten: $('#whiten').checked, colorCutout: $('#colorcut').checked, fixTilt: $('#tilt').checked, fixExposure: $('#expo').checked, sizes }, (p, c) => { if (!c) btn.innerHTML = `<span class="spin"></span>Downloading model ${Math.round(p * 100)}%`; });
    btn.innerHTML = '<span class="spin"></span>Fitting the file size';
    const main = r.renders[0], rep = main.report, [lo, hi] = window_(rule), effDpi = Math.max(72, Math.min(1200, Math.round(main.h / rule.height_mm * 25.4)));
    const png = await process({ raw: { data: main.data, w: main.w, h: main.h }, out: { fmt: 'png', level: 1, fast: true } });
    const out = await eng.run({ blob: new Blob([png.bytes], { type: 'image/png' }), w: main.w, h: main.h, lo, hi });
    const bytes = jpegDensity(out.bytes, effDpi);
    rep.checks.push({ id: 'file', label: 'File', status: out.info.in_window ? 'pass' : 'fail', detail: hi ? `${bytes.length.toLocaleString()} bytes, window ${lo.toLocaleString()} to ${hi.toLocaleString()}` : `${main.w} x ${main.h} px, ${bytes.length.toLocaleString()} bytes` });
    const overall = rep.checks.some((c) => c.status === 'fail') ? 'fail' : rep.checks.some((c) => c.status === 'warn') ? 'warn' : 'pass';
    let sheetHtml = '';
    if (both) {
      const print = r.renders[r.renders.length - 1], sh = await idCall({ op: 'sheet', photos: [{ data: print.data, w: print.w, h: print.h }], paper: $('#paper').value, cw: rule.width_mm, ch: rule.height_mm, margin: 3, gutter: 2, copies: 0, cutMarks: true, rotate: true, dpi });
      const j = await process({ raw: { data: sh.data, w: sh.w, h: sh.h }, out: { fmt: 'jpeg', q: 95, bg: [255, 255, 255] } });
      sheetHtml = `<a class="dl" style="margin-top:12px" download="${esc(baseName(f.name))}-${rule.id}-sheet.jpg" href="${URL.createObjectURL(new Blob([jpegDensity(j.bytes, dpi)], { type: 'image/jpeg' }))}">Download the print sheet (${sh.info.photos} photos)</a>`;
    }
    const list = `<ul class="found" style="list-style:none;padding:0;margin-top:12px">${rep.checks.map((c) => `<li style="margin-top:6px"><b class="${tint[c.status]}">${{ pass: 'OK', warn: 'Check', fail: 'Fix' }[c.status] || esc(c.status)}</b> ${esc(c.label)}: ${esc(c.detail)}${c.fixed ? ' (fixed)' : ''}</li>`).join('')}</ul>${rep.unchecked.length ? `<p class="hint">Not checked, please look yourself: ${esc(rep.unchecked.join(', '))}</p>` : ''}<p class="hint">This meets the published rule as read here. It does not promise the authority accepts the photo. Source: ${esc(rule.authority || rule.source_url)}.</p>`;
    return { blob: new Blob([bytes], { type: 'image/jpeg' }), name: `${baseName(f.name)}-${rule.id}-${main.w}x${main.h}.jpg`, delta: false, bad: rep.checks.some((c) => c.id === 'file' && c.status === 'fail'), title: '', lines: [`${main.w} x ${main.h} px`, rule.name, r.usedModel ? 'AI cut-out used' : 'plain backdrop cut', `${(r.lap.faces / 1000).toFixed(1)} s to find the face`], html: (() => { const probs = rep.checks.filter((c) => c.status !== 'pass' && ADVICE[c.id]); return probs.length ? `<p class="advice"><b>${overall === 'fail' ? 'Needs a small fix.' : 'Nearly there.'}</b> ${esc(probs.slice(0, 2).map((c) => ADVICE[c.id]).join(' '))} The file is still ready to download.</p>` : '<p class="advice">This meets the rule as read here.</p>'; })() + sheetHtml + fold(`${rep.checks.filter((c) => c.status === 'pass').length} of ${rep.checks.length} checks OK`, list) };
  },
});
