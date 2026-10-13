import http from 'node:http'; import fs from 'node:fs'; import path from 'node:path';
const types = {'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.wasm':'application/wasm','.mp4':'video/mp4'};
http.createServer((req,res)=>{
  const p = path.join(path.dirname(new URL(import.meta.url).pathname), decodeURIComponent(req.url.split('?')[0]) === '/' ? 'bench.html' : decodeURIComponent(req.url.split('?')[0]));
  if(!fs.existsSync(p) || fs.statSync(p).isDirectory()){res.writeHead(404);return res.end();}
  res.writeHead(200,{'Content-Type':types[path.extname(p)]||'application/octet-stream','Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp','Cross-Origin-Resource-Policy':'same-origin'});
  fs.createReadStream(p).pipe(res);
}).listen(4800,()=>console.log('bench on http://localhost:4800'));
