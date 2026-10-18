// Shared by the three immersive concepts. Plain ES module, no build step.
export const TINT = { pdf:'#D9544F', image:'#8E5BC4', media:'#E08A2E', id:'#D9A92C', files:'#2D6CDF' };
export const CATNAME = { pdf:'PDF', image:'Image', media:'Video & audio', id:'ID photo', files:'Files' };
export const ICON = {
  pdf:'M7 3h7l4 4v14H7z M10 12h5 M10 16h5',
  image:'M4 18l5-6 4 4 3-3 4 5 M10.5 8a1.5 1.5 0 1 0 -3 0a1.5 1.5 0 1 0 3 0',
  media:'M8 5l11 7-11 7z',
  id:'M15 9a3 3 0 1 0 -6 0a3 3 0 1 0 6 0 M6 20c1-4 11-4 12 0',
  files:'M4 7h6l2 2h8v9H4z'
};
// [name, category, keywords, sizeTarget?]
export const TOOLS = ([
 ['Compress PDF','pdf','shrink smaller size email reduce mb kb',1],['Merge PDF','pdf','combine join'],['Split PDF','pdf','separate extract pages divide'],
 ['Redact PDF','pdf','black out hide blackout private censor'],['Organize PDF pages','pdf','reorder rotate delete pages'],['PDF to images','pdf','png jpg convert export pages'],
 ['Images to PDF','pdf','combine photos jpg png'],['OCR','pdf','text scan read recognize searchable'],
 ['Hash files','files','checksum sha verify'],['Zip','files','archive compress pack'],['Unzip','files','extract archive unpack'],
 ['Instant file search','files','find locate spotlight'],['Disk map','files','storage space big folders usage'],['Share to phones','files','send airdrop transfer qr wifi'],
 ['Passport photo','id','id visa size photo biometric'],['Print sheet','id','print photos sheet 4x6'],['Signature cleaner','id','sign transparent scan'],
 ['Resize to exact KB','image','kb size under exam form target limit',3],['Compress image','image','shrink smaller size jpg png reduce',1],['Resize image','image','dimensions pixels scale'],
 ['Convert image','image','format png jpg webp heic'],['Remove background','image','cutout transparent bg'],['Remove watermark','image','clean logo'],
 ['Erase object','image','remove delete object retouch'],['Upscale','image','enlarge sharpen bigger resolution hd'],['Strip metadata','image','exif privacy location remove'],
 ['Image to text','image','ocr read text extract'],
 ['Compress video','media','shrink smaller size mp4 mb',1],['Convert video or audio','media','format mp4 mp3 mov wav'],['Trim video or audio','media','cut clip shorten'],
 ['Extract audio','media','mp3 sound from video'],['Noise removal','media','denoise clean hiss voice'],['Text to speech','media','voice read aloud narrate tts'],
 ['Normalize loudness','media','volume level lufs'],['HDR to SDR','media','tone map colors washed'],['Repair video','media','fix broken corrupt'],['Stereo to mono','media','channel mix audio']
]).map((t,i)=>({i,name:t[0],cat:t[1],k:t[2],size:t[3]||0}));

const SYN = { smaller:'compress size', small:'compress size', under:'size compress', shrink:'compress', reduce:'compress', lighter:'compress',
  mb:'size compress', kb:'size compress', email:'compress', text:'ocr', transcribe:'ocr', read:'ocr', cut:'trim', clip:'trim', fix:'repair', louder:'normalize', quieter:'normalize' };
const words = s => (s.toLowerCase().match(/[a-z0-9]+/g) || []);

export function fileKind(file){
  if(!file) return null;
  const n = (file.name||'').toLowerCase(), t = file.type||'';
  if(t.startsWith('image/')||/\.(png|jpe?g|webp|heic|gif|bmp|tiff?)$/.test(n)) return 'image';
  if(t==='application/pdf'||n.endsWith('.pdf')) return 'pdf';
  if(t.startsWith('video/')||t.startsWith('audio/')||/\.(mp4|mov|mkv|webm|mp3|wav|m4a|flac)$/.test(n)) return 'media';
  return 'files';
}
const KIND_DEFAULT = { image:'Compress image', pdf:'Compress PDF', media:'Compress video', files:'Zip', id:'Passport photo' };

