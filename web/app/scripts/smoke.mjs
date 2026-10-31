// Runs every tool once on the built site, with the service worker on (threads) and blocked (single thread).   node scripts/smoke.mjs http://localhost:8099 [on|off]
import { chromium } from 'playwright-core'; import fs from 'node:fs'; import path from 'node:path';
const base = process.argv[2] || 'http://localhost:8099', modes = process.argv[3] ? [process.argv[3]] : ['on', 'off'], fx = (n) => path.resolve('../fixtures/' + n), M = (n) => '/tmp/pdfout/' + n;
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const b = await chromium.launch({ executablePath: exe });
const models = (p) => p.route('**/*', (r) => { const u = r.request().url(); const f = u.includes('huggingface.co') ? (u.includes('isnet') ? 'isnet.onnx' : u.includes('PP-OCRv5_mobile_det') ? 'ppocr_det.onnx' : u.includes('en_PP-OCR') ? 'ppocr_en.onnx' : u.includes('migan') ? 'migan.onnx' : null) : u.includes('media.githubusercontent.com') ? 'yunet.onnx' : null; return f ? r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync(M(f)) }) : r.continue(); });
const tests = [
  ['resize-to-kb/ssc-signature', 'natural.jpg', async () => {}],
  ['resize', 'natural.jpg', async (p) => { await p.fill('#w', '600'); }],
  ['compress', 'natural.jpg', async () => {}],
  ['convert', 'natural.jpg', async (p) => { await p.selectOption('#fmt', 'webp'); }],
  ['strip-metadata', 'exif.jpg', async () => {}],
  ['signature', 'signature-photo.jpg', async (p) => { await p.selectOption('#preset', 'ssc-signature'); }],
  ['print-sheet', 'portrait.jpg', async () => {}],
  ['images-to-pdf', ['portrait.jpg', 'small-portrait.png'], async () => {}],
  ['pdf-merge', ['pdf/hebrew-parable.pdf', 'pdf/f1040.pdf'], async () => {}],
  ['pdf-split', 'pdf/hebrew-parable.pdf', async () => {}],
  ['pdf-organize', 'pdf/hebrew-parable.pdf', async (p) => { await p.check('#rev'); }],
  ['pdf-compress', 'pdf/hebrew-parable.pdf', async () => {}],
  ['pdf-to-images', 'pdf/f1040.pdf', async (p) => { await p.fill('#pages', '1'); }],
  ['media', 'test.mp4', async (p) => { await p.selectOption('#quality', 'low'); }],
  ['upscale', 'small-portrait.png', async (p) => { await p.selectOption('#scale', '2'); }],
  ['remove-background', 'portrait.jpg', async () => {}],
  ['ocr', 'f1040-page.png', async () => {}],
  ['ocr-pdf', 'pdf/scan.pdf', async () => {}],
  ['passport-photo/us-passport', 'portrait-big.jpg', async () => {}],
  ['remove-watermark', 'wm-corner.png', async (p) => { await p.check('#own'); }],
  ['erase', 'portrait.jpg', async (p) => { await p.waitForSelector('#editor:not([hidden])'); await p.waitForTimeout(200); const bb = await p.locator('#paint').boundingBox(); await p.mouse.move(bb.x + bb.width * 0.2, bb.y + bb.height * 0.55); await p.mouse.down(); await p.mouse.move(bb.x + bb.width * 0.3, bb.y + bb.height * 0.6, { steps: 5 }); await p.mouse.up(); }],
];
for (const mode of modes) {
  let ok = 0; const bad = [];
  for (const [page, file, setup] of tests) {
    const ctx = await b.newContext({ acceptDownloads: true, ...(mode === 'off' ? { serviceWorkers: 'block' } : {}) }), p = await ctx.newPage(); await models(p); const errs = []; p.on('pageerror', (e) => errs.push(String(e).slice(0, 120)));
    try {
      await p.goto(`${base}/tools/${page}/`); await p.waitForTimeout(mode === 'off' ? 500 : 3000); await p.waitForFunction(() => !document.querySelector('#preset,#rule') || !document.querySelector('#preset,#rule').disabled, null, { timeout: 60000 });
      await p.setInputFiles('#file', (Array.isArray(file) ? file : [file]).map(fx)); await p.waitForTimeout(500); await setup(p); await p.click('#go');
      await p.waitForSelector('#result .card', { timeout: 240000 }); const alert = await p.$('#result [role=alert]');
      if (alert) throw new Error('error card: ' + (await alert.textContent()).slice(0, 100));
      ok++;
    } catch (e) { bad.push(`${page}: ${String(e.message).split('\n')[0].slice(0, 110)} ${errs.join(' ')}`); }
    await ctx.close();
  }
  console.log(`threads ${mode.toUpperCase()}: ${ok} of ${tests.length} tools ran`, bad.length ? '\n  FAILED: ' + bad.join('\n  FAILED: ') : '');
}
await b.close();
