// Breakpoint check: screenshots every width at every key scroll moment, and fails on overflow,
// pieces outside the frame or a lopsided hero. Run before showing or merging any visual change.
//   node scripts/shots.mjs <url> [outDir]
import { chromium } from 'playwright-core';
import fs from 'node:fs';

const url = process.argv[2] || 'http://localhost:4321/';
const out = process.argv[3] || 'shots';
const SIZES = [[375, 812], [390, 844], [768, 1024], [1024, 768], [1280, 800], [1440, 900], [1920, 1080], [2560, 1440]];
const top = (sel, off = 52) => `document.querySelector('${sel}').getBoundingClientRect().top + scrollY - ${off}`;
const MOMENTS = {
  top: '0',
  heroEnd: "document.querySelector('.hero').offsetTop + document.querySelector('.hero').offsetHeight - innerHeight",
  statement: top('.statement'),
  how: "document.getElementById('how').offsetTop + (document.getElementById('how').offsetHeight - innerHeight) * .5",
  set: top('.bento', 68),
  set2: top('.tile:nth-child(3)', 68),
  set3: top('.tile:nth-child(5)', 68),
  app: top('.app'),
  speed: top('.numbers'),
  privacy: top('.privacy'),
  agents: top('.agent-tile', 68),
  get: top('.get'),
  downloads: top('.downloads'),
  downloads2: top('.extras', 68),
  end: 'document.body.scrollHeight',
};
// Every block that is read as one piece must fit in the visible screen under the 52px nav.
const UNITS = ['.statement p', '.set header', '.tile', '.app header', '.app-stage', '.numbers header', '.stats', '.privacy header', '.privacy figure', '.agent-tile', '.get .wrap > *', '.downloads header', '.plat', '.how .steps', '.screen-bar'];

fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--hide-scrollbars', '--mute-audio'] });
const problems = [];
for (const [w, h] of SIZES) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on('response', (r) => r.status() >= 400 && !/favicon\.ico$/.test(r.url()) && errors.push(`${r.status()} ${r.url()}`));
  let bytes = 0;
  page.on('response', async (r) => { try { bytes += (await r.body()).length; } catch {} });
  await page.goto(url, { waitUntil: 'networkidle' });
  await page.waitForTimeout(2200);
  const fit = await page.evaluate((units) => {
    const room = innerHeight - 52, bad = [];
    for (const sel of units) for (const el of document.querySelectorAll(sel)) {
      if (getComputedStyle(el).position === 'absolute' || el.getAttribute('aria-hidden') === 'true') continue; // decoration, not content
      const h = Math.round(el.getBoundingClientRect().height);
      if (h > room + 1) bad.push(`${sel} is ${h}px, screen has ${room}px`);
    }
    const b = document.getElementById('bar').getBoundingClientRect();
    if (b.bottom > innerHeight + 1) bad.push(`first screen cuts the command bar by ${Math.round(b.bottom - innerHeight)}px`);
    return bad;
  }, UNITS);
  fit.forEach((f) => problems.push(`${w}x${h}: ${f}`));
  for (const [name, expr] of Object.entries(MOMENTS)) {
    await page.evaluate(`window.scrollTo({ top: ${expr}, behavior: 'instant' }); dispatchEvent(new Event('scroll'))`);
    await page.waitForTimeout(name === 'top' ? 300 : 1300);
    await page.screenshot({ path: `${out}/${w}x${h}-${name}.png` });
    const r = await page.evaluate(() => {
      const vw = innerWidth, cx = vw / 2, res = { overflow: document.documentElement.scrollWidth - vw };
      const pcs = [...document.querySelectorAll('.hero .pc')].map((e) => e.getBoundingClientRect()).filter((b) => b.width > 2 && b.bottom > 0 && b.top < innerHeight);
      if (pcs.length) {
        const left = pcs.filter((b) => b.left + b.width / 2 < cx), right = pcs.filter((b) => b.left + b.width / 2 >= cx);
        const reachL = cx - Math.min(...pcs.map((b) => b.left)), reachR = Math.max(...pcs.map((b) => b.right)) - cx;
        res.hero = { left: left.length, right: right.length, reachL: Math.round(reachL), reachR: Math.round(reachR), outside: pcs.filter((b) => b.left < 0 || b.right > vw).length };
      }
      return res;
    });
    const tag = `${w}x${h} ${name}`;
    if (r.overflow > 0) problems.push(`${tag}: page is ${r.overflow}px wider than the screen`);
    if (name === 'heroEnd' && r.hero) {
      const { left, right, reachL, reachR, outside } = r.hero;
      if (Math.abs(left - right) > 2) problems.push(`${tag}: hero pieces ${left} left vs ${right} right`);
      if (Math.max(reachL, reachR) / Math.max(1, Math.min(reachL, reachR)) > 1.3) problems.push(`${tag}: hero reach ${reachL}px left vs ${reachR}px right`);
      if (outside) problems.push(`${tag}: ${outside} hero pieces cut by the screen edge`);
      console.log(`${tag}  pieces L${left}/R${right}  reach ${reachL}/${reachR}px  outside ${outside}`);
    }
  }
  if (errors.length) problems.push(`${w}x${h}: console errors: ${errors.join(' | ')}`);
  if (w === 1440) console.log(`page weight at 1440: ${(bytes / 1024).toFixed(0)} KB`);
  await page.close();
}
await browser.close();
console.log(problems.length ? `\nFAIL\n- ${problems.join('\n- ')}` : '\nPASS: every block fits the screen, no overflow, balanced hero, no pieces cut, no console errors or failed requests');
process.exitCode = problems.length ? 1 : 0;
