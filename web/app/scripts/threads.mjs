// Times the ONNX Runtime tools with threads (service worker isolation on) and without (service workers blocked). The model files come
// from local copies so the numbers are compute only.   node scripts/threads.mjs http://localhost:8099
import { chromium } from 'playwright-core'; import fs from 'node:fs'; import path from 'node:path';
const base = process.argv[2] || 'http://localhost:8099', fx = (n) => path.resolve('../fixtures/' + n), M = (n) => '/tmp/pdfout/' + n;
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const b = await chromium.launch({ executablePath: exe });
const route = (p) => p.route('**/huggingface.co/**', (r) => { const u = r.request().url(); const f = u.includes('isnet') ? 'isnet.onnx' : u.includes('PP-OCRv5_mobile_det') ? 'ppocr_det.onnx' : u.includes('en_PP-OCR') ? 'ppocr_en.onnx' : u.includes('migan') ? 'migan.onnx' : null; return f ? r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync(M(f)) }) : r.continue(); });
async function run(mode, tool, file, setup = async () => {}) {
  const ctx = await b.newContext(mode === 'off' ? { serviceWorkers: 'block' } : {}), p = await ctx.newPage(); await route(p);
  await p.goto(`${base}/tools/${tool}/`); await p.waitForTimeout(mode === 'off' ? 800 : 3500);
  const iso = await p.evaluate(() => crossOriginIsolated); const out = [];
  for (const k of [1, 2]) { // first run includes the model session, the second is warm
    await p.setInputFiles('#file', fx(file)); await setup(p); await p.evaluate(() => (document.querySelector('#result').innerHTML = '')); const t0 = Date.now(); await p.click('#go'); await p.waitForSelector('#result .card', { timeout: 300000 });
    out.push({ run: k, wall: Date.now() - t0, text: (await p.textContent('#result .meta')).replace(/\s+/g, ' ').slice(0, 130) });
  }
  console.log(tool, mode === 'off' ? 'threads OFF' : 'threads ON ', 'isolated=' + iso, JSON.stringify(out)); await ctx.close();
}
for (const mode of ['on', 'off']) { await run(mode, 'remove-background', 'portrait-big.jpg'); await run(mode, 'ocr', 'f1040-page.png'); }
await b.close();
