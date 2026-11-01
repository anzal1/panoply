// Downloads the Real-ESRGAN files (the host sends no CORS headers, so the site serves them itself) and checks the pinned hashes.
import fs from 'node:fs'; import crypto from 'node:crypto'; import { execSync } from 'node:child_process'; import path from 'node:path'; import os from 'node:os';
const ZIP = 'https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/real_esrgan_general_x4v3/releases/v0.64.0/real_esrgan_general_x4v3-onnx-float.zip';
const WANT = { 'real_esrgan_general_x4v3.onnx': '09e6b675a0a18a97e1057c8dd7e984ef878a0a02ce104430da525b6241c858e7', 'real_esrgan_general_x4v3.data': '512d0ec9940c2e9d85d27f2952f12a0b77b7841dc22df4ce9f3ea458bc98f37f' };
export default function fetchModels(dir) { return run(dir); }
function run(dir) {
const out = dir || 'public/models/esrgan', sha = (p) => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
fs.mkdirSync(out, { recursive: true });
if (Object.entries(WANT).every(([n, h]) => fs.existsSync(path.join(out, n)) && sha(path.join(out, n)) === h)) { console.log('esrgan files present'); return; }
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'esr-')); execSync(`curl -sL -o ${tmp}/m.zip "${ZIP}" && unzip -o -q ${tmp}/m.zip -d ${tmp}`);
for (const [n, h] of Object.entries(WANT)) { const p = path.join(tmp, 'real_esrgan_general_x4v3-onnx-float', n); if (sha(p) !== h) throw new Error('hash mismatch for ' + n); fs.copyFileSync(p, path.join(out, n)); }
console.log('esrgan files written');
}
if (process.argv[1] && process.argv[1].endsWith('fetch-models.mjs')) run();
