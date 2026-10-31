// Models are the same pinned ONNX files the desktop downloads (URL and SHA-256 from panoply-vision/panoply-ocr), fetched once
// with progress, verified, and kept in the browser's Cache Storage so the next visit starts at once.
export const MODELS = {
  yunet: { name: 'YuNet face detector', url: 'https://media.githubusercontent.com/media/opencv/opencv_zoo/26cc381e4d2594bb9f47a26eb8fd96c94a13660d/models/face_detection_yunet/face_detection_yunet_2023mar.onnx', sha256: '8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4', size: 232589, license: 'MIT' },
  esrgan: { name: 'Real-ESRGAN upscale model', size: 4997183, license: 'BSD-3-Clause', files: [{ name: 'real_esrgan_general_x4v3.onnx', url: new URL('./models/esrgan/real_esrgan_general_x4v3.onnx', import.meta.url).href, sha256: '09e6b675a0a18a97e1057c8dd7e984ef878a0a02ce104430da525b6241c858e7' }, { name: 'real_esrgan_general_x4v3.data', url: new URL('./models/esrgan/real_esrgan_general_x4v3.data', import.meta.url).href, sha256: '512d0ec9940c2e9d85d27f2952f12a0b77b7841dc22df4ce9f3ea458bc98f37f' }] },
  isnet: { name: 'IS-Net background model', url: 'https://huggingface.co/xrds/isnet-general-onnx-int8/resolve/71eff2372ec9c8edbc6ca637ded591423d23b65a/onnx/model_quantized.onnx', sha256: '3b21a6706dc8d6e4ba9f5b31ebc6940f6c785b58862e27bb25daa9dd4424b87f', size: 44229662, license: 'MIT' },
  migan: { name: 'MI-GAN fill model', url: 'https://huggingface.co/andraniksargsyan/migan/resolve/406830d0fa60666da0071c342ad2fbc8f30c5c64/migan_pipeline_v2.onnx', sha256: '6f1f3530a1a2324b19752018ce756088b07973cda8d7d890034ace5c8a48c40b', size: 28079181, license: 'MIT' },
  ppocr_det: { name: 'PP-OCRv5 text detector', url: 'https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.onnx', sha256: 'a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d', license: 'Apache-2.0' },
  ppocr_en: { name: 'PP-OCRv5 English recognizer', url: 'https://huggingface.co/PaddlePaddle/en_PP-OCRv5_mobile_rec_onnx/resolve/3fafbc3b5dcf93dd72add9f48368be8a3a2cd33b/inference.onnx', sha256: 'b5f833dfc5d0eb71da397b4efa06ebeee9b431b690a47d6af40d77d8eabc557f', license: 'Apache-2.0' },
  ppocr_latin: { name: 'PP-OCRv5 Latin recognizer', url: 'https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.onnx', sha256: '7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498', license: 'Apache-2.0' },
  ppocr_deva: { name: 'PP-OCRv5 Devanagari recognizer', url: 'https://huggingface.co/PaddlePaddle/devanagari_PP-OCRv5_mobile_rec_onnx/resolve/251aec19e36739540d35e2cc943f6aa7503b98e5/inference.onnx', sha256: 'cb789212ce96c69d3e74728ae4309d179281d68cb3945d0616b67cafab41c986', license: 'Apache-2.0' },
};
const CACHE = 'panoply-models-v1';
const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');
async function loadOne(m, onProgress) {
  const cache = await caches.open(CACHE).catch(() => null);
  const hit = cache && await cache.match(m.url);
  if (hit) { onProgress(1, true); return new Uint8Array(await hit.arrayBuffer()); }
  const res = await fetch(m.url); if (!res.ok) throw new Error(`The ${m.name} could not be downloaded (${res.status}).`);
  const total = +res.headers.get('content-length') || m.size || 0, reader = res.body.getReader(); let got = 0, buf = new Uint8Array(total || 1 << 26);
  for (;;) { // written straight into one buffer: no second copy of the model while downloading
    const { done, value } = await reader.read(); if (done) break;
    if (got + value.length > buf.length) { const n = new Uint8Array(Math.max(buf.length * 2, got + value.length)); n.set(buf.subarray(0, got)); buf = n; }
    buf.set(value, got); got += value.length; onProgress(total ? got / total : 0, false);
  }
  buf = buf.subarray(0, got);
  if (hex(await crypto.subtle.digest('SHA-256', buf)) !== m.sha256) throw new Error(`The ${m.name} download did not match its checksum, so it was thrown away. Try again.`);
  if (cache) await cache.put(m.url, new Response(buf, { headers: { 'content-type': 'application/octet-stream' } })).catch(() => {});
  return buf;
}
export async function loadModel(key, onProgress = () => {}) { return loadOne(MODELS[key], onProgress); }
// Models that come as several files (an .onnx and its external weights): returns {name: bytes}.
export async function loadModelFiles(key, onProgress = () => {}) {
  const m = MODELS[key], out = {}; let i = 0;
  for (const f of m.files) { out[f.name] = await loadOne({ ...f, name: m.name }, (p, c) => onProgress((i + p) / m.files.length, c)); i++; }
  return out;
}
export async function isCached(key) { try { const m = MODELS[key], c = await caches.open(CACHE); return (await Promise.all((m.files || [m]).map((f) => c.match(f.url)))).every(Boolean); } catch { return false; } }
