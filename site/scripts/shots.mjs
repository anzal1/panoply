// Breakpoint check: screenshots every width at every key scroll moment, and fails on overflow,
// pieces outside the frame or a lopsided hero. Run before showing or merging any visual change.
//   node scripts/shots.mjs <url> [outDir]
import { chromium } from 'playwright-core';
import fs from 'node:fs';

const url = process.argv[2] || 'http://localhost:4321/';
const out = process.argv[3] || 'shots';
const SIZES = [[375, 812], [390, 844], [768, 1024], [1024, 768], [1280, 800], [1440, 900], [1920, 1080], [2560, 1440]];
const MOMENTS = {
  top: '0',
  heroEnd: "document.querySelector('.hero').offsetTop + document.querySelector('.hero').offsetHeight - innerHeight",
  how: "document.getElementById('how').offsetTop + (document.getElementById('how').offsetHeight - innerHeight) * .5",
  set: "document.querySelector('.bento').getBoundingClientRect().top + scrollY - 80",
  privacy: "document.getElementById('privacy').offsetTop + 200",
  end: 'document.body.scrollHeight',
};

fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--hide-scrollbars', '--mute-audio'] });
const problems = [];
for (const [w, h] of SIZES) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on('response', (r) => r.status() >= 400 && !/favicon\.ico$/.test(r.url()) && errors.push(`${r.status()} ${r.url()}`));
  await page.goto(url, { waitUntil: 'networkidle' });
  await page.waitForTimeout(2200);
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
  await page.close();
}
await browser.close();
console.log(problems.length ? `\nFAIL\n- ${problems.join('\n- ')}` : '\nPASS: no overflow, balanced hero, no pieces cut, no console errors');
process.exitCode = problems.length ? 1 : 0;
