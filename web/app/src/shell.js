// Shared tool shell: nav, header, drop zone with a cheap preview, options slot, run button, result card.
import { keepFullLabels, foldNotes } from './fulllabel.js';
import { app, setWorking, setDone, reveal, mirrorStatus, actions } from './flow.js';
export const $ = (s, r = document) => r.querySelector(s);
export const esc = (s) => String(s).replace(/[<>&"]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c]));
export const fold = (label, html) => `<details class="tinfo"><summary>${label}</summary>${html}</details>`;
export const fmtBytes = (n) => n >= 1e6 ? (n / 1e6).toFixed(2) + ' MB' : n >= 1e3 ? (n / 1e3).toFixed(1) + ' KB' : n + ' B';
export const TOOLS = [['resize-to-kb', 'Resize to exact KB'], ['resize', 'Resize'], ['compress', 'Compress'], ['convert', 'Convert'], ['strip-metadata', 'Remove metadata'], ['pdf-merge', 'Merge PDF'], ['pdf-split', 'Split PDF'], ['pdf-organize', 'Organize PDF'], ['pdf-compress', 'Compress PDF'], ['pdf-to-images', 'PDF to pictures'], ['remove-background', 'Remove background'], ['ocr', 'Read text'], ['upscale', 'Upscale'], ['erase', 'Erase objects'], ['remove-watermark', 'Remove watermark'], ['ocr-pdf', 'Searchable PDF'], ['passport-photo', 'Passport photo'], ['signature', 'Signature cleaner'], ['print-sheet', 'Print sheet'], ['images-to-pdf', 'Pictures to PDF'], ['media', 'Video and audio']];
export async function thumb(f, keepAlpha = false) {
  try {
    const b = await createImageBitmap(f, { resizeWidth: 640, resizeQuality: 'medium', imageOrientation: 'from-image' });
    const c = new OffscreenCanvas(b.width, b.height); const g = c.getContext('2d'); if (!keepAlpha) { g.fillStyle = '#fff'; g.fillRect(0, 0, b.width, b.height); } g.drawImage(b, 0, 0); b.close();
    return URL.createObjectURL(await c.convertToBlob(keepAlpha ? { type: 'image/png' } : { type: 'image/jpeg', quality: 0.85 }));
  } catch { return ''; }
}
export function registerOffline() {
  if (!('serviceWorker' in navigator)) return;
  navigator.serviceWorker.register(new URL('../sw.js', import.meta.url), { scope: '../' }).then(() => navigator.serviceWorker.ready).then(() => { 0 }).catch(() => {});
}
export function nav(current) {
  const n = $('#nav'); if (!n) return;
  const t = (TOOLS.find(([id]) => id === current) || [])[1] || '';
  n.innerHTML = `<a href="../">Panoply</a><span aria-hidden="true">/</span><a href="../" class="all">All tools</a><span aria-hidden="true">/</span><a href="./" aria-current="page">${t}</a>`;
}
// cfg: { id, title, sub, accept, options: html, run(file, ui) -> { blob, name, lines[], hint } , onReady(root) }
export function tool(cfg) {
  registerOffline(); nav(cfg.id); keepFullLabels(); foldNotes();
  if (!document.querySelector('[data-keep]')) { document.title = `${cfg.title} | Panoply`; $('#h1').textContent = cfg.title; $('#sub').textContent = cfg.sub; }
  const file = $('#file'); file.accept = cfg.accept || 'image/*'; if (cfg.multiple) file.multiple = true;
  $('#opts').innerHTML = cfg.options || ''; if (!cfg.options) $('#opts').hidden = true;
  let f = null, srcUrl = '', outUrl = '', busy = false, token = 0;
  const go = $('#go'); go.textContent = cfg.cta || 'Get the file';
  const enable = () => (go.disabled = busy || !f || (cfg.multiple && f.length < (cfg.min || 1)));
  const drop = $('#drop'), root = app(), initialDrop = drop.innerHTML; root.classList.toggle('auto', !!cfg.auto); mirrorStatus();
  let tune = null, again = false, timer = 0;
  if (cfg.auto) { // runs on drop; the options live in a Tune disclosure that re-runs the job when something changes
    go.closest('section').hidden = true;
    if (cfg.options) { tune = document.createElement('details'); tune.className = 'tinfo tune'; tune.id = 'tune'; tune.innerHTML = '<summary>Tune</summary>'; $('#result').after(tune); tune.append($('#opts')); }
  }
  const isOk = (x) => cfg.pdf ? (x.type === 'application/pdf' || /\.pdf$/i.test(x.name)) : cfg.accept && cfg.accept.includes('video') ? /^(video|audio)\//.test(x.type) || /\.(mp4|mov|m4v|webm|mkv|mp3|m4a|wav|ogg|flac|aac)$/i.test(x.name) : x.type.startsWith('image/');
  const docIcon = '<span id="thumb" class="doc" aria-hidden="true">PDF</span>';
  async function pick(x) {
    if (cfg.multiple) {
      const add = [...(x.length === undefined ? [x] : x)].filter(isOk); if (!add.length) return;
      f = [...(Array.isArray(f) ? f : []), ...add]; ++token; $('#result').innerHTML = '';
      drop.classList.add('has'); root.classList.add('has'); drop.innerHTML = `${docIcon}<span><strong>${f.length} file${f.length === 1 ? '' : 's'}</strong>Tap to add more files.</span>`;
      enable(); cfg.onFile && cfg.onFile(f, (nf) => { f = nf; drop.querySelector('strong').textContent = `${f.length} file${f.length === 1 ? '' : 's'}`; enable(); }); return;
    }
    if (!x || !isOk(x)) return;
    f = x; const my = ++token; if (srcUrl) URL.revokeObjectURL(srcUrl); srcUrl = '';
    drop.classList.add('has'); root.classList.add('has'); drop.innerHTML = `${cfg.pdf || (cfg.accept || '').includes('video') ? docIcon.replace('PDF', cfg.pdf ? 'PDF' : 'MEDIA') : '<span id="thumb"></span>'}<span><strong>${esc(x.name)}</strong><span id="finfo">${fmtBytes(x.size)}</span>. Tap to choose another.</span>`;
    $('#result').innerHTML = ''; enable(); cfg.onFile && cfg.onFile(x);
    if (cfg.pdf || (cfg.accept || '').includes('video')) return maybeRun();
    const u = await thumb(x); if (my !== token) return; srcUrl = u; if (u) $('#thumb').outerHTML = `<img id="thumb" src="${u}" alt="">`;
    maybeRun();
  }
  drop.onclick = () => file.click(); file.onchange = (e) => { pick(cfg.multiple ? e.target.files : e.target.files[0]); file.value = ''; };
  ['dragenter', 'dragover'].forEach((t) => drop.addEventListener(t, (e) => { e.preventDefault(); drop.classList.add('over'); }));
  ['dragleave', 'drop'].forEach((t) => drop.addEventListener(t, (e) => { e.preventDefault(); drop.classList.remove('over'); }));
  drop.addEventListener('drop', (e) => pick(cfg.multiple ? e.dataTransfer.files : e.dataTransfer.files[0]));
  addEventListener('paste', (e) => { const l = [...e.clipboardData.files]; if (l.length) pick(cfg.multiple ? l : l[0]); });
  const reset = () => { f = null; ++token; if (srcUrl) URL.revokeObjectURL(srcUrl); srcUrl = ''; drop.classList.remove('has'); drop.innerHTML = initialDrop; $('#result').innerHTML = ''; setDone(false); root.classList.remove('has'); enable(); cfg.onReset && cfg.onReset(); scrollTo(0, 0); };
  const ready = () => !!f && !busy && (!cfg.multiple || f.length >= (cfg.min || 1)) && (!cfg.ready || cfg.ready());
  const maybeRun = () => { if (cfg.auto && ready()) run(); };
  if (tune) ['input', 'change'].forEach((t) => tune.addEventListener(t, () => { clearTimeout(timer); timer = setTimeout(() => (busy ? (again = true) : maybeRun()), 350); }));
  go.onclick = () => run();
  async function run() {
    if (busy || !f) return;
    busy = true; enable(); setWorking(true); go.innerHTML = '<span class="spin"></span>Working'; const t0 = performance.now();
    try {
      const r = await cfg.run(f); r.ms = performance.now() - t0; window.__last = r;
      if (outUrl) URL.revokeObjectURL(outUrl);
      outUrl = URL.createObjectURL(r.blob); const pv = r.preview === false || cfg.pdf || (cfg.accept || '').includes('video') ? '' : await thumb(r.blob, !!r.checker);
      const inSize = Array.isArray(f) ? f.reduce((a, x) => a + x.size, 0) : f.size, smaller = r.blob.size < inSize, pct = Math.round(Math.abs(1 - r.blob.size / inSize) * 100);
      const cmp = pv || srcUrl ? `<div class="cmp">${srcUrl ? `<figure><img src="${srcUrl}" alt="Original"><figcaption>Original</figcaption></figure>` : ''}${pv ? `<figure${r.checker ? ' class="checker"' : ''}><img src="${pv}" alt="Result"><figcaption>Result</figcaption></figure>` : ''}</div>` : '';
      const more = `<div class="meta">${(r.lines || []).map((l) => `<span>${esc(l)}</span>`).join('')}<span>${(r.ms / 1000).toFixed(2)} s</span></div>${r.hint ? `<p class="hint">${esc(r.hint)}</p>` : ''}`;
      const one = r.single && pv;
      const cmp2 = one ? `<div class="cmp one"><figure${r.checker ? ' class="checker"' : ''}><img id="rimg" src="${pv}" alt="Result"></figure>${srcUrl ? '<button type="button" class="lnk hold">Hold to see the original</button>' : ''}</div>` : cmp;
      $('#result').innerHTML = `<div class="card rcard"><div class="big ${r.bad ? 'bad' : 'tok'}">${r.title ? esc(r.title) + ' ' : ''}${fmtBytes(r.blob.size)} <span class="kb">${r.delta === false ? '' : (!smaller && pct === 0 ? 'Already as small as it gets' : `from ${fmtBytes(inSize)}, ${pct}% ${smaller ? 'smaller' : 'larger'}`)}</span></div>
        ${r.noDownload ? '' : `<a class="dl" href="${outUrl}" download="${esc(r.name)}">Download ${esc(r.name.split('.').pop().toUpperCase())}${r.dlNote ? ' (' + esc(r.dlNote) + ')' : ''}</a>`}
        ${cmp2}${r.html || ''}
        ${fold('Details', more)}</div>`;
      const card = $('#result .rcard'); card.insertBefore(actions({ tune: !!tune, onSettings: () => { setDone(false); scrollTo({ top: 0 }); }, onNew: reset }), card.querySelector('.tinfo'));
      const hold = $('#result .hold'); if (hold) { const img = $('#rimg'), show = (o) => { img.src = o ? srcUrl : pv; }; hold.onpointerdown = () => show(true); ['pointerup', 'pointerleave', 'pointercancel', 'blur'].forEach((t) => hold.addEventListener(t, () => show(false))); hold.onkeydown = (e) => { if (e.key === ' ' || e.key === 'Enter') { e.preventDefault(); show(true); } }; hold.onkeyup = () => show(false); }
      const was = root.classList.contains('done'); setDone(true); if (!was) reveal();
    } catch (e) { setDone(false); $('#result').innerHTML = `<div class="card bad" role="alert">${esc(e.message)}</div>`; }
    busy = false; setWorking(false); go.textContent = cfg.cta || 'Get the file'; enable();
    if (again) { again = false; maybeRun(); }
  }
  cfg.onReady && cfg.onReady(); window.__tool = { pick: (x) => pick(x), run: () => go.click(), maybeRun, state: () => ({ f, busy }) };
}
export function slider(id, label, min, max, val, fmt = (v) => v) {
  return `<div class="field"><label for="${id}">${label} <span class="trv" id="${id}v">${fmt(val)}</span></label><input id="${id}" type="range" min="${min}" max="${max}" value="${val}"></div>`;
}
export function bindSlider(id, fmt = (v) => v) { const e = $('#' + id), v = $('#' + id + 'v'); e.oninput = () => (v.textContent = fmt(+e.value)); }
export const baseName = (n) => n.replace(/\.[^.]+$/, '');
export function sameFormat(f) { return { 'image/jpeg': 'jpeg', 'image/png': 'png', 'image/webp': 'webp', 'image/avif': 'avif' }[f.type] || 'jpeg'; }
