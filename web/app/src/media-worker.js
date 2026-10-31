// Video and audio on WebCodecs (hardware encoders first) through mediabunny: no ffmpeg download, no upload.
import init, { LoudMeter } from './mediawasm/panoply_web_media.js';
import mediaUrl from './mediawasm/panoply_web_media_bg.wasm';
import { AudioSample, AudioSampleSink, Input, Output, Conversion, BlobSource, BufferTarget, ALL_FORMATS, Mp4OutputFormat, WebMOutputFormat, WavOutputFormat, canEncodeVideo, canEncodeAudio, QUALITY_VERY_LOW, QUALITY_LOW, QUALITY_MEDIUM, QUALITY_HIGH } from 'mediabunny';
const ready = init({ module_or_path: mediaUrl });
const Q = { low: QUALITY_LOW, medium: QUALITY_MEDIUM, high: QUALITY_HIGH, tiny: QUALITY_VERY_LOW };
async function capabilities() {
  return { h264: await canEncodeVideo('avc').catch(() => false), vp9: await canEncodeVideo('vp9').catch(() => false), av1: await canEncodeVideo('av1').catch(() => false), aac: await canEncodeAudio('aac').catch(() => false), opus: await canEncodeAudio('opus').catch(() => false) };
}
async function info(blob) {
  const input = new Input({ source: new BlobSource(blob), formats: ALL_FORMATS });
  const dur = await input.computeDuration(), v = await input.getPrimaryVideoTrack(), a = await input.getPrimaryAudioTrack();
  return { duration: dur, format: (await input.getFormat()).name, video: v ? { codec: v.codec, w: v.displayWidth, h: v.displayHeight, rotation: v.rotation } : null, audio: a ? { codec: a.codec, channels: a.numberOfChannels, rate: a.sampleRate } : null, size: blob.size, kbps: dur ? Math.round(blob.size * 8 / dur / 1000) : 0 };
}
async function convert(m, post) {
  const cap = await capabilities(), input = new Input({ source: new BlobSource(m.blob), formats: ALL_FORMATS });
  const target = new BufferTarget(), mp4 = m.container === 'mp4' && cap.h264 && cap.aac || (m.container === 'mp4' && m.mode === 'audio' && cap.aac);
  const webm = !mp4 && m.mode !== 'audio-wav';
  const format = m.mode === 'audio-wav' ? new WavOutputFormat() : mp4 ? new Mp4OutputFormat() : new WebMOutputFormat();
  const output = new Output({ format, target });
  const video = m.mode === 'audio' || m.mode === 'audio-wav' ? { discard: true } : { codec: mp4 ? 'avc' : 'vp9', ...(m.height ? { height: m.height } : {}), bitrate: Q[m.quality || 'medium'], hardwareAcceleration: 'no-preference', forceTranscode: m.mode === 'convert' };
  const audio = m.mode === 'audio-wav' ? { codec: 'pcm-s16' } : { codec: mp4 ? 'aac' : 'opus', bitrate: m.mode === 'audio' ? (m.audioKbps || 128) * 1000 : 128000 };
  const conv = await Conversion.init({ input, output, video, audio, ...(m.start != null || m.end != null ? { trim: { start: m.start || 0, ...(m.end ? { end: m.end } : {}) } } : {}) });
  if (!conv.isValid) throw new Error('This file cannot be converted here: ' + conv.discardedTracks.map((t) => `${t.track.type} (${t.reason})`).join(', '));
  conv.onProgress = (p) => post({ id: m.id, progress2: p });
  const t0 = performance.now(); await conv.execute();
  const ext = m.mode === 'audio-wav' ? 'wav' : m.mode === 'audio' ? (mp4 ? 'm4a' : 'webm') : mp4 ? 'mp4' : 'webm';
  return { bytes: target.buffer, ext, mime: ext === 'wav' ? 'audio/wav' : ext === 'm4a' ? 'audio/mp4' : ext === 'mp4' ? 'video/mp4' : m.mode === 'audio' ? 'audio/webm' : 'video/webm', ms: performance.now() - t0, codec: mp4 ? 'H.264 + AAC' : 'VP9 + Opus', cap };
}
async function infoFull(blob) {
  const input = new Input({ source: new BlobSource(blob), formats: ALL_FORMATS }), dur = await input.computeDuration(), tracks = [];
  for (const v of await input.getVideoTracks()) tracks.push({ type: 'video', codec: v.codec, w: v.displayWidth, h: v.displayHeight, rotation: v.rotation, fps: await v.computePacketStats(200).then((s) => Math.round(s.averagePacketRate * 100) / 100).catch(() => null) });
  for (const a of await input.getAudioTracks()) tracks.push({ type: 'audio', codec: a.codec, channels: a.numberOfChannels, rate: a.sampleRate });
  return { duration: dur, format: (await input.getFormat()).name, mime: await input.getMimeType().catch(() => ''), size: blob.size, kbps: dur ? Math.round(blob.size * 8 / dur / 1000) : 0, tracks };
}
// Normalize: measure with the desktop's BS.1770 meter, plan the gain like the desktop's normalize module, then apply it (with a lookahead
// limiter envelope when the peaks need one) while re-encoding only the audio. Video is copied as it is.
async function normalize(m, post) {
  await ready; const input = new Input({ source: new BlobSource(m.blob), formats: ALL_FORMATS }), track = await input.getPrimaryAudioTrack();
  if (!track) throw new Error('This file has no audio to normalize.');
  const rate = track.sampleRate, ch = track.numberOfChannels, dur = await input.computeDuration();
  const layout = ch === 1 ? 'mono' : ch === 2 ? 'stereo' : ch === 6 ? '5.1' : '';
  const meter = new LoudMeter(rate, ch, layout), sink = new AudioSampleSink(track); let t0 = performance.now(), frames = 0, firstTs = null;
  for await (const s of sink.samples()) {
    if (firstTs === null) firstTs = s.timestamp;
    const buf = new Float32Array(s.numberOfFrames * s.numberOfChannels); s.copyTo(buf, { planeIndex: 0, format: 'f32' }); meter.push(buf); frames += s.numberOfFrames; s.close();
    if (dur) post({ id: m.id, progress2: Math.min(0.45, (frames / rate / dur) * 0.45) });
  }
  const before = JSON.parse(meter.finish()), tMeasure = performance.now() - t0;
  if (!(before.integrated_lufs > -70)) throw new Error('This sound is too quiet to measure (everything is under -70 LUFS), so there is no loudness to set.');
  const ceiling = m.ceiling ?? -1, target = m.target ?? -16;
  const gain = m.peakMode ? ceiling - before.true_peak_dbtp : target - before.integrated_lufs, after = before.true_peak_dbtp + gain, limit = !m.peakMode && after > ceiling;
  if (Math.abs(gain) < 0.1 && !limit) return { skipped: true, before, gain, target, ceiling };
  const container = m.container === 'wav' ? 'wav' : (m.ext === 'webm' || m.ext === 'mkv') ? 'webm' : 'mp4';
  const cap = await capabilities(), mp4 = container === 'mp4' && cap.aac;
  const measureOut = async (bytes, mime) => { // the desktop measures the result again and corrects it; so do we
    const inp = new Input({ source: new BlobSource(new Blob([bytes], { type: mime })), formats: ALL_FORMATS }), tr = await inp.getPrimaryAudioTrack(), mt = new LoudMeter(tr.sampleRate, tr.numberOfChannels, tr.numberOfChannels === 1 ? 'mono' : tr.numberOfChannels === 2 ? 'stereo' : tr.numberOfChannels === 6 ? '5.1' : '');
    for await (const sm of new AudioSampleSink(tr).samples()) { const buf = new Float32Array(sm.numberOfFrames * sm.numberOfChannels); sm.copyTo(buf, { planeIndex: 0, format: 'f32' }); mt.push(buf); sm.close(); }
    const r = JSON.parse(mt.finish()); mt.free(); return r;
  };
  const run = async (gainDb, useLimit, from, to) => {
    const env = meter.envelope(gainDb, useLimit ? ceiling - 0.3 : 0, useLimit), block = meter.block_samples();
    const format = container === 'wav' ? new WavOutputFormat() : mp4 ? new Mp4OutputFormat() : new WebMOutputFormat(), target2 = new BufferTarget(), output = new Output({ format, target: target2 });
    const audio = { codec: container === 'wav' ? 'pcm-s16' : mp4 ? 'aac' : 'opus', bitrate: 192000, process: (s) => {
      const buf = new Float32Array(s.numberOfFrames * s.numberOfChannels); s.copyTo(buf, { planeIndex: 0, format: 'f32' });
      const f0 = Math.round((s.timestamp - firstTs) * s.sampleRate), n = s.numberOfFrames, c = s.numberOfChannels;
      for (let i = 0; i < n; i++) { const pos = (f0 + i) / block, k = Math.min(env.length - 1, Math.max(0, Math.floor(pos))), k2 = Math.min(env.length - 1, k + 1), fr = pos - k, g = env[k] * (1 - fr) + env[k2] * fr; for (let j = 0; j < c; j++) buf[i * c + j] *= g; }
      const out = new AudioSample({ data: buf, format: 'f32', numberOfChannels: c, sampleRate: s.sampleRate, timestamp: s.timestamp }); s.close(); return out;
    } };
    const conv = await Conversion.init({ input: new Input({ source: new BlobSource(m.blob), formats: ALL_FORMATS }), output, audio, video: container === 'wav' ? { discard: true } : {} });
    if (!conv.isValid) throw new Error('This file cannot be normalized here: ' + conv.discardedTracks.map((t) => `${t.track.type} (${t.reason})`).join(', '));
    conv.onProgress = (p) => post({ id: m.id, progress2: from + p * (to - from) }); await conv.execute(); return target2.buffer;
  };
  const mime = container === 'wav' ? 'audio/wav' : mp4 ? (m.hasVideo ? 'video/mp4' : 'audio/mp4') : 'video/webm';
  let g = gain, useLimit = limit, bytes = await run(g, useLimit, 0.45, 0.75), after2 = await measureOut(bytes, mime), attempts = 1;
  let best = { bytes, after: after2, g, useLimit };
  for (; attempts < 3 && !m.peakMode && Math.abs(best.after.integrated_lufs - target) > 0.5; attempts++) {
    const step = Math.max(-6, Math.min(3, target - best.after.integrated_lufs)); // a limiter eats extra gain, so never chase it far
    const g2 = best.g + step, lim2 = before.true_peak_dbtp + g2 > ceiling;
    if (Math.abs(g2 - gain) > 3 && step > 0) break;
    const b2 = await run(g2, lim2, 0.75, 0.95), a2 = await measureOut(b2, mime);
    if (Math.abs(a2.integrated_lufs - target) < Math.abs(best.after.integrated_lufs - target)) best = { bytes: b2, after: a2, g: g2, useLimit: lim2 }; else break;
  }
  if (!m.peakMode && Math.abs(best.after.integrated_lufs - target) >= Math.abs(before.integrated_lufs - target)) { meter.free(); return { skipped: true, tooPeaky: true, before, gain, target, ceiling, after: best.after }; }
  ({ bytes, after: after2, g, useLimit } = best);
  meter.free();
  return { bytes, ext: container === 'wav' ? 'wav' : mp4 ? (m.hasVideo ? 'mp4' : 'm4a') : 'webm', mime, before, after: after2, gain: g, limit: useLimit, target, ceiling, measureMs: tMeasure, attempts, offTarget: Math.abs(after2.integrated_lufs - target) > 0.5 && !m.peakMode };
}
onmessage = async (e) => {
  const m = e.data;
  try {
    if (m.op === 'caps') postMessage({ id: m.id, caps: await capabilities() });
    else if (m.op === 'info') postMessage({ id: m.id, info: await info(m.blob) });
    else if (m.op === 'infoFull') postMessage({ id: m.id, info: await infoFull(m.blob) });
    else if (m.op === 'normalize') { const r = await normalize(m, postMessage); postMessage({ id: m.id, ...r }, r.bytes ? [r.bytes] : []); }
    else { const r = await convert(m, postMessage); postMessage({ id: m.id, ...r }, [r.bytes]); }
  } catch (err) { postMessage({ id: m.id, error: String(err.message || err) }); }
};
