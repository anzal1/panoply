// Accessibility audit with axe-core in headless Chrome, at a phone and a desktop width.
//   node scripts/audit.mjs <url>
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import { createRequire } from 'node:module';
const axe = fs.readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');
const url = process.argv[2] || 'http://localhost:4321/';
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--mute-audio'] });
let fails = 0;
for (const [w, h] of [[390, 844], [1440, 900]]) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  await page.goto(url, { waitUntil: 'networkidle' });
  // reveal everything so the audit sees the finished page
  await page.evaluate(async () => { for (let y = 0; y < document.body.scrollHeight; y += 400) { scrollTo(0, y); dispatchEvent(new Event('scroll')); await new Promise((r) => setTimeout(r, 30)); } scrollTo(0, 0); });
  await page.waitForTimeout(4000); // let typing and fades finish
  await page.addScriptTag({ content: axe });
  const r = await page.evaluate(async () => (await axe.run(document, { resultTypes: ['violations'] })).violations.map((v) => ({ id: v.id, impact: v.impact, n: v.nodes.length, help: v.help, where: v.nodes.slice(0, 3).map((n) => n.target.join(' ')) })));
  console.log(`${w}x${h}: ${r.length ? r.length + ' violation types' : 'no violations'}`);
  for (const v of r) console.log(`  [${v.impact}] ${v.id} x${v.n}: ${v.help}\n     ${v.where.join(' | ')}`);
  fails += r.filter((v) => v.impact === 'serious' || v.impact === 'critical').length;
  await page.close();
}
await browser.close();
process.exitCode = fails ? 1 : 0;
