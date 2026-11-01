// Static server with on-the-fly brotli/gzip so the browser reports real transfer sizes.
import http from 'node:http'; import fs from 'node:fs'; import zlib from 'node:zlib'; import path from 'node:path';
const root = path.resolve(process.argv[2] || path.dirname(new URL(import.meta.url).pathname));
const types = {'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.jpg':'image/jpeg','.json':'application/json','.mjs':'text/javascript','.css':'text/css','.svg':'image/svg+xml','.png':'image/png','.webp':'image/webp'};
const cache = new Map();
http.createServer((req,res)=>{
  let p = path.join(root, decodeURIComponent(req.url.split('?')[0])); if (p.endsWith('/')) p += 'index.html';
  if (fs.existsSync(p) && fs.statSync(p).isDirectory()) p = path.join(p, 'index.html');
  if (!p.startsWith(root) || !fs.existsSync(p)) { res.writeHead(404); return res.end(); }
  const ae = req.headers['accept-encoding']||''; const enc = p.includes('/models/') ? '' : ae.includes('br')?'br':ae.includes('gzip')?'gzip':'';
  const key = p+enc; let body = cache.get(key);
  if (!body) { const raw = fs.readFileSync(p); body = enc==='br'?zlib.brotliCompressSync(raw,{params:{[zlib.constants.BROTLI_PARAM_QUALITY]:11}}):enc==='gzip'?zlib.gzipSync(raw,{level:9}):raw; cache.set(key,body); }
  const h = {...(process.env.NOISO?{}:{'cross-origin-opener-policy':'same-origin','cross-origin-embedder-policy':'require-corp'}),'content-type':types[path.extname(p)]||'application/octet-stream','cache-control':'no-store','content-length':body.length};
  if (enc) h['content-encoding']=enc; res.writeHead(200,h); res.end(body);
}).listen(8099,()=>console.log('http://localhost:8099'));