export function search(query, kind){
  const q = (query||'').trim();
  const sizeIntent = /\d+\s*(kb|mb|k|m)\b/i.test(q) || /\b(under|below|smaller)\b/i.test(q);
  let toks = words(q); const ex = [];
  toks.forEach(t=>{ if(SYN[t]) ex.push(...words(SYN[t])); });
  const out = [];
  for(const t of TOOLS){
    let s = 0; const nw = words(t.name), kw = words(t.k);
    for(const tk of toks.concat(ex)){
      const w = ex.includes(tk) && !toks.includes(tk) ? .45 : 1;
      if(tk.length < 2) continue;
      if(nw.some(x=>x.startsWith(tk))) s += 3*w; else if(kw.some(x=>x===tk)) s += 2*w; else if(tk.length>2 && kw.some(x=>x.startsWith(tk))) s += 1.2*w;
    }
    if(sizeIntent && t.size) s += 3 + t.size;
    if(s>0 && kind){ if(t.cat===kind) s += 3; else if(sizeIntent) s -= 1.5; }
    if(s>0) out.push({tool:t,score:s});
  }
  out.sort((a,b)=>b.score-a.score||a.tool.i-b.tool.i);
  if(!q && kind){ const d = TOOLS.find(t=>t.name===KIND_DEFAULT[kind]); const rest = TOOLS.filter(t=>t.cat===kind&&t!==d); return [d,...rest].slice(0,5).map((t,j)=>({tool:t,score:5-j*.2})); }
  return out.slice(0,6);
}

export async function fontsReady(){
  try{ await Promise.all([document.fonts.load('600 40px Geist'),document.fonts.load('500 20px Geist'),document.fonts.load('500 20px "Geist Mono"')]); }catch(e){}
}
function rr(c,x,y,w,h,r){ c.beginPath(); c.moveTo(x+r,y); c.arcTo(x+w,y,x+w,y+h,r); c.arcTo(x+w,y+h,x,y+h,r); c.arcTo(x,y+h,x,y,r); c.arcTo(x,y,x+w,y,r); c.closePath(); }
function glyph(c,cat,x,y,s,color,lw){ c.save(); c.translate(x,y); c.scale(s/24,s/24); c.strokeStyle=color; c.lineWidth=lw; c.lineCap='round'; c.lineJoin='round'; const p = new Path2D(ICON[cat]); c.stroke(p); c.restore(); }
function wrap(c,text,x,y,maxW,lh){ const ws=text.split(' '); let line=''; const lines=[]; for(const w of ws){ const t=line?line+' '+w:w; if(c.measureText(t).width>maxW&&line){lines.push(line);line=w}else line=t } lines.push(line); lines.forEach((l,i)=>c.fillText(l,x,y+i*lh)); return lines.length; }
function mk(w,h){ const c=document.createElement('canvas'); c.width=w; c.height=h; return c; }

