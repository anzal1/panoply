// Renders the sharing card (public/og.png) from the hero's settled frame at 1200x630.
//   node scripts/og.mjs <url>
import { chromium } from 'playwright-core';
const url = process.argv[2] || 'http://localhost:4321/';
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
await page.goto(url, { waitUntil: 'networkidle' });
await page.waitForTimeout(2000);
await page.evaluate(() => { const h = document.querySelector('.hero'); scrollTo({ top: h.offsetTop + h.offsetHeight - innerHeight, behavior: 'instant' }); dispatchEvent(new Event('scroll')); });
await page.waitForTimeout(1200);
await page.addStyleTag({ content: '.nav{display:none}.hero-tag{bottom:5vh}.hero-art{margin-top:-9vh}.hero-tag p{font-size:40px}' });
await page.evaluate(() => { document.querySelector('.hero-tag p').innerHTML = 'Every file tool. One app.<br><span>Panoply. Free, open source, on your computer.</span>'; });
await page.screenshot({ path: 'public/og.png' });
await browser.close();
console.log('wrote public/og.png');
