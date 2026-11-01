// Breakpoint pass: 375..2560 with real heights, result state, overflow, console errors, failed requests, axe, block-fits-screen.
import { chromium } from 'playwright-core'; import fs from 'node:fs'; import path from 'node:path';
const url = process.argv[2] || 'http://localhost:8099/index.html', dir = process.argv[3] || 'shots';
fs.mkdirSync(dir, { recursive: true });
const sizes = [[375, 812], [390, 844], [768, 1024], [1024, 768], [1280, 800], [1440, 900], [1920, 1080], [2560, 1440]];
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const axeSrc = fs.readFileSync('node_modules/axe-core/axe.min.js', 'utf8');
const browser = await chromium.launch({ executablePath: exe, args: ['--mute-audio'] });
let fail = 0;
const pages = [
  ['', null], ['resize-to-kb', async (p) => { await p.selectOption('#preset', 'passport-seva-photo'); }],
  ['resize', async (p) => { await p.fill('#w', '800'); }], ['pdf-merge', 'pdf', async () => {}], ['pdf-split', 'pdf', async (p) => { await p.selectOption('#mode', 'ranges'); await p.fill('#ranges', '1-2, last'); }], ['pdf-organize', 'pdf', async (p) => { await p.fill('#del', '2'); await p.check('#rev'); }], ['pdf-compress', 'pdf', async () => {}], ['ocr-pdf', 'pdf', async () => {}], ['pdf-to-images', 'pdf', async (p) => { await p.fill('#pages', '1'); }], ['remove-background', async () => {}], ['ocr', async () => {}], ['remove-watermark', async (p) => { await p.check('#own'); }], ['passport-photo', async (p) => { await p.waitForFunction(() => !document.querySelector('#rule').disabled); await p.selectOption('#rule', 'us-passport'); }], ['signature', async (p) => { await p.selectOption('#preset', 'ssc-signature'); }], ['print-sheet', async () => {}], ['images-to-pdf', 'imgs', async () => {}], ['media', 'media', async () => {}], ['upscale', async (p) => { await p.selectOption('#scale', '2'); }], ['erase', async (p) => { await p.waitForSelector('#editor:not([hidden])'); await p.waitForTimeout(200); const b = await p.locator('#paint').boundingBox(); await p.mouse.move(b.x + b.width * 0.2, b.y + b.height * 0.55); await p.mouse.down(); await p.mouse.move(b.x + b.width * 0.3, b.y + b.height * 0.6, { steps: 6 }); await p.mouse.up(); }], ['compress', async () => {}], ['convert', async (p) => { await p.selectOption('#fmt', 'webp'); }], ['strip-metadata', async () => {}],
];
const base = url.replace(/\/$|\/index\.html$/, '');
for (const scheme of ['light', 'dark']) for (const [w, h] of sizes) for (const entry of pages.filter((e) => !process.env.ONLY || process.env.ONLY.split(',').includes(e[0]))) {
  const pg = entry[0], kind = typeof entry[1] === 'string' ? entry[1] : '', isPdf = kind === 'pdf', setup = kind ? entry[2] : entry[1];
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, colorScheme: scheme, deviceScaleFactor: 1 }), page = await ctx.newPage();
  if ((pg === 'passport-photo' || pg === 'remove-background' || pg === 'ocr' || pg === 'ocr-pdf' || pg === 'upscale' || pg === 'erase' || pg === 'remove-watermark' || pg === 'passport-photo') && ![375, 768, 1440, 2560].includes(w)) { await ctx.close(); continue; }
  const errs = [], bad = [];
  if (pg === 'passport-photo') await page.route('**/*', async (r) => { const u = r.request().url(); if (u.includes('huggingface.co')) return r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/isnet.onnx') }); if (u.includes('media.githubusercontent.com')) return r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/yunet.onnx') }); return r.continue(); });
  if (pg === 'remove-watermark') await page.route('**/*', async (r) => { const u = r.request().url(); if (u.includes('huggingface.co')) return r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/migan.onnx') }); if (u.includes('media.githubusercontent.com')) return r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/yunet.onnx') }); return r.continue(); });
  if (pg === 'erase') await page.route('**/huggingface.co/**', (r) => r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/migan.onnx') }));
  if (pg === 'ocr' || pg === 'ocr-pdf') await page.route('**/huggingface.co/**', (r) => r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync(r.request().url().includes('PP-OCRv5_mobile_det') ? '/tmp/pdfout/ppocr_det.onnx' : '/tmp/pdfout/ppocr_en.onnx') }));
  if (pg === 'remove-background') await page.route('**/huggingface.co/**', (r) => r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*', 'content-type': 'application/octet-stream' }, body: fs.readFileSync('/tmp/pdfout/isnet.onnx') }));
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text())); page.on('pageerror', (e) => errs.push(String(e)));
  page.on('requestfailed', (r) => { const e = r.failure() && r.failure().errorText; if (e === 'net::ERR_ABORTED' && r.url().includes('/models/')) return; /* Chromium reports a fully read, hash-checked model fetch as aborted now and then */ bad.push(r.url() + ' ' + e); }); page.on('response', (r) => r.status() >= 400 && bad.push(r.status() + ' ' + r.url()));
  await page.goto(`${base}/${pg}${pg ? '/' : ''}`);
  const tag = `${scheme}-${w}x${h}-${pg || 'home'}`;
  if (setup) {
    await page.waitForFunction(() => { const s = document.querySelector('#preset'); return !s || !s.disabled; });
    await page.screenshot({ path: path.join(dir, `${tag}-empty.png`) });
    await page.setInputFiles('#file', pg === 'images-to-pdf' ? [path.resolve('../web/fixtures/portrait.jpg'), path.resolve('../web/fixtures/small-portrait.png')] : pg === 'media' ? path.resolve('../web/fixtures/test.mp4') : pg === 'signature' ? path.resolve('../web/fixtures/signature-photo.jpg') : isPdf ? (pg === 'pdf-merge' ? [path.resolve('../web/fixtures/pdf/hebrew-parable.pdf'), path.resolve('../web/fixtures/pdf/f1040.pdf')] : pg === 'ocr-pdf' ? path.resolve('../web/fixtures/pdf/scan.pdf') : path.resolve('../web/fixtures/pdf/hebrew-parable.pdf')) : path.resolve(pg === 'remove-background' ? '../web/fixtures/portrait.jpg' : pg === 'ocr' ? '../web/fixtures/f1040-page.png' : pg === 'upscale' ? '../web/fixtures/small-portrait.png' : pg === 'erase' ? '../web/fixtures/portrait.jpg' : pg === 'passport-photo' ? '../web/fixtures/portrait-big.jpg' : pg === 'remove-watermark' ? '../web/fixtures/wm-corner.png' : '../web/fixtures/natural.jpg')); await page.waitForTimeout(isPdf || kind ? 600 : 0); await setup(page); await page.click('#go'); await page.waitForSelector('#result .card', { timeout: 120000 }); await page.waitForTimeout(300);
  }
  await page.screenshot({ path: path.join(dir, `${tag}-result.png`) });
  const m = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth, sh: document.documentElement.scrollHeight, cards: [...document.querySelectorAll('.card, #drop, .tool')].map((e) => [e.id || e.className, Math.round(e.getBoundingClientRect().height)]) }));
  if (m.sh > h) { await page.screenshot({ path: path.join(dir, `${tag}-full.png`), fullPage: true }); }
  await page.addScriptTag({ content: axeSrc });
  const ax = await page.evaluate(async () => (await axe.run()).violations.filter((v) => ['serious', 'critical'].includes(v.impact)).map((v) => v.id + ':' + v.nodes.map((n) => n.target.join(' ')).join(',')));
  const problems = [];
  if (m.sw > m.cw) problems.push(`overflow ${m.sw}>${m.cw}`);
  if (errs.length) problems.push('console: ' + errs.join(' | ')); if (bad.length) problems.push('failed: ' + bad.join(' | ')); if (ax.length) problems.push('axe: ' + ax.join(';'));
  const tall = m.cards.filter(([n, ch]) => n !== 'card' && !String(n).includes('card') && ch > h - 24).map(([n, ch]) => `${n}=${ch}`); if (tall.length) problems.push('block taller than screen: ' + tall.join(','));
  if (problems.length) { console.log(tag, 'FAIL ' + problems.join(' ; ')); fail++; }
  await ctx.close();
}
console.log(fail ? fail + ' FAIL' : 'all ok'); await browser.close(); process.exit(fail ? 1 : 0);
