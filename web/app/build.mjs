// Builds the browser tools: bundles every tool and worker with esbuild into --out (default dist), copies the ONNX Runtime files, and writes
// the service worker (offline caching, plus the cross-origin isolation headers GitHub Pages cannot send). Run from web/app.
//   node build.mjs --out ../../site/public/tools/app --sw ../../site/public/tools/sw.js
import { build } from 'esbuild'; import fs from 'node:fs'; import path from 'node:path'; import crypto from 'node:crypto';
const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i > 0 ? process.argv[i + 1] : d; };
const OUT = path.resolve(arg('out', 'dist')), SW = path.resolve(arg('sw', path.join(OUT, '..', 'sw.js')));
process.chdir(path.dirname(new URL(import.meta.url).pathname)); // entry points are relative to web/app, wherever the build is started from
fs.rmSync(OUT, { recursive: true, force: true }); fs.mkdirSync(path.join(OUT, 'assets'), { recursive: true });
const common = { absWorkingDir: process.cwd(), bundle: true, format: 'esm', minify: true, loader: { '.wasm': 'file', '.json': 'json', '.dict': 'text' }, assetNames: 'assets/[name]-[hash]', publicPath: './', target: 'es2022', legalComments: 'none' };
const tools = { resize: 'resize', compress: 'compress', convert: 'convert', strip: 'strip', 'resize-to-kb': 'resize-to-kb', 'pdf-merge': 'pdf-merge', 'pdf-split': 'pdf-split', 'pdf-organize': 'pdf-organize', 'pdf-compress': 'pdf-compress', 'pdf-to-images': 'pdf-to-images', background: 'background', ocr: 'ocr', 'ocr-pdf': 'ocr-pdf', upscale: 'upscale', erase: 'erase', watermark: 'watermark', passport: 'passport', signature: 'signature', sheet: 'sheet', 'images-to-pdf': 'images-to-pdf', media: 'media' };
// Plain workers and the page tools share code chunks.
await build({ ...common, splitting: true, chunkNames: 'chunk-[hash]', outdir: OUT, entryPoints: { 'image-worker': 'src/image-worker.js', 'pdf-worker': 'src/pdf-worker.js', 'render-worker': 'src/render-worker.js', 'media-worker': 'src/media-worker.js', 'id-worker': 'src/id-worker.js', worker: 'src/worker.js', ...Object.fromEntries(Object.entries(tools).map(([k, v]) => [k, `src/tools/${v}.js`])) } });
// Workers that run ONNX Runtime Web take its wasm from the page's own folder instead of bundling it.
await build({ ...common, conditions: ['onnxruntime-web-use-extern-wasm'], outdir: OUT, entryPoints: { 'vision-worker': 'src/vision-worker.js', 'passport-worker': 'src/passport-worker.js', 'ocr-worker': 'src/ocr-worker.js', 'upscale-worker': 'src/upscale-worker.js', 'inpaint-worker': 'src/inpaint-worker.js', 'watermark-worker': 'src/watermark-worker.js' } });
fs.mkdirSync(path.join(OUT, 'ort'), { recursive: true });
for (const f of ['ort-wasm-simd-threaded.mjs', 'ort-wasm-simd-threaded.wasm']) fs.copyFileSync('node_modules/onnxruntime-web/dist/' + f, path.join(OUT, 'ort', f));
// The Real-ESRGAN files (the host sends no CORS headers) are fetched at build time and checked against pinned hashes.
await import('./fetch-models.mjs').then((m) => m.default && m.default(path.join(OUT, 'models', 'esrgan')));

