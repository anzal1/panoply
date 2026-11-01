// Closed selects cut long choices short on a phone. This keeps the full chosen text readable under each select.
const fullLabel = (sel) => {
  let n = sel.nextElementSibling;
  if (!n || !n.classList.contains('tfull')) { n = document.createElement('div'); n.className = 'tfull'; n.setAttribute('aria-hidden', 'true'); sel.after(n); }
  const t = sel.selectedOptions[0] ? sel.selectedOptions[0].textContent : '';
  if (n.textContent !== t) n.textContent = t;
  const cs = getComputedStyle(sel), ctx = (fullLabel.c ||= document.createElement('canvas').getContext('2d')); ctx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
  const hid = ctx.measureText(t).width <= sel.clientWidth - 48; // the arrow and padding take about 48px
  if (n.hidden !== hid) n.hidden = hid;
};
export function keepFullLabels(root = '#opts') {
  const all = () => document.querySelectorAll(`${root} select`).forEach(fullLabel);
  document.addEventListener('change', (e) => e.target.matches && e.target.matches(`${root} select`) && fullLabel(e.target));
  addEventListener('resize', all); setInterval(all, 1000); setTimeout(all, 0);
}

// Notes under the options (model sizes, licences, disclaimers) live in one collapsed disclosure instead of on screen.
export function foldNotes(root = '#opts') {
  const run = () => {
    const box = document.querySelector(root); if (!box) return;
    const hs = [...box.querySelectorAll('.hint')].filter((h) => !h.closest('.tinfo'));
    if (!hs.length) return;
    let d = box.querySelector(':scope > .tinfo');
    if (!d) { d = document.createElement('details'); d.className = 'tinfo'; d.innerHTML = '<summary>Notes</summary>'; box.append(d); }
    hs.forEach((h) => d.append(h));
  };
  new MutationObserver(run).observe(document.querySelector(root) || document.body, { childList: true, subtree: true }); run();
}
