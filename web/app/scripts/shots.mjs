// Breakpoint pass: 375..2560 with real heights, result state, overflow, console errors, failed requests, axe, block-fits-screen.
import { chromium } from 'playwright-core'; import fs from 'node:fs'; import path from 'node:path';
const url = process.argv[2] || 'http://localhost:8099', dir = process.argv[3] || 'shots';
fs.mkdirSync(dir, { recursive: true });
const sizes = [[375, 812], [390, 844], [768, 1024], [1024, 768], [1280, 800], [1440, 900], [1920, 1080], [2560, 1440]];
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const axeSrc = fs.readFileSync('node_modules/axe-core/axe.min.js', 'utf8');
const browser = await chromium.launch({ executablePath: exe, args: ['--mute-audio'] });
let fail = 0;
const pages = [
  ['', null], ['tools', null],
  ['tools/resize-to-kb', async (p) => { await p.getByRole('radio', { name: 'SSC', exact: true }).click(); }],
  ['tools/resize-to-kb/ssc-signature', async () => {}],
  ['tools/resize', async (p) => { await p.fill('#w', '800'); }], ['tools/compress', async () => {}], ['tools/convert', async (p) => { await p.selectOption('#fmt', 'webp'); }], ['tools/strip-metadata', async () => {}],
  ['tools/pdf-merge', 'pdf', async () => {}], ['tools/pdf-split', 'pdf', async (p) => { await p.selectOption('#mode', 'ranges'); await p.fill('#ranges', '1-2, last'); }], ['tools/pdf-organize', 'pdf', async (p) => { await p.fill('#del', '2'); await p.check('#rev'); }], ['tools/pdf-compress', 'pdf', async () => {}], ['tools/pdf-to-images', 'pdf', async (p) => { await p.fill('#pages', '1'); }], ['tools/ocr-pdf', 'pdf', async () => {}],
  ['tools/remove-background', async () => {}], ['tools/ocr', async () => {}], ['tools/upscale', async (p) => { await p.selectOption('#scale', '2'); }],
  ['tools/erase', async (p) => { await p.waitForSelector('#editor:not([hidden])'); await p.waitForTimeout(200); const b = await p.locator('#paint').boundingBox(); await p.mouse.move(b.x + b.width * 0.2, b.y + b.height * 0.55); await p.mouse.down(); await p.mouse.move(b.x + b.width * 0.3, b.y + b.height * 0.6, { steps: 6 }); await p.mouse.up(); }],
  ['tools/remove-watermark', async (p) => { await p.check('#own'); }], ['tools/passport-photo/us-passport', async () => {}], ['tools/signature', async (p) => { await p.selectOption('#preset', 'ssc-signature'); }], ['tools/print-sheet', async () => {}], ['tools/images-to-pdf', 'imgs', async () => {}], ['tools/media', 'media', async () => {}],
];
const F = (n) => path.resolve('../fixtures/' + n);
const inputFor = (pg) => pg.endsWith('images-to-pdf') ? [F('portrait.jpg'), F('small-portrait.png')] : pg.endsWith('media') ? F('test.mp4') : pg.endsWith('signature') ? F('signature-photo.jpg') : pg.endsWith('pdf-merge') ? [F('pdf/hebrew-parable.pdf'), F('pdf/f1040.pdf')] : pg.endsWith('ocr-pdf') ? F('pdf/scan.pdf') : pg.includes('pdf') && (!pg.includes('images') || pg.endsWith('pdf-to-images')) ? F('pdf/hebrew-parable.pdf') : pg.endsWith('remove-background') || pg.endsWith('erase') || pg.endsWith('print-sheet') ? F('portrait.jpg') : pg.endsWith('ocr') ? F('f1040-page.png') : pg.endsWith('upscale') ? F('small-portrait.png') : pg.includes('passport') ? F('portrait-big.jpg') : pg.endsWith('remove-watermark') ? F('wm-corner.png') : F('natural.jpg');
const modelFor = (u) => u.includes('huggingface.co') ? (u.includes('isnet') ? 'isnet.onnx' : u.includes('PP-OCRv5_mobile_det') ? 'ppocr_det.onnx' : u.includes('en_PP-OCR') ? 'ppocr_en.onnx' : u.includes('migan') ? 'migan.onnx' : null) : u.includes('media.githubusercontent.com') ? 'yunet.onnx' : null;
const base = url.replace(/\/$|\/index\.html$/, '');
for (const scheme of ['dark']) for (const [w, h] of sizes) for (const entry of pages.filter((e) => !process.env.ONLY || process.env.ONLY.split(',').includes(e[0]))) {
  const pg = entry[0], kind = typeof entry[1] === 'string' ? entry[1] : '', isPdf = kind === 'pdf', setup = kind ? entry[2] : entry[1];
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, colorScheme: scheme, deviceScaleFactor: 1 }), page = await ctx.newPage();
  if (/(passport-photo|remove-background|\/ocr|ocr-pdf|upscale|erase|remove-watermark)/.test(pg) && ![375, 768, 1440, 2560].includes(w)) { await ctx.close(); continue; }
  const errs = [], bad = [];
  await page.route('**/*', (r) => { const f = modelFor(r.request().url()); return f ? r.fulfill({ status: 200, headers: { 'access-control-allow-origin': '*' }, body: fs.readFileSync('/tmp/pdfout/' + f) }) : r.continue(); });
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text())); page.on('pageerror', (e) => errs.push(String(e)));
  page.on('requestfailed', (r) => { const e = r.failure() && r.failure().errorText; if (e === 'net::ERR_ABORTED' && r.url().includes('/models/')) return; /* Chromium reports a fully read, hash-checked model fetch as aborted now and then */ bad.push(r.url() + ' ' + e); }); page.on('response', (r) => r.status() >= 400 && bad.push(r.status() + ' ' + r.url()));
  await page.goto(`${base}/${pg}${pg ? '/' : ''}`);
  if (pg.startsWith('tools/')) await page.waitForFunction(() => crossOriginIsolated, null, { timeout: 15000 }).catch(() => {}); // the service worker reloads the page once

  const tag = `${scheme}-${w}x${h}-${pg || 'home'}`;
  if (setup) {
    await page.waitForFunction(() => { const s = document.querySelector('#preset'); return !s || !s.disabled; });
    await page.screenshot({ path: path.join(dir, `${tag}-empty.png`) });
    const auto = await page.evaluate(() => document.querySelector('.tool-app').classList.contains('auto'));
    if (auto) { await page.evaluate(() => { const t = document.querySelector('#tune'); if (t) { t.open = true; document.querySelector('.tool-app').classList.add('more'); } }); await setup(page); await page.setInputFiles('#file', inputFor(pg)); }
    else { await page.setInputFiles('#file', inputFor(pg)); await page.waitForTimeout(isPdf || kind ? 600 : 0); await setup(page); await page.waitForFunction(() => { const g = document.querySelector('#go'); return g && !g.disabled; }, null, { timeout: 60000 }); await page.click('#go'); }
    await page.waitForSelector('#result .rcard, #result .card.bad', { timeout: 120000 }); await page.waitForFunction(() => new Promise((ok) => { let y = scrollY, n = 0; const t = setInterval(() => { n = scrollY === y ? n + 1 : 0; y = scrollY; if (n >= 4) { clearInterval(t); ok(true); } }, 100); }), null, { timeout: 15000 }).catch(() => {}); // the result scrolls into view; wait for it to stop
    await page.evaluate(() => { const t = document.querySelector('#tune'); if (t) t.open = false; }); await page.waitForTimeout(400);
  }
  await page.screenshot({ path: path.join(dir, `${tag}-result.png`) });
  const m = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth, sh: document.documentElement.scrollHeight, cards: [...document.querySelectorAll('.card, #drop, .tool')].map((e) => [e.id || e.className, Math.round(e.getBoundingClientRect().height)]) }));
  if (m.sh > h) { await page.screenshot({ path: path.join(dir, `${tag}-full.png`), fullPage: true }); }
  if (!pg) { const H = await page.evaluate(() => document.documentElement.scrollHeight); for (let y = 0; y < H; y += Math.round(h / 2)) { await page.evaluate((v) => scrollTo(0, v), y); await page.waitForTimeout(120); } await page.evaluate(() => scrollTo(0, 0)); await page.emulateMedia({ reducedMotion: 'reduce' }); await page.waitForTimeout(1500); } // home sections fade in on scroll; axe must see the settled state
  await page.addScriptTag({ content: axeSrc });
  const runAxe = () => page.evaluate(async () => (await axe.run()).violations.filter((v) => ['serious', 'critical'].includes(v.impact)).map((v) => v.id + ':' + v.nodes.map((n) => n.target.join(' ')).join(',')));
  let ax = await runAxe(); for (let t = 0; t < 4 && ax.length && ax.every((x) => x.startsWith('color-contrast:')); t++) { await page.waitForTimeout(1500); ax = await runAxe(); } // the hero demo loops; a real contrast bug persists, a mid-fade frame does not
  const problems = [];
  if (setup) { const dl = await page.evaluate(() => { const a = document.querySelector('#result a.dl'); if (!a) return null; const r = a.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight; }); if (dl === false) problems.push('Download button is below the fold'); if (dl === null && !(await page.locator('#result .card.bad').count())) problems.push('no Download button'); }
  if (m.sw > m.cw) problems.push(`overflow ${m.sw}>${m.cw}`);
  if (errs.length) problems.push('console: ' + errs.join(' | ')); if (bad.length) problems.push('failed: ' + bad.join(' | ')); if (ax.length) problems.push('axe: ' + ax.join(';'));
  const tall = m.cards.filter(([n, ch]) => n !== 'card' && !String(n).includes('card') && ch > h - 24).map(([n, ch]) => `${n}=${ch}`); if (tall.length) problems.push('block taller than screen: ' + tall.join(','));
  if (problems.length) { console.log(tag, 'FAIL ' + problems.join(' ; ')); fail++; }
  await ctx.close();
}
console.log(fail ? fail + ' FAIL' : 'all ok'); await browser.close(); process.exit(fail ? 1 : 0);
