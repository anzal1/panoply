// Renders the nine style frames, silent and headless. No window opens, no audio plays.
//   node docs/launch/style-frames/render.mjs            (uses the Playwright you have installed)
//   CHROME=/path/to/chrome node ... render.mjs          (optional, else Playwright's own Chromium)
// Output: docs/launch/style-frames/<option>-<scene>.png at 1920x1080, then contact-sheet.png.
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(process.env.PW_FROM || import.meta.url);
const { chromium } = require(process.env.PW_PKG || 'playwright-core');

const OPTIONS = { a: 'midnight', b: 'paper', c: 'fan' };
const SCENES = ['pain', 'beat', 'end'];
const scale = Number(process.env.SCALE || 1);

const browser = await chromium.launch({
  executablePath: process.env.CHROME || undefined,
  headless: true,
  args: ['--mute-audio', '--force-color-profile=srgb', '--disable-gpu'],
});
const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: scale });
const page = await ctx.newPage();
page.on('pageerror', (e) => console.error('pageerror:', e.message));
page.on('console', (m) => m.type() === 'error' && console.error('console:', m.text()));
const url = pathToFileURL(join(here, 'frames.html')).href;
for (const [o, name] of Object.entries(OPTIONS)) {
  for (const s of SCENES) {
    await page.goto(`${url}?opt=${o}&scene=${s}`);
    await page.waitForSelector('body[data-ready="1"]');
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(150);
    const out = join(here, `${o}-${name}-${s}.png`);
    await page.screenshot({ path: out });
    console.log('wrote', out);
  }
}
await browser.close();
