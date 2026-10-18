// Records each immersive concept on CI (software WebGL). Never run this on the Mac.
import { chromium } from 'playwright';
import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';

const BASE = process.env.BASE || 'http://localhost:8080';
const OUT = path.resolve(process.env.OUT || '../../out');
const CONCEPTS = (process.env.ONLY || '05-panoply,06-fan,07-never-leaves').split(',');
const sleep = ms => new Promise(r => setTimeout(r, ms));
fs.mkdirSync(OUT + '/raw', { recursive: true }); fs.mkdirSync(OUT + '/logs', { recursive: true });

const ARGS = ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist', '--enable-webgl', '--disable-dev-shm-usage', '--autoplay-policy=no-user-gesture-required'];

async function scrollTo(page, p) { await page.evaluate(p => window.scrollTo(0, (document.documentElement.scrollHeight - innerHeight) * p), p); }
async function scrollSweep(page, a, b, ms, steps = 24) { for (let i = 0; i <= steps; i++) { await scrollTo(page, a + (b - a) * i / steps); await sleep(ms / steps); } }
async function dropFile(page) {
  await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 900; c.height = 600; const x = c.getContext('2d');
    const g = x.createLinearGradient(0, 0, 0, 600); g.addColorStop(0, '#9DC2E8'); g.addColorStop(.6, '#E8D9B0'); g.addColorStop(1, '#4E7F53'); x.fillStyle = g; x.fillRect(0, 0, 900, 600);
    x.fillStyle = '#FFE9A8'; x.beginPath(); x.arc(640, 170, 70, 0, 7); x.fill();
    const blob = await new Promise(r => c.toBlob(r, 'image/jpeg', .9));
    const f = new File([blob], 'photo.jpg', { type: 'image/jpeg' });
    const dt = new DataTransfer(); dt.items.add(f);
    window.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
}

async function run(browser, name, phone) {
  const tag = `${name}-${phone ? 'phone' : 'desktop'}`;
  const size = phone ? { width: 390, height: 844 } : { width: 1440, height: 900 };
  const ctx = await browser.newContext({ viewport: size, deviceScaleFactor: 1, isMobile: phone, hasTouch: phone, recordVideo: { dir: OUT + '/raw/' + tag, size } });
  const page = await ctx.newPage();
  const log = [];
  page.on('console', m => log.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', e => log.push(`[pageerror] ${e.message}`));
  page.on('requestfailed', r => log.push(`[requestfailed] ${r.url()}`));
  const shot = async n => page.screenshot({ path: `${OUT}/${tag}-${n}.png` }).catch(e => log.push('[shot-fail] ' + e.message));
  const t0 = Date.now(); const at = s => sleep(Math.max(0, s * 1000 - (Date.now() - t0)));
  try {
    await page.goto(`${BASE}/${name}/index.html?rec=1`, { waitUntil: 'load' });
    await page.waitForSelector('html[data-ready="1"]', { timeout: 90000 }).catch(e => log.push('[not-ready] ' + e.message));
    const t1 = Date.now(); const rel = s => sleep(Math.max(0, s * 1000 - (Date.now() - t1)));
    await rel(1.5); await shot('1-load');
    if (phone) {
      await scrollSweep(page, 0, 0.3, 2000, 10); await shot('2-scroll');
      await page.click('input[type=text]').catch(() => {}); await page.keyboard.type('under 50 kb', { delay: 120 });
      await rel(7); await shot('3-typed'); await rel(8.5);
    } else {
      if (name === '06-fan') {
        for (const [i, p] of [[2, 0.18], [3, 0.36], [4, 0.58], [5, 0.9]]) { await scrollSweep(page, i === 2 ? 0 : [0, 0, .18, .36, .58][i - 1], p, 1600, 12); await sleep(900); await shot(`${i}-p${Math.round(p * 100)}`); }
        await scrollSweep(page, 0.9, 1, 800, 6); await sleep(4500); await shot('6-end');
        await scrollTo(page, 0.5);
      } else {
        await scrollSweep(page, 0, 0.1, 1500, 8); await scrollTo(page, 0);
      }
      await page.click('input[type=text]').catch(() => {}); await page.keyboard.type('under 50 kb', { delay: 130 });
      await sleep(2500); await shot('7-typed');
      await dropFile(page); await sleep(2200); await shot('8-dropping'); await sleep(4500); await shot('9-docked');
    }
  } catch (e) { log.push('[script-error] ' + e.message); }
  const vid = page.video();
  await ctx.close();
  fs.writeFileSync(`${OUT}/logs/${tag}.log`, log.join('\n') + '\n');
  try {
    const webm = await vid.path();
    const mp4 = `${OUT}/${tag}.mp4`;
    execSync(`ffmpeg -y -loglevel error -i "${webm}" -c:v libx264 -pix_fmt yuv420p -movflags +faststart -r 30 "${mp4}"`);
    const dur = parseFloat(execSync(`ffprobe -v error -show_entries format=duration -of csv=p=0 "${mp4}"`).toString());
    for (const [i, f] of [[1, 0.35], [2, 0.8]]) execSync(`ffmpeg -y -loglevel error -ss ${(dur * f).toFixed(2)} -i "${mp4}" -frames:v 1 "${OUT}/${tag}-frame${i}.jpg"`);
    console.log(tag, 'duration', dur.toFixed(1), 's');
  } catch (e) { console.log('convert failed', tag, e.message); }
}

const browser = await chromium.launch({ args: ARGS });
for (const n of CONCEPTS) { await run(browser, n, false); await run(browser, n, true); }
await browser.close();
fs.rmSync(OUT + '/raw', { recursive: true, force: true });
