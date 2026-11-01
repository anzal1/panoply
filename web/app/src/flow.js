// Shared flow pieces for every tool page: the page states (working, done), the status line that mirrors the run button,
// the result actions, and the quick-pick chips. The shell and the resize-to-kb page both use them.
const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s).replace(/[<>&"]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;' }[c]));
const reduced = () => matchMedia('(prefers-reduced-motion:reduce)').matches;
export const app = () => $('.tool-app');
export const setWorking = (on) => app().classList.toggle('working', on);
export const setDone = (on) => app().classList.toggle('done', on);
export function reveal() {
  requestAnimationFrame(() => { const t = $('#result').getBoundingClientRect().top + scrollY - 72; scrollTo({ top: Math.max(0, t), behavior: reduced() ? 'auto' : 'smooth' }); });
}
export function store(key, val) { try { if (val === undefined) return localStorage.getItem(key); localStorage.setItem(key, val); } catch { return null; } }

// A progress line made from whatever the tool writes into the run button: a bar for model downloads, a spinner for the rest.
export function mirrorStatus() {
  const go = $('#go'), st = document.createElement('div'); st.id = 'status'; st.setAttribute('aria-live', 'polite'); $('#drop').after(st);
  const draw = () => {
    const t = go.textContent.trim(), m = /Downloading\D*(\d+)%/.exec(t);
    if (m && +m[1] >= 100) st.innerHTML = '<p><span class="spin"></span>Working</p>';
    else if (m) st.innerHTML = `<div class="bar" role="progressbar" aria-label="Model download" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${m[1]}"><i style="transform:scaleX(${m[1] / 100})"></i></div><p><b>Downloading once, then instant and offline.</b> <span>${m[1]}%</span></p>`;
    else if (go.querySelector('.spin') && app().classList.contains('auto')) st.innerHTML = `<p><span class="spin"></span>${esc(t)}</p>`;
    else if (!go.querySelector('.spin')) st.innerHTML = '';
  };
  new MutationObserver(draw).observe(go, { childList: true, subtree: true, characterData: true });
}

// Quick-pick chips (a radio group). items: [[id, label]], returns { get, set }. The last pick is remembered on this device.
export function chips(host, items, { key, label, onPick, none }) {
  const el = document.createElement('div'); el.className = 'chips'; el.setAttribute('role', 'radiogroup'); el.setAttribute('aria-label', label);
  el.innerHTML = items.map(([id, t]) => `<button type="button" role="radio" aria-checked="false" data-id="${esc(id)}">${esc(t)}</button>`).join('') + (none ? `<button type="button" class="chip-more" data-more>${esc(none)}</button>` : '');
  host.before(el); let cur = '';
  const set = (id, fire = true) => { cur = id; el.querySelectorAll('[role=radio]').forEach((b) => b.setAttribute('aria-checked', String(b.dataset.id === id))); if (key && id) store(key, id); if (fire && onPick) onPick(id); };
  el.addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; if (b.dataset.more !== undefined) { onPick && onPick('', true); return; } set(b.dataset.id); });
  return { el, get: () => cur, set, disable: (fn) => el.querySelectorAll('[role=radio]').forEach((b) => (b.disabled = !!fn(b.dataset.id))) };
}

// The result's actions: back to the settings, or start over.
export function actions({ tune, onSettings, onNew }) {
  const d = document.createElement('div'); d.className = 'racts';
  d.innerHTML = `${tune ? '' : '<button type="button" class="lnk" data-a="settings">Change settings</button>'}<button type="button" class="lnk" data-a="new">New file</button>`;
  d.onclick = (e) => { const a = e.target.closest('[data-a]'); if (!a) return; a.dataset.a === 'new' ? onNew() : onSettings(); };
  return d;
}

// One plain sentence and a next step for whatever a worker threw. The raw text stays out of sight.
export function plainError(e, { pdf = false } = {}) {
  const m = String((e && e.message) || e || '');
  if (/password|encrypt/i.test(m)) return 'This PDF is locked with a password. Remove the lock and try again.';
  if (/memory|alloc|RangeError|too large|too big/i.test(m)) return 'That file is too big for this device. Try a smaller one.';
  if (/fetch|network|offline|sha|integrity|model.*(load|download)|load.*model/i.test(m)) return "The model couldn't download. Check your connection and try again.";
  if (/decod|invalid|corrupt|unsupported|malformed|parse|not an? (image|pdf)|could not (open|read)/i.test(m)) return pdf ? "We couldn't open that PDF. It may be damaged. Try another copy." : "We couldn't open that file. Try a JPG, PNG or WebP.";
  if (m.length <= 140 && /^[A-Z"].*[.!?]$/.test(m)) return m;
  return pdf ? "That PDF didn't work. Try another copy." : "That file didn't work. Try another one, or a smaller one.";
}
