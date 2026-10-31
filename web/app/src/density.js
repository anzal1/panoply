// Write the print resolution into a JPEG (JFIF) or PNG (pHYs), like the desktop does, so printers and layout programs read the real size.
export function jpegDensity(b, dpi) {
  if (b.length < 20 || b[0] !== 0xFF || b[1] !== 0xD8 || b[2] !== 0xFF || b[3] !== 0xE0 || String.fromCharCode(...b.subarray(6, 11)) !== 'JFIF\0') return b;
  const d = Math.min(dpi, 65535); b[13] = 1; b[14] = d >> 8; b[15] = d & 255; b[16] = d >> 8; b[17] = d & 255; return b;
}
const crcT = new Uint32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (b) => { let c = ~0; for (const x of b) c = crcT[(c ^ x) & 255] ^ (c >>> 8); return ~c >>> 0; };
export function pngDensity(png, dpi) {
  const sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; if (png.length < 33 || sig.some((v, i) => png[i] !== v)) return png;
  let i = 8, at = -1; const dv = new DataView(png.buffer, png.byteOffset, png.byteLength);
  while (i + 8 <= png.length) { const len = dv.getUint32(i), kind = String.fromCharCode(...png.subarray(i + 4, i + 8)); if (kind === 'pHYs') return png; if (kind === 'IDAT') { at = i; break; } i += 12 + len; }
  if (at < 0) return png;
  const ppm = Math.round(dpi / 0.0254), chunk = new Uint8Array(21), cv = new DataView(chunk.buffer); cv.setUint32(0, 9); chunk.set([0x70, 0x48, 0x59, 0x73], 4); cv.setUint32(8, ppm); cv.setUint32(12, ppm); chunk[16] = 1; cv.setUint32(17, crc(chunk.subarray(4, 17)));
  const out = new Uint8Array(png.length + 21); out.set(png.subarray(0, at)); out.set(chunk, at); out.set(png.subarray(at), at + 21); return out;
}
