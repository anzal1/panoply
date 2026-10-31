// Store-only ZIP writer (no compression: PDFs are already compressed).
const T = new Uint32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (b) => { let c = ~0; for (let i = 0; i < b.length; i++) c = T[(c ^ b[i]) & 255] ^ (c >>> 8); return ~c >>> 0; };
export function zip(files) { // [{name, bytes}]
  const enc = new TextEncoder(), parts = [], cd = []; let off = 0;
  for (const f of files) {
    const n = enc.encode(f.name), c = crc(f.bytes), h = new DataView(new ArrayBuffer(30)); 
    h.setUint32(0, 0x04034b50, true); h.setUint16(4, 20, true); h.setUint16(6, 0x0800, true); h.setUint16(8, 0, true); h.setUint16(10, 0, true); h.setUint16(12, 0x21, true);
    h.setUint32(14, c, true); h.setUint32(18, f.bytes.length, true); h.setUint32(22, f.bytes.length, true); h.setUint16(26, n.length, true); h.setUint16(28, 0, true);
    parts.push(new Uint8Array(h.buffer), n, f.bytes);
    const e = new DataView(new ArrayBuffer(46));
    e.setUint32(0, 0x02014b50, true); e.setUint16(4, 20, true); e.setUint16(6, 20, true); e.setUint16(8, 0x0800, true); e.setUint16(12, 0, true); e.setUint16(14, 0x21, true);
    e.setUint32(16, c, true); e.setUint32(20, f.bytes.length, true); e.setUint32(24, f.bytes.length, true); e.setUint16(28, n.length, true); e.setUint32(42, off, true);
    cd.push(new Uint8Array(e.buffer), n); off += 30 + n.length + f.bytes.length;
  }
  const cdLen = cd.reduce((a, p) => a + p.length, 0), end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true); end.setUint16(8, files.length, true); end.setUint16(10, files.length, true); end.setUint32(12, cdLen, true); end.setUint32(16, off, true);
  return new Blob([...parts, ...cd, new Uint8Array(end.buffer)], { type: 'application/zip' });
}
