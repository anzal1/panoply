// The command bar: real DOM, shared by all three concepts. Themed by CSS variables in bar.css.
import { search, fileKind } from './tools.js';
export function createBar({ mount, placeholder='Drop a file, or type under 50 kb', onChange=()=>{}, onFile=()=>{}, onPick=()=>{} }){
  const el = document.createElement('div'); el.className='pbar';
  el.innerHTML = `<form class="pbar-in" role="search" aria-label="Panoply command bar" autocomplete="off">
    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>
    <input type="text" aria-label="What do you want to do?" placeholder="${placeholder}" spellcheck="false">
    <span class="pbar-file" hidden></span>
    <label class="pbar-add" title="Choose a file"><input type="file" hidden>+</label>
  </form><div class="pbar-res" aria-live="polite"></div><div class="pbar-note" aria-live="polite"></div>`;
  mount.appendChild(el);
  const input = el.querySelector('input[type=text]'), res = el.querySelector('.pbar-res'), chip = el.querySelector('.pbar-file'), note = el.querySelector('.pbar-note'), pick = el.querySelector('input[type=file]');
  let kind = null, matches = [], file = null;
  function render(){
    matches = search(input.value, kind);
    res.innerHTML = '';
    matches.slice(0,3).forEach((m,i)=>{ const b=document.createElement('button'); b.type='button'; b.textContent=m.tool.name; if(i===0) b.className='on'; b.onmouseenter=()=>onChange(input.value,matches,m.tool); b.onclick=()=>onPick(m.tool,file); res.appendChild(b); });
    onChange(input.value, matches, null);
  }
  input.addEventListener('input', render);
  el.querySelector('form').addEventListener('submit', e=>{ e.preventDefault(); if(matches[0]) onPick(matches[0].tool, file); });
  addEventListener('keydown', e=>{ if(e.key==='Escape'){ input.value=''; setFile(null); render(); } if(e.key==='/' && document.activeElement!==input){ e.preventDefault(); input.focus(); } });
  function setFile(f){ file=f; kind=fileKind(f); chip.hidden=!f; chip.textContent = f? f.name : ''; if(!f) note.textContent=''; render(); }
  function take(f){ if(!f) return; setFile(f); onFile(f, kind); }
  pick.addEventListener('change', ()=>take(pick.files[0]));
  let depth=0;
  addEventListener('dragenter',e=>{ e.preventDefault(); depth++; document.body.classList.add('dragging'); });
  addEventListener('dragleave',()=>{ depth=Math.max(0,depth-1); if(!depth) document.body.classList.remove('dragging'); });
  addEventListener('dragover',e=>e.preventDefault());
  addEventListener('drop',e=>{ e.preventDefault(); depth=0; document.body.classList.remove('dragging'); take(e.dataTransfer.files[0]); });
  return { el, input, setNote:t=>{ note.textContent=t; }, setFile, refresh:render, get file(){return file}, get kind(){return kind}, get matches(){return matches} };
}
