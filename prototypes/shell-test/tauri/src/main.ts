import './style.css';
import tools from '../../shared/utilities.json';
import { ICONS } from './icons.generated';
import { fuzzy } from './fuzzy';
import { Spring } from './spring';

interface Tool {
  id: string;
  name: string;
  subtitle: string;
  category: string;
  lucide: string;
  accepts: string[];
}

// ---- host bridge (no-ops in a plain browser, used for design preview) ----------------
type Internals = { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
const internals = (window as unknown as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__;
const inTauri = !!internals;
const host = (cmd: string, args: Record<string, unknown> = {}) => {
  if (internals) void internals.invoke(cmd, args);
};
if (!inTauri) document.documentElement.classList.add('preview');
const wall = () => performance.timeOrigin + performance.now();

// ---- constants from the spec ---------------------------------------------------------
const ROW_H = 44;
const HEAD_H = 28;
const PAD_TOP = 8;
const SHOW_MS = 180;
const HIDE_MS = 120;
const EASE_OUT = 'cubic-bezier(0.2, 0.9, 0.25, 1)';

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const panel = $('panel');
const inner = $('inner');
const input = $<HTMLInputElement>('q');
const scroll = $('scroll');
const content = $('content');
const itemsEl = $('items');
const pill = $('pill');
const pillFlash = pill.firstElementChild as HTMLElement;
const emptyEl = $('empty');
const emptyA = $('emptyA');
const dropEl = $('drop');
const dropA = $('dropA');
const listEl = $('list');
const ring = $('ring');
const chip = $('chip');
const chipA = $('chipA');
const chipB = $('chipB');
const lead = $('lead');

const svg = (name: string, size: number, extra = '') =>
  `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" ${extra} aria-hidden="true">${ICONS[name] ?? ''}</svg>`;
$('mag').innerHTML = ICONS.search;
$('chipXIcon').innerHTML = ICONS.x;

input.placeholder = `Search ${tools.length} tools or drop a file`;

// ---- model ---------------------------------------------------------------------------
const catOrder: string[] = [];
for (const t of tools as Tool[]) if (!catOrder.includes(t.category)) catOrder.push(t.category);

interface RowRef {
  tool: Tool;
  index: number;
  el: HTMLElement;
  nm: HTMLElement;
  lower: string;
  hl: string;
  y: number;
  firstInSection: boolean;
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const rows: RowRef[] = (tools as Tool[]).map((tool, index) => {
  const el = document.createElement('div');
  el.className = `row c-${tool.category.toLowerCase()}`;
  el.innerHTML =
    `<span class="tile">${svg(tool.lucide, 15)}</span>` +
    `<span class="txt"><span class="nm">${esc(tool.name)}</span><span class="sub">${esc(tool.subtitle)}</span></span>` +
    `<span class="tr"><span class="cat">${esc(tool.category)}</span><span class="kc ret">↵</span></span>`;
  el.dataset.i = String(index);
  return {
    tool,
    index,
    el,
    nm: el.querySelector('.nm') as HTMLElement,
    lower: tool.name.toLowerCase(),
    hl: '',
    y: 0,
    firstInSection: false,
  };
});

const headers = new Map<string, HTMLElement>();
const header = (title: string) => {
  let h = headers.get(title);
  if (!h) {
    h = document.createElement('div');
    h.className = 'hd';
    h.textContent = title;
    headers.set(title, h);
  }
  return h;
};

// ---- state ---------------------------------------------------------------------------
let query = '';
let file: { name: string; ext: string } | null = null;
let visibleRows: RowRef[] = [];
let sel = 0;
let state: 'hidden' | 'showing' | 'shown' | 'hiding' = 'hidden';
let lastShown = false; // a frame of the current show has been presented
const shownWaiters: Array<() => void> = [];
let reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', (e) => (reduceMotion = e.matches));

const normExt = (e: string) => (e === 'jpeg' ? 'jpg' : e === 'tif' ? 'tiff' : e === 'htm' ? 'html' : e);
const accepts = (t: Tool, ext: string) => t.accepts.includes('*') || t.accepts.includes(ext);

// ---- filtering ----------------------------------------------------------------------
interface Hit {
  r: RowRef;
  score: number;
  pos: number[];
}

function search(q: string): Hit[] {
  const tokens = q.toLowerCase().split(/\s+/).filter(Boolean);
  const joined = tokens.join('');
  const hits: Hit[] = [];
  for (const r of rows) {
    if (file && !accepts(r.tool, file.ext)) continue;
    const m = fuzzy(joined, r.tool.name, r.lower);
    if (m) {
      hits.push({ r, score: m.score, pos: m.pos });
      continue;
    }
    // Secondary: every token appears as a word-start inside subtitle or category.
    const hay = ` ${r.tool.subtitle} ${r.tool.category}`.toLowerCase();
    if (tokens.every((t) => hay.includes(` ${t}`))) hits.push({ r, score: -50 - r.index * 0.001, pos: [] });
  }
  hits.sort((a, b) => b.score - a.score || a.r.index - b.r.index);
  return hits;
}

function highlight(r: RowRef, pos: number[]): void {
  const key = pos.join(',');
  if (r.hl === key) return;
  r.hl = key;
  if (!pos.length) {
    r.nm.textContent = r.tool.name;
    return;
  }
  const name = r.tool.name;
  let out = '';
  let pi = 0;
  let open = false;
  for (let i = 0; i < name.length; i++) {
    const on = pi < pos.length && pos[pi] === i;
    if (on) pi++;
    if (on && !open) out += '<b>';
    if (!on && open) out += '</b>';
    open = on;
    out += esc(name[i]);
  }
  if (open) out += '</b>';
  r.nm.innerHTML = out;
}

// ---- rendering ------------------------------------------------------------------------

function render(): void {
  const desired: HTMLElement[] = [];
  const vis: RowRef[] = [];
  let y = PAD_TOP;

  const pushHeader = (t: string) => {
    desired.push(header(t));
    y += HEAD_H;
  };
  const pushRow = (r: RowRef, first: boolean) => {
    r.y = y;
    r.firstInSection = first;
    y += ROW_H;
    desired.push(r.el);
    vis.push(r);
  };

  if (query.trim() === '') {
    for (const c of catOrder) {
      let first = true;
      for (const r of rows) {
        if (r.tool.category !== c) continue;
        if (file && !accepts(r.tool, file.ext)) continue;
        if (first) pushHeader(c);
        highlight(r, []);
        pushRow(r, first);
        first = false;
      }
    }
  } else {
    const hits = search(query);
    if (hits.length) pushHeader('Best matches');
    hits.forEach((h, i) => {
      highlight(h.r, h.pos);
      pushRow(h.r, i === 0);
    });
  }

  // Keyed reconcile: only touch nodes whose position changed.
  let ref: ChildNode | null = itemsEl.firstChild;
  for (const el of desired) {
    if (el === ref) {
      ref = ref.nextSibling;
    } else {
      itemsEl.insertBefore(el, ref);
    }
  }
  while (ref) {
    const nx: ChildNode | null = ref.nextSibling;
    itemsEl.removeChild(ref);
    ref = nx;
  }
  visibleRows = vis;

  const empty = vis.length === 0;
  if (empty) {
    emptyA.textContent = query.trim() ? `No tools match “${query.trim()}”` : file ? `No tools accept .${file.ext}` : 'No tools';
  }
  setMsg(emptyEl, empty);
  content.style.height = `${y + PAD_TOP}px`;
}

function setMsg(el: HTMLElement, on: boolean): void {
  const v = on ? '1' : '0';
  if (el.dataset.on !== v) el.dataset.on = v;
}

// ---- selection pill + scroll follow (one spring loop, transform only for the pill) -----
const pillSpring = new Spring(PAD_TOP + HEAD_H);
const scrollSpring = new Spring(0);
let raf = 0;
let last = 0;
let pillOpacity = 1;

function paintPill(): void {
  pill.style.transform = `translate3d(0, ${pillSpring.value}px, 0)`;
}

function tick(now: number): void {
  const dt = Math.min(0.034, Math.max(0.001, (now - last) / 1000));
  last = now;
  let again = false;
  if (pillSpring.active) {
    pillSpring.step(dt);
    paintPill();
    again = again || pillSpring.active;
  }
  if (scrollSpring.active) {
    scrollSpring.step(dt);
    scroll.scrollTop = scrollSpring.value;
    again = again || scrollSpring.active;
  }
  raf = again ? requestAnimationFrame(tick) : 0;
}

function wake(): void {
  if (!raf) {
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }
}

const viewH = () => listEl.clientHeight || 364;

function maxScroll(): number {
  return Math.max(0, parseFloat(content.style.height || '0') - viewH());
}

/** Aim the pill at the selected row and keep it on screen with a little margin. */
function aim(resetScroll: boolean): void {
  const r = visibleRows[sel];
  if (!r) {
    if (pillOpacity !== 0) {
      pill.style.opacity = '0';
      pillOpacity = 0;
    }
    return;
  }
  if (pillOpacity !== 1) {
    pill.style.opacity = '1';
    pillOpacity = 1;
    pillSpring.jump(r.y);
    paintPill();
  }

  if (resetScroll) {
    scrollSpring.jump(0);
    scroll.scrollTop = 0;
    // The pill must not fly across the list from far outside the viewport: clamp its start
    // to just beyond the nearest edge, then glide.
    const lo = -ROW_H;
    const hi = viewH();
    if (pillSpring.value < lo) pillSpring.value = lo;
    else if (pillSpring.value > hi) pillSpring.value = hi;
  } else {
    const top = r.firstInSection ? r.y - HEAD_H : r.y;
    const margin = 8;
    const st = scrollSpring.active ? scrollSpring.target : scroll.scrollTop;
    let want = st;
    if (sel === 0) want = 0;
    else if (top - margin < st) want = top - margin;
    else if (r.y + ROW_H + margin > st + viewH()) want = r.y + ROW_H + margin - viewH();
    want = Math.min(Math.max(0, want), maxScroll());
    if (want !== st) {
      if (!scrollSpring.active) {
        scrollSpring.value = scroll.scrollTop;
        scrollSpring.velocity = 0;
      }
      scrollSpring.target = want;
    }
  }

  if (reduceMotion) {
    pillSpring.jump(r.y);
    paintPill();
    pill.animate([{ opacity: 0.4 }, { opacity: 1 }], { duration: 90, easing: 'ease-out' });
  } else {
    pillSpring.target = r.y;
  }
  wake();
}

function setSel(i: number, follow = true): void {
  const n = visibleRows.length;
  if (!n) return;
  const c = Math.max(0, Math.min(n - 1, i));
  if (c === sel) return;
  visibleRows[sel]?.el.classList.remove('sel');
  sel = c;
  visibleRows[sel].el.classList.add('sel');
  if (follow) aim(false);
  else {
    pillSpring.target = visibleRows[sel].y;
    wake();
  }
}

// ---- update pipeline (one function for typing, drops, selftest) ---------------------------
function update(): void {
  render();
  visibleRows[sel]?.el.classList.remove('sel');
  sel = 0;
  visibleRows[0]?.el.classList.add('sel');
  aim(true);
}

function onQuery(): void {
  query = input.value;
  update();
}
input.addEventListener('input', onQuery);

// ---- run ------------------------------------------------------------------------------
let leadTimer = 0;
function run(): void {
  const r = visibleRows[sel];
  if (!r) return;
  pillFlash.animate([{ opacity: 0 }, { opacity: 1, offset: 0.18 }, { opacity: 0 }], {
    duration: reduceMotion ? 220 : 340,
    easing: 'ease-out',
  });
  lead.textContent = `Opened ${r.tool.name}`;
  lead.animate([{ opacity: 0.2 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' });
  clearTimeout(leadTimer);
  leadTimer = window.setTimeout(() => {
    lead.textContent = 'Arsenal';
    lead.animate([{ opacity: 0.2 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' });
  }, 1200);
}

// ---- file drop ---------------------------------------------------------------------------
const extOf = (p: string) => {
  const base = p.split('/').pop() ?? p;
  const i = base.lastIndexOf('.');
  return i > 0 ? normExt(base.slice(i + 1).toLowerCase()) : '';
};

function setFile(path: string | null): void {
  if (!path) {
    file = null;
    chip.hidden = true;
  } else {
    const name = path.split('/').pop() ?? path;
    file = { name, ext: extOf(path) };
    // Middle truncation: the tail (last 8 chars, i.e. the extension) never shrinks.
    const cut = Math.max(0, name.length - 8);
    chipA.textContent = name.slice(0, cut);
    chipB.textContent = name.slice(cut);
    chipA.title = '';
    chip.hidden = false;
  }
  query = input.value;
  update();
}
$('chipX').addEventListener('click', () => {
  setFile(null);
  input.focus();
});

function drag(kind: string, paths: string[]): void {
  if (kind === 'enter') {
    const ext = paths[0] ? extOf(paths[0]) : '';
    dropA.textContent = ext ? `Drop to see tools for .${ext}` : 'Drop to see tools for this file';
    ring.classList.add('on');
    listEl.classList.add('dropping');
    setMsg(dropEl, true);
  } else if (kind === 'leave') {
    ring.classList.remove('on');
    listEl.classList.remove('dropping');
    setMsg(dropEl, false);
  } else if (kind === 'drop') {
    ring.classList.remove('on');
    listEl.classList.remove('dropping');
    setMsg(dropEl, false);
    if (paths[0]) setFile(paths[0]);
  }
}

// ---- show / hide ---------------------------------------------------------------------------
let anim: Animation | null = null;
let hideFallback = 0;

function presented(): void {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const t1 = wall();
      lastShown = true;
      host('shown', { t1 });
      while (shownWaiters.length) shownWaiters.shift()!();
    }),
  );
}

function show(): void {
  clearTimeout(hideFallback);
  const wasHiding = state === 'hiding';
  let from = 0;
  if (wasHiding && anim) {
    from = parseFloat(getComputedStyle(inner).opacity) || 0;
    anim.cancel();
  }
  state = 'showing';
  lastShown = false;
  inner.style.opacity = '1';
  anim?.cancel();
  anim = reduceMotion
    ? inner.animate([{ opacity: from }, { opacity: 1 }], { duration: 120, easing: 'ease-out' })
    : inner.animate(
        [
          { opacity: from, transform: 'translate3d(0,-6px,0) scale(0.985)' },
          { opacity: 1, transform: 'translate3d(0,0,0) scale(1)' },
        ],
        { duration: SHOW_MS, easing: EASE_OUT },
      );
  anim.onfinish = () => {
    if (state === 'showing') state = 'shown';
  };
  input.focus({ preventScroll: true });
  presented();
}

function resetState(): void {
  query = '';
  input.value = '';
  if (file) {
    file = null;
    chip.hidden = true;
  }
  ring.classList.remove('on');
  listEl.classList.remove('dropping');
  setMsg(dropEl, false);
  update();
  lead.textContent = 'Arsenal';
  clearTimeout(leadTimer);
}

function finishHide(): void {
  if (state !== 'hiding') return;
  state = 'hidden';
  inner.style.opacity = '0';
  anim?.cancel();
  anim = null;
  resetState();
  host('finish_hide');
}

function hide(fromPage = false): void {
  if (state === 'hidden' || state === 'hiding') return;
  if (fromPage) host('begin_hide');
  state = 'hiding';
  lastShown = false;
  anim?.cancel();
  anim = reduceMotion
    ? inner.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 90, easing: 'ease-in', fill: 'forwards' })
    : inner.animate(
        [
          { opacity: 1, transform: 'translate3d(0,0,0) scale(1)' },
          { opacity: 0, transform: 'translate3d(0,0,0) scale(0.99)' },
        ],
        { duration: HIDE_MS, easing: 'ease-in', fill: 'forwards' },
      );
  anim.onfinish = finishHide;
  hideFallback = window.setTimeout(finishHide, HIDE_MS + 60);
}

// ---- keyboard ------------------------------------------------------------------------------
window.addEventListener('keydown', (e) => {
  if (e.isComposing) return;
  const k = e.key;
  if (k === 'ArrowDown' || (e.ctrlKey && (k === 'n' || k === 'j'))) {
    e.preventDefault();
    setSel(sel + 1);
  } else if (k === 'ArrowUp' || (e.ctrlKey && (k === 'p' || k === 'k'))) {
    e.preventDefault();
    setSel(sel - 1);
  } else if (k === 'Enter') {
    e.preventDefault();
    run();
  } else if (k === 'Escape') {
    e.preventDefault();
    if (input.value !== '') {
      input.value = '';
      onQuery();
    } else {
      hide(true);
    }
  } else if (k === 'Tab' || (e.metaKey && k.toLowerCase() === 'k')) {
    e.preventDefault();
  }
});

// Keep focus in the field no matter where the pointer lands.
panel.addEventListener('mousedown', (e) => {
  if (e.target !== input) e.preventDefault();
});
window.addEventListener('focus', () => state !== 'hidden' && input.focus({ preventScroll: true }));
window.addEventListener('contextmenu', (e) => e.preventDefault());
window.addEventListener('dragstart', (e) => e.preventDefault());
window.addEventListener('dragover', (e) => e.preventDefault());
window.addEventListener('drop', (e) => e.preventDefault());
window.addEventListener('gesturestart', (e) => e.preventDefault());
scroll.addEventListener('wheel', () => {
  // A person scrolling wins over the follow animation.
  if (scrollSpring.active) scrollSpring.jump(scroll.scrollTop);
}, { passive: true });

let lx = -1;
let ly = -1;
itemsEl.addEventListener('mousemove', (e) => {
  if (e.clientX === lx && e.clientY === ly) return;
  lx = e.clientX;
  ly = e.clientY;
  const el = (e.target as HTMLElement).closest('.row') as HTMLElement | null;
  if (!el) return;
  const i = visibleRows.findIndex((r) => r.el === el);
  if (i >= 0 && i !== sel) setSel(i, false);
});
itemsEl.addEventListener('click', (e) => {
  const el = (e.target as HTMLElement).closest('.row') as HTMLElement | null;
  if (!el) return;
  const i = visibleRows.findIndex((r) => r.el === el);
  if (i >= 0) {
    setSel(i, false);
    run();
  }
});

// ---- self test (GET /selftest) ---------------------------------------------------------------
const frame2 = () =>
  new Promise<void>((res) => requestAnimationFrame(() => requestAnimationFrame(() => res())));

async function selftest(): Promise<void> {
  if (!lastShown) await new Promise<void>((r) => shownWaiters.push(r));
  const seq: string[] = [];
  for (const word of ['compress pdf', 'heic to jpg', 'trim video']) {
    for (let i = 1; i <= word.length; i++) seq.push(word.slice(0, i));
    for (let i = word.length - 1; i >= 0; i--) seq.push(word.slice(0, i));
  }
  const times: number[] = [];
  for (const q of seq) {
    const t = performance.now();
    input.value = q;
    onQuery();
    await frame2();
    times.push(performance.now() - t);
  }
  const s = [...times].sort((a, b) => a - b);
  const rank = (p: number) => s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)];
  const r3 = (x: number) => Math.round(x * 1000) / 1000;
  const json = JSON.stringify({ n: s.length, p50: r3(rank(0.5)), p95: r3(rank(0.95)), max: r3(s[s.length - 1]) });
  input.value = '';
  onQuery();
  host('selftest_done', { json });
}

// ---- boot ---------------------------------------------------------------------------------
render();
visibleRows[0]?.el.classList.add('sel');
pillSpring.jump(visibleRows[0]?.y ?? PAD_TOP + HEAD_H);
paintPill();

interface Api {
  show: () => void;
  hide: () => void;
  drag: (kind: string, paths: string[]) => void;
  selftest: () => Promise<void>;
}
(window as unknown as { __arsenal: Api }).__arsenal = { show, hide: () => hide(false), drag, selftest };

// First presented frame: the launch timestamp. The window is ordered in at alpha 0 by the host.
requestAnimationFrame(() => requestAnimationFrame(() => host('launched', { t: wall() })));

if (!inTauri) {
  // Design preview in a plain browser: present immediately.
  state = 'showing';
  inner.style.opacity = '1';
  input.focus();
  state = 'shown';
}