// style 'plate': warm paper, landscape 4:3 (640x480). 'fan': portrait (512x920). 'glass': dark panel (512x704).
export function paintCard(tool, style='plate'){
  const tint = TINT[tool.cat];
  if(style==='plate'){
    const cv = mk(640,480), c = cv.getContext('2d');
    const g = c.createLinearGradient(0,0,0,480); g.addColorStop(0,'#FFFDF8'); g.addColorStop(1,'#F6F1E5'); c.fillStyle=g; c.fillRect(0,0,640,480);
    c.fillStyle=tint; rr(c,44,44,92,92,24); c.fill(); glyph(c,tool.cat,64,64,52,'#fff',2);
    c.fillStyle='#26231E'; c.font='600 60px Geist, sans-serif'; c.textBaseline='alphabetic'; wrap(c,tool.name,44,300,560,64);
    c.fillStyle='#7A746A'; c.font='500 24px "Geist Mono", monospace'; c.fillText(CATNAME[tool.cat].toUpperCase(),44,438);
    c.fillStyle='rgba(60,45,20,.06)'; c.fillRect(0,0,640,3); return cv;
  }
  if(style==='fan'){
    const cv = mk(512,920), c = cv.getContext('2d');
    const g = c.createLinearGradient(0,0,0,920); g.addColorStop(0,'#FFFFFF'); g.addColorStop(1,'#F1F3F7'); c.fillStyle=g; c.fillRect(0,0,512,920);
    const wash = c.createRadialGradient(256,300,10,256,300,380); wash.addColorStop(0,tint+'26'); wash.addColorStop(1,tint+'00'); c.fillStyle=wash; c.fillRect(0,0,512,920);
    c.fillStyle=tint; rr(c,176,200,160,160,42); c.fill(); glyph(c,tool.cat,208,232,96,'#fff',1.9);
    c.fillStyle='#17181B'; c.textAlign='center'; c.font='600 56px Geist, sans-serif'; const n=wrapC(c,tool.name,256,640,420,62);
    c.fillStyle='#6B6F7A'; c.font='500 24px "Geist Mono", monospace'; c.fillText(CATNAME[tool.cat].toUpperCase(),256,640+n*62+22);
    return cv;
  }
  const cv = mk(512,704), c = cv.getContext('2d');
  const g = c.createLinearGradient(0,0,0,704); g.addColorStop(0,'#1A2029'); g.addColorStop(1,'#0E1218'); c.fillStyle=g; c.fillRect(0,0,512,704);
  const gl = c.createRadialGradient(256,250,10,256,250,300); gl.addColorStop(0,'rgba(190,210,255,.22)'); gl.addColorStop(1,'rgba(190,210,255,0)'); c.fillStyle=gl; c.fillRect(0,0,512,704);
  glyph(c,tool.cat,176,170,160,'#EAF0FF',1.15);
  c.fillStyle='#EEF2FA'; c.textAlign='center'; c.font='600 48px Geist, sans-serif'; const n=wrapC(c,tool.name,256,500,440,54);
  c.fillStyle='#8C96A8'; c.font='500 22px "Geist Mono", monospace'; c.fillText(CATNAME[tool.cat].toUpperCase(),256,500+n*54+14);
  return cv;
}
function wrapC(c,text,x,y,maxW,lh){ const ws=text.split(' '); let line=''; const lines=[]; for(const w of ws){ const t=line?line+' '+w:w; if(c.measureText(t).width>maxW&&line){lines.push(line);line=w}else line=t } lines.push(line); lines.forEach((l,i)=>c.fillText(l,x,y+i*lh)); return lines.length; }

// A card for the visitor's own file. Images show the image itself. Nothing is read by a server.
export async function paintFile(file, style='plate'){
  const kind = fileKind(file);
  const portrait = style!=='plate';
  const W = portrait?512:640, H = portrait?(style==='fan'?920:704):480;
  const cv = mk(W,H), c = cv.getContext('2d');
  c.fillStyle = style==='glass'?'#161B23':'#FFFFFF'; c.fillRect(0,0,W,H);
  let drew = false;
  if(kind==='image'){
    try{ const bmp = await createImageBitmap(file); const pad=18, aw=W-pad*2, ah=H-pad*2-70; const s=Math.max(aw/bmp.width,ah/bmp.height); c.save(); c.beginPath(); c.rect(pad,pad,aw,ah); c.clip(); c.drawImage(bmp,pad+(aw-bmp.width*s)/2,pad+(ah-bmp.height*s)/2,bmp.width*s,bmp.height*s); c.restore(); drew=true; }catch(e){}
  }
  if(!drew){ c.fillStyle=TINT[kind]+'33'; c.fillRect(18,18,W-36,H-106); glyph(c,kind==='media'?'media':kind,W/2-60,(H-70)/2-60,120,TINT[kind],1.5); }
  c.fillStyle = style==='glass'?'#EEF2FA':'#26231E'; c.font='600 30px Geist, sans-serif'; c.textAlign='left';
  let nm = file.name||'file'; while(c.measureText(nm).width>W-48&&nm.length>6) nm=nm.slice(0,-2); if(nm!==file.name) nm+='...';
  c.fillText(nm,24,H-40);
  c.fillStyle='#8A8478'; c.font='500 20px "Geist Mono", monospace'; const sz=file.size; c.fillText(sz>1e6?(sz/1e6).toFixed(1)+' MB':Math.max(1,Math.round(sz/1e3))+' KB',24,H-14);
  return {canvas:cv, kind};
}
export const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
export const isPhone = () => Math.min(innerWidth, innerHeight) < 600 || /Android|iPhone|iPad/i.test(navigator.userAgent);
export const clamp=(x,a,b)=>Math.min(b,Math.max(a,x)); export const lerp=(a,b,t)=>a+(b-a)*t; export const sstep=(a,b,x)=>{const t=clamp((x-a)/(b-a),0,1);return t*t*(3-2*t)};
// Damped follow, frame-rate independent.
export const damp=(a,b,k,dt)=>lerp(a,b,1-Math.exp(-k*dt));
export function rnd(i){const x=Math.sin(i*127.1+311.7)*43758.5453;return x-Math.floor(x)}
