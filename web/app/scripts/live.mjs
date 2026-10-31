// Checks the built site the way GitHub Pages serves it (no special headers): the service worker must make the page cross-origin
// isolated after one reload, and the tools must work with it and without it.   node scripts/live.mjs http://localhost:8099
import { chromium } from 'playwright-core'; import path from 'node:path';
const base = process.argv[2] || 'http://localhost:8099', fx = (n) => path.resolve('../fixtures/' + n);
const exe = process.env.HOME + '/Library/Caches/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-mac-arm64/chrome-headless-shell';
const b = await chromium.launch({ executablePath: exe });
async function isolated(label, opts) {
  const ctx = await b.newContext({ acceptDownloads: true, ...opts }), p = await ctx.newPage(); let loads = 0; p.on('load', () => loads++);
  const errs = []; p.on('pageerror', (e) => errs.push(String(e))); p.on('requestfailed', (r) => errs.push('req ' + r.url().split('/').slice(-2).join('/')));
  await p.goto(base + '/tools/strip-metadata/'); await p.waitForTimeout(opts && opts.serviceWorkers === 'block' ? 1500 : 4000);
  const iso = await p.evaluate(() => ({ iso: crossOriginIsolated, sab: typeof SharedArrayBuffer, ctrl: !!navigator.serviceWorker && !!navigator.serviceWorker.controller }));
  console.log(label, JSON.stringify(iso), 'page loads', loads, errs.length ? errs : ''); await ctx.close(); return iso;
}
await isolated('with the service worker:', {}); await isolated('service workers blocked:', { serviceWorkers: 'block' });
await b.close();