// ---- tool styles: the app's stylesheet scoped under .tool-app, on the site's dark tokens, so it cannot touch the rest of the page
function scopeCss(css) {
  const out = []; let i = 0;
  const readBlock = (str, start) => { let d = 0, j = start; for (; j < str.length; j++) { if (str[j] === '{') d++; else if (str[j] === '}') { d--; if (d === 0) return j; } } return str.length - 1; };
  const pre = (sel) => sel.split(',').map((x) => x.trim()).filter(Boolean).map((x) => { if (x === ':root' || x === 'html' || x === 'body') return '.tool-app'; if (x === '*') return '.tool-app *'; if (x === 'main') return '.tool-app'; return x.startsWith('[hidden]') ? '.tool-app [hidden]' : '.tool-app ' + x; }).join(',');
  const walkCss = (str) => { const res = []; let k = 0; while (k < str.length) { const open = str.indexOf('{', k); if (open < 0) break; const head = str.slice(k, open).trim(), close = readBlock(str, open), body = str.slice(open + 1, close);
      if (head.startsWith('@media')) { if (/prefers-color-scheme:\s*(dark|light)/.test(head)) { /* the site is dark only */ } else res.push(head + '{' + walkCss(body) + '}'); }
      else if (head.startsWith('@keyframes')) res.push(head + '{' + body + '}');
      else if (head === ':root' || head === 'html' || head === 'body') { /* tokens and page-level rules come from the site */ if (head === 'body') res.push('.tool-app{font:14px/20px var(--sf);color:var(--ink);-webkit-font-smoothing:antialiased}'); }
      else res.push(pre(head) + '{' + body + '}');
      k = close + 1; } return res.join('\n'); };
  const tokens = '.tool-app{--card:var(--ui,#16161C);--field:var(--ui2,#1F2027);--ink:#EDEDF0;--ink2:#A6A8B1;--ink3:#8E909A;--hair:rgba(255,255,255,.12);--accent:#5E9BFF;--on-accent:#0D0E12;--good:#3DD56D;--warn:#FFB340;--bad:#FF7A70;--tint:rgba(94,155,255,.14);color-scheme:dark}';
  return tokens + '\n' + walkCss(css.replace(/\/\*[\s\S]*?\*\//g, ''));
}
fs.writeFileSync(path.join(OUT, 'tools.css'), scopeCss(fs.readFileSync('src/ui.css', 'utf8')));

// ---- service worker
const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]);
const skip = /^(models|ort)\//, big = /avif_enc|pdfium|squoosh_oxipng_bg\.wasm$|\.map$/;
const files = walk(OUT).map((f) => path.relative(OUT, f).split(path.sep).join('/')).filter((f) => !skip.test(f) && !big.test(f)).sort();
const ver = crypto.createHash('sha1').update(files.map((f) => f + fs.statSync(path.join(OUT, f)).size).join('|')).digest('hex').slice(0, 10);
const base = '/' + path.relative(path.join(path.dirname(SW), '..'), OUT).split(path.sep).join('/') + '/'; // e.g. /tools/app/ (the service worker sits in /tools/)
fs.mkdirSync(path.dirname(SW), { recursive: true });
fs.writeFileSync(SW, `// Generated by web/app/build.mjs. Two jobs for everything under /tools/:
// 1. Cross-origin isolation. GitHub Pages cannot send COOP and COEP headers, so this worker adds them to the responses it serves; the
//    pages register it, reload once, and then SharedArrayBuffer (threads for ONNX Runtime Web) is available. Without it every tool still
//    works, single-threaded.
// 2. Offline use. The app files are cached up front; models, big codecs and pages are cached the first time they are used.
const V = 'panoply-tools-${ver}', BASE = ${JSON.stringify(base)}, PRE = ${JSON.stringify(files.map((f) => base + f))};
self.addEventListener('install', (e) => { e.waitUntil(caches.open(V).then((c) => c.addAll(PRE)).then(() => self.skipWaiting())); });
self.addEventListener('activate', (e) => { e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k.startsWith('panoply-tools-') && k !== V).map((k) => caches.delete(k)))).then(() => self.clients.claim())); });
const isolate = (r) => { if (r.type === 'opaque' || r.type === 'opaqueredirect' || r.status === 0) return r; const h = new Headers(r.headers); h.set('Cross-Origin-Embedder-Policy', 'require-corp'); h.set('Cross-Origin-Opener-Policy', 'same-origin'); h.set('Cross-Origin-Resource-Policy', 'same-origin'); return new Response(r.body, { status: r.status, statusText: r.statusText, headers: h }); };
self.addEventListener('fetch', (e) => {
  const u = new URL(e.request.url); if (e.request.method !== 'GET' || u.origin !== location.origin || !u.pathname.startsWith('/tools/')) return;
  e.respondWith((async () => {
    const c = await caches.open(V), hit = await c.match(e.request, { ignoreSearch: true });
    const net = fetch(e.request).then((r) => { if (r.ok && r.status === 200) c.put(e.request, r.clone()); return r; }).catch(() => null);
    if (hit) { e.waitUntil(net); return isolate(hit); }
    const r = await net; return r ? isolate(r) : new Response('Offline, and this part has not been used yet.', { status: 503, headers: { 'content-type': 'text/plain' } });
  })());
});
`);
const size = (d) => walk(d).reduce((a, f) => a + fs.statSync(f).size, 0);
console.log('tools built:', files.length, 'app files precached,', (size(OUT) / 1e6).toFixed(1), 'MB in', path.relative(process.cwd(), OUT), '| worker version', ver);
