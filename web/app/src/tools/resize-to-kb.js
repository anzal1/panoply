// Resize to exact KB: the preset list, the worker pool search and the result card (the page markup comes from the site).
import { run, presets, ready } from '../engine.js';
const $ = (s) => document.querySelector(s);
let file = null, srcUrl = null, outUrl = null, busy = false;
window.__bench = {};
const CUSTOM = '__custom';
const fmt = (n) => n.toLocaleString('en-US');
const esc = (s) => String(s).replace(/[<>&"]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c]));
const spec = (p) => `${p.width_px ? `${p.width_px} x ${p.height_px} px` : p.physical_mm ? `${p.physical_mm[0]} x ${p.physical_mm[1]} mm` : 'Any size'}, ${p.max_bytes_claimed ? `${p.min_bytes_claimed || 0} to ${p.max_bytes_claimed} KB` : 'no size cap'}`;
function updatePreset() {
  const id = $('#preset').value, p = presets.find((x) => x.id === id);
  $('#custom').hidden = id !== CUSTOM; const h = $('#presetHint'); h.className = 'hint';
  const st = p ? { stale: 'This rule may be out of date. Check the portal.', conflict: 'Sources disagree on this rule. Check the portal.' }[p.status] || '' : '';
  const rule = p ? `${spec(p)}. ` : '';
  h.textContent = rule + st; if (st) h.classList.add('warn');
}
function enable() { $('#go').disabled = busy || !file; }
const groups = {}; presets.forEach((p) => (groups[p.authority.split(',')[0]] ||= []).push(p));
$('#preset').innerHTML = Object.entries(groups).map(([g, ps]) => `<optgroup label="${esc(g)}">${ps.map((p) => {
  return `<option value="${p.id}">${esc(p.field[0].toUpperCase() + p.field.slice(1))}: ${esc(p.form.length > 46 ? p.form.slice(0, 44) + '...' : p.form)}</option>`;
}).join('')}</optgroup>`).join('') + `<option value="${CUSTOM}">Custom size and KB</option>`;
$('#preset').disabled = true; updatePreset();
ready.then(() => { $('#preset').disabled = false; const want = document.querySelector('[data-preset]'); if (want && presets.some((x) => x.id === want.dataset.preset)) { $('#preset').value = want.dataset.preset; updatePreset(); } window.__bench.ready = performance.now(); enable(); });
function show(m) {
  if (outUrl) URL.revokeObjectURL(outUrl);
  outUrl = URL.createObjectURL(new Blob([m.bytes], { type: 'image/jpeg' }));
  const i = m.info, n = m.bytes.length, max = i.hi || Math.max(n * 1.2, i.lo * 1.2), pct = (v) => Math.min(100, (v / max) * 100);
  const name = $('#preset').value === CUSTOM ? 'custom' : $('#preset').value;
  $('#result').innerHTML = `<div class="card"><div class="big ${i.in_window ? 'tok' : 'bad'}">${fmt(n)} bytes <span class="kb">(${(n / 1000).toFixed(2)} KB)</span></div>
    <div class="gauge" role="img" aria-label="${i.in_window ? 'Inside' : 'Outside'} the allowed size"><i style="left:${pct(i.lo)}%;right:${100 - pct(i.hi || max)}%"></i><b style="left:calc(${pct(n)}% - 1px)"></b></div>
    <div class="meta"><span>Allowed ${i.lo ? fmt(i.lo) : '0'} to ${i.hi ? fmt(i.hi) : 'no limit'} bytes</span><span>${i.width} x ${i.height} px</span><span>Quality ${i.quality}</span><span>${(m.ms / 1000).toFixed(2)} s</span></div>
    ${i.reducedFrom ? `<p class="hint">The picture was reduced from ${i.reducedFrom[0]} x ${i.reducedFrom[1]} to ${i.width} x ${i.height} pixels so a good quality fits ${fmt(i.hi)} bytes.</p>` : ''}${i.padded_shape ? '<p class="hint">The photo is a different shape than the target, so it was padded with white instead of cropped.</p>' : ''}${i.padded_by ? `<p class="hint">The picture is too simple to reach the minimum on its own, so ${fmt(i.padded_by)} bytes of padding were added.</p>` : ''}
    <div class="cmp"><figure>${srcUrl ? `<img src="${srcUrl}" alt="Original">` : ''}<figcaption>Original, ${i.srcW} x ${i.srcH}</figcaption></figure><figure><img src="${outUrl}" alt="Result" width="${i.width}" height="${i.height}"><figcaption>Result</figcaption></figure></div>
    <a class="dl" href="${outUrl}" download="panoply-${name}.jpg">Download JPEG</a></div>`;
}
async function thumb(f) { // a 640 px preview made while decoding, so a 24 MP original is never decoded just to show it
  try {
    const b = await createImageBitmap(f, { resizeWidth: 640, resizeQuality: 'medium', imageOrientation: 'from-image' });
    const c = new OffscreenCanvas(b.width, b.height); c.getContext('2d').drawImage(b, 0, 0); b.close();
    return URL.createObjectURL(await c.convertToBlob({ type: 'image/jpeg', quality: 0.85 }));
  } catch { return ''; }
}
async function pick(f) {
  if (!f || !f.type.startsWith('image/')) return;
  file = f; if (srcUrl) URL.revokeObjectURL(srcUrl); srcUrl = '';
  const d = $('#drop'); d.classList.add('has');
  d.innerHTML = `<span id="thumb"></span><span><strong>${esc(f.name)}</strong>${(f.size / 1e6).toFixed(1)} MB. Tap to choose another.</span>`;
  $('#result').innerHTML = ''; enable();
  const u = await thumb(f); if (file !== f) return; srcUrl = u; if (u) $('#thumb').outerHTML = `<img id="thumb" src="${u}" alt="">`;
}
$('#drop').onclick = () => $('#file').click();
$('#file').onchange = (e) => pick(e.target.files[0]);
$('#preset').onchange = updatePreset;
['dragenter', 'dragover'].forEach((t) => $('#drop').addEventListener(t, (e) => { e.preventDefault(); $('#drop').classList.add('over'); }));
['dragleave', 'drop'].forEach((t) => $('#drop').addEventListener(t, (e) => { e.preventDefault(); $('#drop').classList.remove('over'); }));
$('#drop').addEventListener('drop', (e) => pick(e.dataTransfer.files[0]));
addEventListener('paste', (e) => pick([...e.clipboardData.files][0]));
function args(f, preset) {
  if (preset !== CUSTOM) return { blob: f, preset };
  const kb = (id) => parseFloat($(id).value) || 0;
  return { blob: f, w: parseInt($('#cw').value) || 0, h: parseInt($('#ch').value) || 0, lo: Math.round(kb('#cmin') * 1024), hi: Math.round(kb('#cmax') * 1000) };
}
$('#go').onclick = async () => {
  busy = true; enable(); $('#go').innerHTML = '<span class="spin"></span>Working';
  try { const m = await run(args(file, $('#preset').value)); window.__bench.last = m; show(m); }
  catch (e) { $('#result').innerHTML = `<div class="card bad" role="alert">${esc(e.message)}</div>`; }
  busy = false; $('#go').textContent = 'Get the file'; enable();
};
window.__run = run;
window.__test = async (url, preset) => { const blob = await (await fetch(url)).blob(); try { const m = await run({ blob, preset }); return { ms: m.ms, lap: m.lap, info: m.info, bytes: m.bytes.length, heap: m.heap, wasmMem: m.wasmMem, stats: m.stats, workers: m.workers, sha: [...m.bytes.subarray(0, 64)].join(',') + ':' + m.bytes.length }; } catch (e) { return { error: e.message }; } };
