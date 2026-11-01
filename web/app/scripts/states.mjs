// The transient states the breakpoint pass cannot reach by waiting: working, model downloading (a slow local model server) and an error.
import { chromium } from 'playwright-core'; import fs from 'node:fs'; import https from 'node:https'; import path from 'node:path';
const base = process.argv[2] || 'http://localhost:8099', dir = process.argv[3] || 'shots-states'; fs.mkdirSync(dir, { recursive: true });
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const model = fs.readFileSync('/tmp/pdfout/isnet.onnx'), F = (n) => path.resolve('../fixtures/' + n);
const srv = https.createServer({ key: fs.readFileSync('/tmp/ux/k.pem'), cert: fs.readFileSync('/tmp/ux/c.pem') }, (q, r) => { r.writeHead(200, { 'access-control-allow-origin': '*', 'content-length': model.length, 'content-type': 'application/octet-stream' }); let i = 0; const step = Math.ceil(model.length / 14); const t = setInterval(() => { r.write(model.subarray(i, i + step)); i += step; if (i >= model.length) { clearInterval(t); r.end(); } }, 220); r.on('close', () => clearInterval(t)); }).listen(8101);
const b = await chromium.launch({ executablePath: exe, args: ['--mute-audio'] });
let bad = 0;
for (const [w, h] of [[375, 812], [768, 1024], [1440, 900], [2560, 1440]]) {
  // model downloading, then the result
  let c = await b.newContext({ viewport: { width: w, height: h }, ignoreHTTPSErrors: true }), p = await c.newPage();
  await p.route(/huggingface\.co.*isnet/, (r) => r.continue({ url: 'https://localhost:8101/isnet.onnx' }));
  await p.goto(base + '/tools/remove-background/'); await p.waitForFunction(() => crossOriginIsolated, null, { timeout: 8000 }).catch(() => {}); await p.waitForTimeout(600);
  await p.setInputFiles('#file', F('portrait.jpg')); await p.waitForFunction(() => { const b = document.querySelector('#status .bar'); return b && +b.getAttribute('aria-valuenow') >= 35; }, null, { timeout: 20000, polling: 50 }).catch(() => { bad++; console.log(w, 'no model bar'); });
  await p.screenshot({ path: `${dir}/removebg-${w}-downloading.png` });
  const line = await p.evaluate(() => document.querySelector('#status')?.innerText); console.log(w, 'status:', JSON.stringify(line));
  await p.waitForSelector('#result .rcard', { timeout: 120000 }); await p.waitForTimeout(400);
  // live Tune: a new background re-runs from the cached mask
  await p.evaluate(() => { const t = document.querySelector('#tune'); t.open = true; }); await p.selectOption('#look', 'color'); const t0 = Date.now();
  await p.waitForFunction(() => document.querySelector('.tool-app').classList.contains('working'), null, { timeout: 4000 }).catch(() => {}); await p.waitForFunction(() => !document.querySelector('.tool-app').classList.contains('working'), null, { timeout: 60000 });
  console.log(w, 'tune re-run ms', Date.now() - t0); await p.waitForTimeout(300); await p.screenshot({ path: `${dir}/removebg-${w}-tune.png` }); await c.close();
  // error: a file that is named like a photo and is not one
  c = await b.newContext({ viewport: { width: w, height: h }, ignoreHTTPSErrors: true }); p = await c.newPage();
  await p.goto(base + '/tools/compress/'); await p.waitForTimeout(500);
  await p.setInputFiles('#file', { name: 'broken.jpg', mimeType: 'image/jpeg', buffer: Buffer.from('this is not a picture') }); await p.waitForSelector('#result .card.bad', { timeout: 15000 }).catch(() => { bad++; console.log(w, 'no error card'); });
  await p.waitForTimeout(300); await p.screenshot({ path: `${dir}/compress-${w}-error.png` });
  console.log(w, 'error text:', JSON.stringify(await p.evaluate(() => document.querySelector('#result .card.bad')?.innerText))); await c.close();
}
await b.close(); srv.close(); process.exit(bad ? 1 : 0);
