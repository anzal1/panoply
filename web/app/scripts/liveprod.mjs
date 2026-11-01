// Loads the live site headless: console errors, failed requests, and one real resize end to end.
import { chromium } from 'playwright-core';
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const b = await chromium.launch({ executablePath: exe, args: ['--mute-audio'] });
const base = 'https://panoply.anzalabidi.dev';
for (const [pg, run] of [['/tools/', 0], ['/tools/resize-to-kb/', 1], ['/tools/passport-photo/', 0]]) {
  const ctx = await b.newContext({ viewport: { width: 1280, height: 800 } }), p = await ctx.newPage(), bad = [], errs = [];
  p.on('console', (m) => m.type() === 'error' && errs.push(m.text())); p.on('pageerror', (e) => errs.push(String(e)));
  p.on('requestfailed', (r) => bad.push(r.url() + ' ' + (r.failure() || {}).errorText)); p.on('response', (r) => r.status() >= 400 && bad.push(r.status() + ' ' + r.url()));
  await p.goto(base + pg); await p.waitForFunction(() => crossOriginIsolated, null, { timeout: 20000 }).catch(() => {}); await p.waitForTimeout(2500);
  const iso = await p.evaluate(() => crossOriginIsolated);
  let res = '';
  if (run) {
    await p.setInputFiles('#file', '../fixtures/natural.jpg'); await p.waitForTimeout(500);
    await p.waitForFunction(() => !document.querySelector('#go').disabled, null, { timeout: 30000 });
    const t = Date.now(); await p.click('#go'); await p.waitForSelector('#result .big', { timeout: 120000 });
    res = await p.evaluate(() => document.querySelector('#result .big').textContent) + ' in ' + (Date.now() - t) + ' ms, preset ' + await p.evaluate(() => document.querySelector('#preset').value);
  }
  console.log(pg, 'isolated', iso, 'errors', JSON.stringify(errs), 'bad', JSON.stringify(bad), res);
  await ctx.close();
}
await b.close();
