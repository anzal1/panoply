// Lossless metadata removal: no re-encode, the picture bytes are untouched. JPEG, PNG and WebP.
const u16 = (b, o) => (b[o] << 8) | b[o + 1];
function exifFacts(b, s, e) { // b[s..e) is the TIFF block inside APP1 after "Exif\0\0"
  const t = b.subarray(s, e); if (t.length < 12) return { orientation: 1, facts: [] };
  const le = t[0] === 0x49, r16 = (o) => le ? t[o] | (t[o + 1] << 8) : (t[o] << 8) | t[o + 1], r32 = (o) => le ? (t[o] | (t[o + 1] << 8) | (t[o + 2] << 16) | (t[o + 3] << 24)) >>> 0 : ((t[o] << 24) | (t[o + 1] << 16) | (t[o + 2] << 8) | t[o + 3]) >>> 0;
  const facts = []; let orientation = 1; const ifd = r32(4); if (ifd + 2 > t.length) return { orientation, facts };
  const str = (o, n) => { const p = n > 4 ? r32(o + 8) : o + 8; return String.fromCharCode(...t.subarray(p, p + Math.min(n, 40))).replace(/\0.*$/, '').trim(); };
  for (let k = 0, n = r16(ifd); k < n; k++) {
    const en = ifd + 2 + k * 12; if (en + 12 > t.length) break; const tag = r16(en), cnt = r32(en + 4);
    if (tag === 0x0112) orientation = r16(en + 8);
    else if (tag === 0x8825) facts.push('GPS location');
    else if (tag === 0x010f || tag === 0x0110) { const v = str(en, cnt); if (v) facts.push((tag === 0x010f ? 'Camera make: ' : 'Camera model: ') + v); }
    else if (tag === 0x0132) facts.push('Date and time');
    else if (tag === 0x0131) { const v = str(en, cnt); if (v) facts.push('Software: ' + v); }
    else if (tag === 0xa431 || tag === 0x9c9b) facts.push('Owner or author fields');
  }
  return { orientation, facts };
}
const orientationApp1 = (o) => Uint8Array.from([0xFF, 0xE1, 0, 0x22, 0x45, 0x78, 0x69, 0x66, 0, 0, 0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, o, 0, 0, 0, 0, 0, 0, 0]);

export function stripJpeg(b, keepIcc) {
  const out = [b.subarray(0, 2)], removed = []; let i = 2, orientation = 1, facts = [];
  while (i + 4 <= b.length && b[i] === 0xFF) {
    const m = b[i + 1], len = u16(b, i + 2), seg = b.subarray(i, i + 2 + len);
    if (m === 0xDA) break;
    const id = String.fromCharCode(...b.subarray(i + 4, i + 4 + 6));
    if (m === 0xE1 && id.startsWith('Exif')) { const x = exifFacts(b, i + 10, i + 2 + len); orientation = x.orientation; facts = x.facts; removed.push('EXIF'); }
    else if (m === 0xE1) removed.push('XMP');
    else if (m === 0xED) removed.push('IPTC / Photoshop');
    else if (m === 0xFE) removed.push('Comment');
    else if (m === 0xE2 && id.startsWith('ICC_PR') && keepIcc) out.push(seg);
    else if (m === 0xE2 && id.startsWith('ICC_PR')) removed.push('Colour profile');
    else if (m >= 0xE0 && m <= 0xEF && m !== 0xE0 && m !== 0xE2) removed.push('APP' + (m - 0xE0) + ' data');
    else out.push(seg);
    i += 2 + len;
  }
  if (orientation > 1 && orientation <= 8) out.splice(1, 0, orientationApp1(orientation)); // keep the photo upright
  out.push(b.subarray(i));
  return { bytes: cat(out), removed: [...new Set(removed)], facts, orientationKept: orientation > 1 };
}
export function stripPng(b, keepIcc) {
  const out = [b.subarray(0, 8)], removed = []; let i = 8;
  const drop = new Set(['eXIf', 'tEXt', 'zTXt', 'iTXt', 'tIME']);
  while (i + 12 <= b.length) {
    const len = ((b[i] << 24) | (b[i + 1] << 16) | (b[i + 2] << 8) | b[i + 3]) >>> 0, type = String.fromCharCode(...b.subarray(i + 4, i + 8)), end = i + 12 + len;
    if (drop.has(type) || (type === 'iCCP' && !keepIcc)) removed.push({ eXIf: 'EXIF', tEXt: 'Text notes', zTXt: 'Text notes', iTXt: 'Text notes (XMP)', tIME: 'Modified time', iCCP: 'Colour profile' }[type]);
    else out.push(b.subarray(i, end));
    i = end; if (type === 'IEND') break;
  }
  return { bytes: cat(out), removed: [...new Set(removed)], facts: [] };
}
export function stripWebp(b) {
  if (String.fromCharCode(...b.subarray(0, 4)) !== 'RIFF') throw new Error('Not a WebP file.');
  const out = [], removed = []; let i = 12, orientation = 1, facts = [], vp8x = null;
  while (i + 8 <= b.length) {
    const type = String.fromCharCode(...b.subarray(i, i + 4)), len = b[i + 4] | (b[i + 5] << 8) | (b[i + 6] << 16) | (b[i + 7] << 24), end = i + 8 + len + (len & 1);
    if (type === 'EXIF') { const s0 = i + 8 + (String.fromCharCode(...b.subarray(i + 8, i + 12)) === 'Exif' ? 6 : 0), x = exifFacts(b, s0, i + 8 + len); orientation = x.orientation; facts = x.facts; removed.push('EXIF'); }
    else if (type === 'XMP ') removed.push('XMP');
    else { const c = b.slice(i, Math.min(end, b.length)); if (type === 'VP8X') { c[8] &= ~(0x08 | 0x04); vp8x = c; } out.push(c); }
    i = end;
  }
  if (orientation > 1 && orientation <= 8 && vp8x) { vp8x[8] |= 0x08; const t = Uint8Array.from([0x49, 0x49, 0x2A, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, orientation, 0, 0, 0, 0, 0, 0, 0]); out.push(cat([Uint8Array.from([0x45, 0x58, 0x49, 0x46, 26, 0, 0, 0]), t])); }
  const body = cat(out), head = new Uint8Array(12); head.set(b.subarray(0, 12)); const sz = body.length + 4; head[4] = sz & 255; head[5] = (sz >> 8) & 255; head[6] = (sz >> 16) & 255; head[7] = (sz >>> 24) & 255;
  return { bytes: cat([head, body]), removed: [...new Set(removed)], facts, orientationKept: orientation > 1 && !!vp8x };
}
function cat(parts) { const n = parts.reduce((a, p) => a + p.length, 0), o = new Uint8Array(n); let k = 0; for (const p of parts) { o.set(p, k); k += p.length; } return o; }
export function strip(b, keepIcc) {
  if (b[0] === 0xFF && b[1] === 0xD8) return { ...stripJpeg(b, keepIcc), type: 'image/jpeg', ext: 'jpg' };
  if (b[0] === 0x89 && b[1] === 0x50) return { ...stripPng(b, keepIcc), type: 'image/png', ext: 'png' };
  if (b[0] === 0x52 && b[1] === 0x49) return { ...stripWebp(b), type: 'image/webp', ext: 'webp' };
  throw new Error('This tool reads JPEG, PNG and WebP photos.');
}
