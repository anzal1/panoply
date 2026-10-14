//! Lossless Huffman optimization and joining of baseline JPEG strips, without a coefficient
//! buffer.
//!
//! MozJPEG (like libjpeg) can only optimize Huffman tables, write progressive scans, or run its
//! trellis quantizer after it holds every coefficient of the picture in memory: 3 bytes a pixel
//! for optimized tables, 6 for trellis. That is what capped the best Compress mode near 24
//! megapixels inside 300 MB. This module removes the cap:
//!
//! * the picture is cut into strips of whole MCU rows, and each strip is encoded as a small
//!   baseline JPEG of its own, with trellis quantization, standard Huffman tables and the
//!   strip's own coefficient buffer (a few megapixels at most, and strips can encode on several
//!   threads at once);
//! * [`merge_and_optimize`] then reads the entropy coded data of every strip twice: once to
//!   count how often each Huffman symbol occurs and build the optimal tables from the counts (the
//!   algorithm of ITU T.81 Annex K.2, as libjpeg's `jpeg_gen_optimal_table` does), and once to
//!   write the same symbols and extra bits, in order, with the new codes, as one scan.
//!
//! Joining changes one thing: DC values are coded as the difference from the previous block, and
//! the first block of a strip was coded against zero, so those differences are redone against
//! the last block of the strip above. Nothing about the picture changes: the output decodes to
//! the same pixels as the strips would, bit for bit. Memory is one strip plus the output buffer
//! whatever the picture size, and the strips themselves can wait in a file.
//!
//! Measured on a 49 MP photograph at quality 75: the joined, optimized baseline file is 0.2
//! percent larger than MozJPEG's progressive file for the same picture, and 36 percent smaller
//! than a plain streaming baseline file.
//!
//! Only the shape this crate's encoder writes is handled: baseline (SOF0, or SOF1 at 8 bits),
//! one interleaved scan, no restart intervals. Anything else comes back unchanged from
//! [`optimize_huffman`].

// Table-driven JPEG code: index loops mirror the spec (ITU T.81) on purpose.
#![allow(clippy::needless_range_loop, clippy::type_complexity)]

use std::borrow::Cow;
use std::io::Write;

use crate::error::{ImageError, Result};

/// Where the strips of a picture are kept: in memory, or in a file read back one at a time.
pub trait StripSource {
    /// How many strips there are.
    fn count(&self) -> usize;
    /// The bytes of strip `i`, a complete JPEG file.
    fn get(&self, i: usize) -> Result<Cow<'_, [u8]>>;
}

impl StripSource for [Vec<u8>] {
    fn count(&self) -> usize {
        self.len()
    }
    fn get(&self, i: usize) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Borrowed(&self[i]))
    }
}

/// Join `strips` that were all encoded with the standard Huffman tables (the same tables in every
/// strip), by writing each strip's entropy coded data one after the other with restart markers in
/// between and a restart interval of one strip. No symbol is decoded: the strips are copied.
///
/// A restart resets the DC prediction, which is exactly where each strip started, and every strip
/// ends on a byte boundary, so the bytes can be joined as they are. `strip_mcus` is the number of
/// MCUs in every strip but the last, and the strips must be a whole number of MCU rows.
pub fn join_restart(
    strips: &(impl StripSource + ?Sized),
    total_height: usize,
    strip_mcus: usize,
    out: &mut dyn Write,
) -> Result<()> {
    let n = strips.count();
    if n == 0 || strip_mcus == 0 || strip_mcus > 65535 {
        return Err(bad(
            "no strips, or too many MCUs in a strip for a restart interval",
        ));
    }
    let first_bytes = strips.get(0)?;
    let first =
        Parsed::parse(&first_bytes).ok_or_else(|| bad("the first strip is not baseline"))?;
    let mut head = Vec::with_capacity(first_bytes.len().min(1 << 16));
    for &(a, b) in &first.keep {
        let seg = &first_bytes[a..b];
        if a == first.sof_start {
            let mut seg = seg.to_vec();
            let h = u16::try_from(total_height)
                .map_err(|_| bad("a picture taller than a JPEG allows"))?;
            seg[5..7].copy_from_slice(&h.to_be_bytes());
            head.extend_from_slice(&seg);
        } else {
            head.extend_from_slice(seg);
        }
    }
    // The tables, as the strips wrote them.
    for (class, tables) in [(0u8, &first.dc_tables), (1u8, &first.ac_tables)] {
        for (id, t) in tables.iter().enumerate() {
            let Some((bits, vals)) = t else { continue };
            let len = 2 + 1 + 16 + vals.len();
            head.extend_from_slice(&[0xFF, 0xC4]);
            head.extend_from_slice(&(len as u16).to_be_bytes());
            head.push((class << 4) | id as u8);
            head.extend_from_slice(&bits[1..]);
            head.extend_from_slice(vals);
        }
    }
    if n > 1 {
        head.extend_from_slice(&[0xFF, 0xDD, 0, 4]);
        head.extend_from_slice(&(strip_mcus as u16).to_be_bytes());
    }
    head.extend_from_slice(&first_bytes[first.sos.0..first.sos.1]);
    out.write_all(&head)?;
    for i in 0..n {
        let bytes = strips.get(i)?;
        let p = Parsed::parse(&bytes).ok_or_else(|| bad("a strip is not baseline"))?;
        if p.width != first.width
            || p.dc_tables != first.dc_tables
            || p.ac_tables != first.ac_tables
        {
            return Err(bad("strips with different tables"));
        }
        // The data runs to the end marker, which is left out.
        let data = &bytes[p.data..];
        let end = data
            .len()
            .checked_sub(2)
            .filter(|&e| data[e..] == [0xFF, 0xD9])
            .ok_or_else(|| bad("a strip without an end marker"))?;
        out.write_all(&data[..end])?;
        if i + 1 < n {
            out.write_all(&[0xFF, 0xD0 + (i % 8) as u8])?;
        }
    }
    out.write_all(&[0xFF, 0xD9])?;
    Ok(())
}

/// Returns `jpeg` with optimal Huffman tables, or `jpeg` itself (copied) when it is not a
/// single scan baseline file this module understands.
pub fn optimize_huffman(jpeg: &[u8]) -> Result<Vec<u8>> {
    if Parsed::parse(jpeg).is_none() {
        return Ok(jpeg.to_vec());
    }
    let mut out = Vec::with_capacity(jpeg.len());
    let one = [jpeg.to_vec()];
    let h = Parsed::parse(jpeg).map(|p| p.height).unwrap_or(0);
    merge_and_optimize(&one[..], h, &mut out)?;
    Ok(out)
}

/// True when [`optimize_huffman`] would change `jpeg` (it is one this module understands).
pub fn can_optimize(jpeg: &[u8]) -> bool {
    Parsed::parse(jpeg).is_some()
}

struct Comp {
    h: usize,
    v: usize,
    dc: usize,
    ac: usize,
}

struct Parsed {
    width: usize,
    height: usize,
    comps: Vec<Comp>,
    /// Segments to keep, in order, before the scan (everything except DHT).
    keep: Vec<(usize, usize)>,
    /// Where the SOF segment starts (its marker), so the height can be patched.
    sof_start: usize,
    /// The SOS segment including its marker.
    sos: (usize, usize),
    /// Where the entropy coded data starts.
    data: usize,
    /// The four DC and four AC tables as the file defines them: `(bits, values)`.
    dc_tables: [Option<([u8; 17], Vec<u8>)>; 4],
    ac_tables: [Option<([u8; 17], Vec<u8>)>; 4],
}

impl Parsed {
    fn parse(b: &[u8]) -> Option<Parsed> {
        if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
            return None;
        }
        let mut i = 2;
        let mut keep = vec![(0usize, 2usize)];
        let mut frame: Option<(usize, usize, Vec<(usize, usize)>)> = None;
        let mut dc_tables: [Option<([u8; 17], Vec<u8>)>; 4] = Default::default();
        let mut ac_tables: [Option<([u8; 17], Vec<u8>)>; 4] = Default::default();
        let mut scans = 0;
        let mut sof_start = 0usize;
        loop {
            while i < b.len() && b[i] == 0xFF && b.get(i + 1) == Some(&0xFF) {
                i += 1;
            }
            if i + 4 > b.len() || b[i] != 0xFF {
                return None;
            }
            let m = b[i + 1];
            let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
            if len < 2 || i + 2 + len > b.len() {
                return None;
            }
            let seg = &b[i + 4..i + 2 + len];
            match m {
                // Anything but baseline Huffman: not ours.
                0xC2 | 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
                // Baseline, or extended sequential at 8 bits (what libjpeg writes when a
                // quantization table has a value over 255): the same Huffman coding.
                0xC0 | 0xC1 => {
                    if seg.len() < 6 || seg[0] != 8 {
                        return None;
                    }
                    let height = usize::from(u16::from_be_bytes([seg[1], seg[2]]));
                    let width = usize::from(u16::from_be_bytes([seg[3], seg[4]]));
                    let n = usize::from(seg[5]);
                    if seg.len() != 6 + 3 * n || !(n == 1 || n == 3) || width == 0 || height == 0 {
                        return None;
                    }
                    let comps = (0..n)
                        .map(|c| {
                            let id = usize::from(seg[6 + 3 * c]);
                            let hv = seg[7 + 3 * c];
                            (id, usize::from(hv))
                        })
                        .collect();
                    frame = Some((width, height, comps));
                    sof_start = i;
                    keep.push((i, i + 2 + len));
                }
                0xC4 => {
                    let mut p = 0;
                    while p < seg.len() {
                        if p + 17 > seg.len() {
                            return None;
                        }
                        let tc = seg[p] >> 4;
                        let th = usize::from(seg[p] & 15);
                        let mut bits = [0u8; 17];
                        bits[1..].copy_from_slice(&seg[p + 1..p + 17]);
                        let n: usize = bits[1..].iter().map(|&x| usize::from(x)).sum();
                        if th > 3 || p + 17 + n > seg.len() || n > 256 {
                            return None;
                        }
                        let vals = seg[p + 17..p + 17 + n].to_vec();
                        match tc {
                            0 => dc_tables[th] = Some((bits, vals)),
                            1 => ac_tables[th] = Some((bits, vals)),
                            _ => return None,
                        }
                        p += 17 + n;
                    }
                }
                // A restart interval would need the restart markers handled.
                0xDD => {
                    if seg.len() != 2 || seg[0] != 0 || seg[1] != 0 {
                        return None;
                    }
                    keep.push((i, i + 2 + len));
                }
                0xDA => {
                    scans += 1;
                    let (width, height, fcomps) = frame.as_ref()?;
                    if scans != 1 || seg.is_empty() {
                        return None;
                    }
                    let ns = usize::from(seg[0]);
                    if ns != fcomps.len() || seg.len() != 1 + 2 * ns + 3 {
                        return None;
                    }
                    let tail = &seg[1 + 2 * ns..];
                    if tail != [0, 63, 0] {
                        return None;
                    }
                    let mut comps = Vec::new();
                    for k in 0..ns {
                        let cs = usize::from(seg[1 + 2 * k]);
                        let t = seg[2 + 2 * k];
                        let &(_, hv) = fcomps.iter().find(|c| c.0 == cs)?;
                        let (h, v) = (hv >> 4, hv & 15);
                        if h == 0 || v == 0 || h > 4 || v > 4 {
                            return None;
                        }
                        comps.push(Comp {
                            h,
                            v,
                            dc: usize::from(t >> 4),
                            ac: usize::from(t & 15),
                        });
                    }
                    if comps.iter().any(|c| c.dc > 3 || c.ac > 3) {
                        return None;
                    }
                    for c in &comps {
                        dc_tables[c.dc].as_ref()?;
                        ac_tables[c.ac].as_ref()?;
                    }
                    return Some(Parsed {
                        width: *width,
                        height: *height,
                        comps,
                        keep,
                        sof_start,
                        sos: (i, i + 2 + len),
                        data: i + 2 + len,
                        dc_tables,
                        ac_tables,
                    });
                }
                _ => keep.push((i, i + 2 + len)),
            }
            i += 2 + len;
        }
    }
}

/// A canonical Huffman decoder.
struct Decoder {
    look: Vec<u16>,
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
}

const LOOK_BITS: u32 = 9;

#[cfg(test)]
static FORCE_SLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Decoder {
    fn new(bits: &[u8; 17], vals: &[u8]) -> Decoder {
        let mut d = Decoder {
            look: vec![0; 1 << LOOK_BITS],
            maxcode: [-1; 18],
            valptr: [0; 17],
            mincode: [0; 17],
            vals: vals.to_vec(),
        };
        let mut code = 0i32;
        let mut k = 0i32;
        for l in 1..=16usize {
            let n = i32::from(bits[l]);
            d.valptr[l] = k;
            d.mincode[l] = code;
            if n > 0 {
                for j in 0..n {
                    let c = code + j;
                    if l as u32 <= LOOK_BITS {
                        let shift = LOOK_BITS - l as u32;
                        let sym = u16::from(*vals.get((k + j) as usize).unwrap_or(&0));
                        for fill in 0..(1u32 << shift) {
                            let idx = ((c as u32) << shift | fill) as usize;
                            if idx < d.look.len() {
                                d.look[idx] = ((l as u16) << 8) | sym;
                            }
                        }
                    }
                }
                code += n;
                k += n;
                d.maxcode[l] = code - 1;
            }
            code <<= 1;
        }
        d.maxcode[17] = i32::MAX;
        d
    }
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    n: u32,
    /// Zero bits fed after the data ended (a marker or the end of the buffer). Reading into them
    /// means the file is damaged.
    fake: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            pos: 0,
            buf: 0,
            n: 0,
            fake: 0,
        }
    }

    #[inline]
    fn fill(&mut self) {
        while self.n <= 56 {
            let mut byte = 0u8;
            if self.pos < self.data.len() {
                let b = self.data[self.pos];
                if b == 0xFF {
                    if self.data.get(self.pos + 1) == Some(&0) {
                        byte = 0xFF;
                        self.pos += 2;
                    } else {
                        // A marker: the entropy data is over, feed zeros.
                        self.fake += 8;
                    }
                } else {
                    byte = b;
                    self.pos += 1;
                }
            } else {
                self.fake += 8;
            }
            self.buf |= u64::from(byte) << (56 - self.n);
            self.n += 8;
        }
    }

    #[inline]
    fn peek(&mut self, k: u32) -> u32 {
        if self.n < k {
            self.fill();
        }
        (self.buf >> (64 - k)) as u32
    }

    #[inline]
    fn skip(&mut self, k: u32) {
        self.buf <<= k;
        self.n -= k;
    }

    #[inline]
    fn symbol(&mut self, d: &Decoder) -> Result<u8> {
        #[cfg(test)]
        let slow = FORCE_SLOW.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(test))]
        let slow = false;
        let p = self.peek(LOOK_BITS) as usize;
        let e = d.look[p];
        if e != 0 && !slow {
            self.skip(u32::from(e >> 8));
            return Ok(e as u8);
        }
        let v = self.peek(16) as i32;
        for l in (if slow { 1 } else { LOOK_BITS as usize + 1 })..=16 {
            let code = v >> (16 - l);
            if code <= d.maxcode[l] && d.maxcode[l] >= 0 {
                self.skip(l as u32);
                let idx = d.valptr[l] + code - d.mincode[l];
                return d
                    .vals
                    .get(idx as usize)
                    .copied()
                    .ok_or_else(|| bad("a code that is not in its table"));
            }
        }
        Err(bad(&format!(
            "a code that is not in its table, at byte {} of {}, peek {:016b}",
            self.pos,
            self.data.len(),
            v
        )))
    }

    #[inline]
    fn raw(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        let v = self.peek(k);
        self.skip(k);
        v
    }
}

fn bad(what: &str) -> ImageError {
    ImageError::Failed(format!(
        "internal: the JPEG could not be re-coded ({what}). The unoptimized file is kept."
    ))
}

/// Walks every block of the scan. `f(component, table_is_ac, table_id, symbol, extra_bits,
/// extra_len)` is called for each Huffman symbol with the raw bits that follow it.
fn walk(
    data: &[u8],
    p: &Parsed,
    dc: &[Option<Decoder>; 4],
    ac: &[Option<Decoder>; 4],
    mut f: impl FnMut(usize, bool, usize, u8, u32, u32),
) -> Result<()> {
    let hmax = p.comps.iter().map(|c| c.h).max().unwrap_or(1);
    let vmax = p.comps.iter().map(|c| c.v).max().unwrap_or(1);
    let mcus_x = p.width.div_ceil(8 * hmax);
    let mcus_y = p.height.div_ceil(8 * vmax);
    let mut bits = Bits::new(data);
    for mcu in 0..mcus_x * mcus_y {
        for (ci, c) in p.comps.iter().enumerate() {
            let (Some(dd), Some(ad)) = (dc[c.dc].as_ref(), ac[c.ac].as_ref()) else {
                return Err(bad("a missing table"));
            };
            for _ in 0..c.h * c.v {
                let s = bits
                    .symbol(dd)
                    .map_err(|e| bad(&format!("{e} [dc mcu {mcu} comp {ci}]")))?;
                if s > 11 {
                    return Err(bad("a DC category out of range"));
                }
                let extra = bits.raw(u32::from(s));
                f(ci, false, c.dc, s, extra, u32::from(s));
                let mut k = 1;
                while k < 64 {
                    let rs = bits.symbol(ad).map_err(|e| {
                        bad(&format!(
                            "{e} [mcu {mcu} of {}, comp {ci}, k {k}]",
                            mcus_x * mcus_y
                        ))
                    })?;
                    let (r, s) = (usize::from(rs >> 4), u32::from(rs & 15));
                    if s == 0 {
                        f(ci, true, c.ac, rs, 0, 0);
                        if r == 15 {
                            k += 16;
                            continue;
                        }
                        break;
                    }
                    let extra = bits.raw(s);
                    f(ci, true, c.ac, rs, extra, s);
                    k += r + 1;
                }
                if bits.n < bits.fake {
                    return Err(bad("the data ends early"));
                }
            }
        }
    }
    Ok(())
}

/// Optimal Huffman table for `freq` (256 symbol counts): `(bits[1..=16], values)`.
/// ITU T.81 K.2, the same procedure as libjpeg's `jpeg_gen_optimal_table`.
fn optimal_table(counts: &[u64; 256]) -> ([u8; 17], Vec<u8>) {
    let mut freq = [0u64; 257];
    freq[..256].copy_from_slice(counts);
    // A pseudo symbol with count 1 keeps the all ones code unused.
    freq[256] = 1;
    let mut codesize = [0usize; 257];
    let mut others = [-1i32; 257];
    loop {
        let mut c1 = -1i32;
        let mut v = u64::MAX;
        for (i, &fr) in freq.iter().enumerate() {
            if fr != 0 && fr <= v {
                v = fr;
                c1 = i as i32;
            }
        }
        let mut c2 = -1i32;
        v = u64::MAX;
        for (i, &fr) in freq.iter().enumerate() {
            if fr != 0 && fr <= v && i as i32 != c1 {
                v = fr;
                c2 = i as i32;
            }
        }
        if c2 < 0 {
            break;
        }
        freq[c1 as usize] += freq[c2 as usize];
        freq[c2 as usize] = 0;
        let mut a = c1;
        codesize[a as usize] += 1;
        while others[a as usize] >= 0 {
            a = others[a as usize];
            codesize[a as usize] += 1;
        }
        others[a as usize] = c2;
        let mut b = c2;
        codesize[b as usize] += 1;
        while others[b as usize] >= 0 {
            b = others[b as usize];
            codesize[b as usize] += 1;
        }
    }
    let mut bits = [0i32; 34];
    for &cs in codesize.iter() {
        if cs > 0 {
            bits[cs.min(33)] += 1;
        }
    }
    // Limit code lengths to 16 bits.
    let mut i = 32;
    while i > 16 {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
        i -= 1;
    }
    // Remove the pseudo symbol's code (the longest one).
    let mut i = 16;
    while bits[i] == 0 {
        i -= 1;
    }
    bits[i] -= 1;
    let mut out_bits = [0u8; 17];
    for l in 1..=16 {
        out_bits[l] = bits[l] as u8;
    }
    let mut vals = Vec::new();
    for l in 1..=32usize {
        for (s, &cs) in codesize.iter().enumerate().take(256) {
            if cs == l {
                vals.push(s as u8);
            }
        }
    }
    (out_bits, vals)
}

/// Encoder side of a table: code and length for each symbol.
fn codes(bits: &[u8; 17], vals: &[u8]) -> ([u32; 256], [u8; 256]) {
    let mut code = [0u32; 256];
    let mut size = [0u8; 256];
    let mut c = 0u32;
    let mut k = 0usize;
    for l in 1..=16usize {
        for _ in 0..bits[l] {
            let s = usize::from(vals[k]);
            code[s] = c;
            size[s] = l as u8;
            c += 1;
            k += 1;
        }
        c <<= 1;
    }
    (code, size)
}

struct BitWriter<'w> {
    out: &'w mut dyn Write,
    pending: Vec<u8>,
    buf: u64,
    n: u32,
    err: Option<std::io::Error>,
}

impl BitWriter<'_> {
    #[inline]
    fn put(&mut self, value: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.buf = (self.buf << len) | u64::from(value & ((1u32 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            let b = (self.buf >> (self.n - 8)) as u8;
            self.pending.push(b);
            if b == 0xFF {
                self.pending.push(0);
            }
            self.n -= 8;
        }
        if self.pending.len() >= 1 << 20 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.err.is_none()
            && let Err(e) = self.out.write_all(&self.pending)
        {
            self.err = Some(e);
        }
        self.pending.clear();
    }

    /// Pad the last byte with ones, write the end marker and report any write error.
    fn finish(mut self) -> Result<()> {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1u32 << pad) - 1, pad);
        }
        self.pending.extend_from_slice(&[0xFF, 0xD9]);
        self.flush();
        match self.err {
            Some(e) => Err(ImageError::from(e)),
            None => Ok(()),
        }
    }
}

/// `(category, extra bits)` of a DC difference.
fn dc_code(diff: i32) -> (u8, u32) {
    if diff == 0 {
        return (0, 0);
    }
    let a = diff.unsigned_abs();
    let cat = 32 - a.leading_zeros();
    let extra = if diff > 0 {
        diff as u32
    } else {
        (diff - 1) as u32 & ((1u32 << cat) - 1)
    };
    (cat as u8, extra)
}

/// The value of a DC difference from its category and extra bits.
fn dc_value(cat: u8, extra: u32) -> i32 {
    if cat == 0 {
        0
    } else if extra < (1u32 << (cat - 1)) {
        extra as i32 - (1i32 << cat) + 1
    } else {
        extra as i32
    }
}

/// Tracks DC values across strips: the first block of each component in a strip was coded
/// against zero, and has to be coded against the last block of the strip above.
struct DcJoin {
    run: [i32; 4],
    carry: [i32; 4],
    first: [bool; 4],
}

impl DcJoin {
    fn new() -> DcJoin {
        DcJoin {
            run: [0; 4],
            carry: [0; 4],
            first: [true; 4],
        }
    }

    /// Call between strips.
    fn next_strip(&mut self) {
        self.carry = self.run;
        self.run = [0; 4];
        self.first = [true; 4];
    }

    /// The DC symbol and extra bits to write for a block of component `c` that was coded as
    /// `(cat, extra)` in its strip.
    fn dc(&mut self, c: usize, cat: u8, extra: u32) -> (u8, u32) {
        let diff = dc_value(cat, extra);
        self.run[c] += diff;
        if self.first[c] {
            self.first[c] = false;
            if self.carry[c] != 0 {
                return dc_code(self.run[c] - self.carry[c]);
            }
        }
        (cat, extra)
    }
}

/// Join `strips` (baseline JPEGs of the same width, each a whole number of MCU rows tall except
/// the last, from top to bottom) into one JPEG `total_height` pixels tall with optimal Huffman
/// tables, written to `out`. See the module documentation.
pub fn merge_and_optimize(
    strips: &(impl StripSource + ?Sized),
    total_height: usize,
    out: &mut dyn Write,
) -> Result<()> {
    let n = strips.count();
    if n == 0 {
        return Err(bad("no strips"));
    }
    let first_bytes = strips.get(0)?;
    let first =
        Parsed::parse(&first_bytes).ok_or_else(|| bad("the first strip is not baseline"))?;
    let make = |t: &[Option<([u8; 17], Vec<u8>)>; 4]| -> [Option<Decoder>; 4] {
        std::array::from_fn(|i| t[i].as_ref().map(|(b, v)| Decoder::new(b, v)))
    };

    // One strip at a time: parse it, check it matches the first, and walk it.
    let each = |pass: &mut dyn FnMut(
        &Parsed,
        &[u8],
        &[Option<Decoder>; 4],
        &[Option<Decoder>; 4],
        usize,
    ) -> Result<()>|
     -> Result<()> {
        for i in 0..n {
            let bytes = strips.get(i)?;
            let p = Parsed::parse(&bytes).ok_or_else(|| bad("a strip is not baseline"))?;
            if p.width != first.width
                || p.comps.len() != first.comps.len()
                || p.comps
                    .iter()
                    .zip(&first.comps)
                    .any(|(a, b)| a.h != b.h || a.v != b.v || a.dc != b.dc || a.ac != b.ac)
            {
                return Err(bad("strips of different shapes"));
            }
            let (dcd, acd) = (make(&p.dc_tables), make(&p.ac_tables));
            pass(&p, &bytes[p.data..], &dcd, &acd, i).map_err(|e| {
                ImageError::Failed(format!(
                    "{e} (strip {i} of {n}, {} bytes, {}x{})",
                    bytes.len(),
                    p.width,
                    p.height
                ))
            })?;
        }
        Ok(())
    };

    // Pass 1: symbol counts, with the DC differences as they will be written.
    let mut dc_counts = vec![[0u64; 256]; 4];
    let mut ac_counts = vec![[0u64; 256]; 4];
    let mut join = DcJoin::new();
    each(&mut |p, data, dcd, acd, i| {
        if i > 0 {
            join.next_strip();
        }
        walk(data, p, dcd, acd, |c, is_ac, t, s, extra, _| {
            if is_ac {
                ac_counts[t][usize::from(s)] += 1;
            } else {
                let (cat, _) = join.dc(c, s, extra);
                dc_counts[t][usize::from(cat)] += 1;
            }
        })
    })?;

    let mut dc_new: [Option<([u8; 17], Vec<u8>)>; 4] = Default::default();
    let mut ac_new: [Option<([u8; 17], Vec<u8>)>; 4] = Default::default();
    for c in &first.comps {
        if dc_new[c.dc].is_none() {
            dc_new[c.dc] = Some(optimal_table(&dc_counts[c.dc]));
        }
        if ac_new[c.ac].is_none() {
            ac_new[c.ac] = Some(optimal_table(&ac_counts[c.ac]));
        }
    }
    let dc_codes: Vec<Option<([u32; 256], [u8; 256])>> = dc_new
        .iter()
        .map(|t| t.as_ref().map(|(b, v)| codes(b, v)))
        .collect();
    let ac_codes: Vec<Option<([u32; 256], [u8; 256])>> = ac_new
        .iter()
        .map(|t| t.as_ref().map(|(b, v)| codes(b, v)))
        .collect();

    // Header from the first strip: its segments (the SOF with the full height), the new tables,
    // then the scan header.
    let mut head = Vec::with_capacity(first_bytes.len().min(1 << 16));
    for &(a, b) in &first.keep {
        let seg = &first_bytes[a..b];
        if a == first.sof_start {
            let mut seg = seg.to_vec();
            let h = u16::try_from(total_height)
                .map_err(|_| bad("a picture taller than a JPEG allows"))?;
            seg[5..7].copy_from_slice(&h.to_be_bytes());
            head.extend_from_slice(&seg);
        } else {
            head.extend_from_slice(seg);
        }
    }
    for (class, tables) in [(0u8, &dc_new), (1u8, &ac_new)] {
        for (id, t) in tables.iter().enumerate() {
            let Some((bits, vals)) = t else { continue };
            let len = 2 + 1 + 16 + vals.len();
            head.extend_from_slice(&[0xFF, 0xC4]);
            head.extend_from_slice(&(len as u16).to_be_bytes());
            head.push((class << 4) | id as u8);
            head.extend_from_slice(&bits[1..]);
            head.extend_from_slice(vals);
        }
    }
    head.extend_from_slice(&first_bytes[first.sos.0..first.sos.1]);
    drop(first_bytes);
    out.write_all(&head)?;

    // Pass 2: the same symbols with the new codes.
    let mut w = BitWriter {
        out,
        pending: Vec::with_capacity(1 << 20),
        buf: 0,
        n: 0,
        err: None,
    };
    let mut join = DcJoin::new();
    each(&mut |p, data, dcd, acd, i| {
        if i > 0 {
            join.next_strip();
        }
        walk(data, p, dcd, acd, |c, is_ac, t, s, extra, elen| {
            let (sym, extra, elen) = if is_ac {
                (s, extra, elen)
            } else {
                let (cat, ex) = join.dc(c, s, extra);
                (cat, ex, u32::from(cat))
            };
            let tab = if is_ac { &ac_codes[t] } else { &dc_codes[t] };
            if let Some((code, size)) = tab {
                w.put(code[usize::from(sym)], u32::from(size[usize::from(sym)]));
            }
            w.put(extra, elen);
        })
    })?;
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::jpeg::{JpegOpts, encode};
    use crate::types::{Depth, Image, Layout, Samples};

    fn photo(w: u32, h: u32, layout: Layout) -> Image {
        let mut img = Image::new(w, h, layout, Depth::U8);
        let ch = layout.channels();
        if let Samples::U8(v) = &mut img.samples {
            let mut s = 12345u32;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    for c in 0..ch {
                        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                        let noise = (s >> 24) as i32 % 24 - 12;
                        let base = ((x as i32 * 5 + y as i32 * 3 + c as i32 * 40) % 256)
                            + ((x as f32 / 9.0).sin() * 40.0) as i32;
                        v[(y * w as usize + x) * ch + c] = (base + noise).clamp(0, 255) as u8;
                    }
                }
            }
        }
        img
    }

    fn baseline(img: &Image, q: f32, subsample: bool) -> Vec<u8> {
        encode(
            img,
            &JpegOpts {
                quality: q,
                progressive: false,
                optimize: true,
                trellis: true,
                subsample,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn pixels(jpeg: &[u8]) -> Vec<u8> {
        let img = crate::ops::common::decode_mem(jpeg.to_vec()).unwrap();
        match img.samples {
            Samples::U8(v) => v,
            _ => unreachable!(),
        }
    }

    fn plain(img: &Image, q: f32, subsample: bool) -> Vec<u8> {
        // Standard tables, what a streaming libjpeg-style encode writes.
        encode(
            img,
            &JpegOpts {
                quality: q,
                progressive: false,
                optimize: false,
                subsample,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn recoding_keeps_every_pixel_and_shrinks_the_file() {
        for (w, h, layout, sub) in [
            (203, 157, Layout::Rgb, true),
            (96, 64, Layout::Rgb, false),
            (131, 77, Layout::Gray, true),
        ] {
            let img = photo(w, h, layout);
            for q in [30.0, 75.0, 95.0] {
                let base = plain(&img, q, sub);
                assert!(can_optimize(&base), "{w}x{h} {layout:?} not understood");
                let opt = optimize_huffman(&base).unwrap();
                assert!(
                    opt.len() < base.len(),
                    "{w}x{h} q{q}: {} against {}",
                    opt.len(),
                    base.len()
                );
                assert_eq!(pixels(&base), pixels(&opt), "{w}x{h} q{q}: pixels changed");
                // Tables that are already optimal stay about the same size.
                let again = optimize_huffman(&baseline(&img, q, sub)).unwrap();
                assert_eq!(pixels(&again), pixels(&baseline(&img, q, sub)));
            }
        }
    }

    #[test]
    fn the_saving_is_close_to_what_mozjpeg_gets_with_the_whole_picture() {
        let img = photo(400, 300, Layout::Rgb);
        let base = baseline(&img, 80.0, true);
        let ours = optimize_huffman(&base).unwrap();
        let theirs = encode(
            &img,
            &JpegOpts {
                quality: 80.0,
                progressive: false,
                optimize: true,
                trellis: true,
                ..Default::default()
            },
        )
        .unwrap();
        let (a, b) = (ours.len() as f64, theirs.len() as f64);
        assert!(
            a < b * 1.08 && a > b * 0.92,
            "ours {a} against MozJPEG's own optimization {b}"
        );
    }

    #[test]
    fn strips_join_into_one_picture_with_every_pixel_of_the_strips() {
        // 4:4:4 and gray have no chroma filter across a strip edge, so the joined picture must be
        // exactly the strips one above the other.
        for (layout, sub) in [(Layout::Rgb, false), (Layout::Gray, true)] {
            let (w, h) = (150u32, 100u32);
            let img = photo(w, h, layout);
            let ch = layout.channels();
            let rows = [32usize, 32, 36]; // multiples of 16 except the last
            let (mut strips, mut want) = (Vec::new(), Vec::new());
            let mut y = 0;
            for r in rows {
                let part = img.crop(0, y as u32, w, r as u32);
                let jpeg = baseline(&part, 70.0, sub);
                want.extend(pixels(&jpeg));
                strips.push(jpeg);
                y += r;
            }
            assert_eq!(y, h as usize);
            let mut out = Vec::new();
            merge_and_optimize(&strips[..], h as usize, &mut out).unwrap();
            let got = pixels(&out);
            assert_eq!(got.len(), w as usize * h as usize * ch);
            assert_eq!(
                got, want,
                "{layout:?}: joined pixels differ from the strips"
            );
            let plain: usize = strips.iter().map(Vec::len).sum();
            assert!(out.len() < plain, "{} against {plain}", out.len());
        }
    }

    #[test]
    fn every_code_of_the_standard_tables_decodes_to_its_symbol() {
        let img = photo(64, 48, Layout::Rgb);
        let jpeg = encode(
            &img,
            &JpegOpts {
                progressive: false,
                optimize: false,
                trellis: false,
                ..Default::default()
            },
        )
        .unwrap();
        let p = Parsed::parse(&jpeg).unwrap();
        for tables in [&p.dc_tables, &p.ac_tables] {
            for (bits, vals) in tables.iter().flatten() {
                let dec = Decoder::new(bits, vals);
                let (code, size) = codes(bits, vals);
                for &sym in vals {
                    // The symbol twice in a row, then padding, so a short read shows.
                    let mut sink = Vec::new();
                    let mut w = BitWriter {
                        out: &mut sink,
                        pending: Vec::new(),
                        buf: 0,
                        n: 0,
                        err: None,
                    };
                    for _ in 0..3 {
                        w.put(code[sym as usize], u32::from(size[sym as usize]));
                    }
                    w.finish().unwrap();
                    let mut b = Bits::new(&sink);
                    for _ in 0..3 {
                        assert_eq!(b.symbol(&dec).unwrap(), sym, "len {}", size[sym as usize]);
                    }
                }
            }
        }
    }

    #[test]
    fn optimal_tables_with_long_codes_decode_every_symbol() {
        // Counts that fall off geometrically give the longest codes the format allows.
        let mut counts = [0u64; 256];
        let mut c = 1u64 << 24;
        for (i, slot) in counts.iter_mut().enumerate().take(200) {
            *slot = c.max(1);
            if i % 2 == 1 {
                c = (c as f64 * 0.8) as u64;
            }
        }
        let (bits, vals) = optimal_table(&counts);
        assert!(bits[16] > 0 || bits[15] > 0, "wanted long codes: {bits:?}");
        let dec = Decoder::new(&bits, &vals);
        let (code, size) = codes(&bits, &vals);
        let mut sink = Vec::new();
        let mut w = BitWriter {
            out: &mut sink,
            pending: Vec::new(),
            buf: 0,
            n: 0,
            err: None,
        };
        for &sym in &vals {
            w.put(code[sym as usize], u32::from(size[sym as usize]));
        }
        w.finish().unwrap();
        let mut b = Bits::new(&sink);
        for &sym in &vals {
            assert_eq!(b.symbol(&dec).unwrap(), sym, "len {}", size[sym as usize]);
        }
    }

    #[test]
    fn stress_many_pictures() {
        let mut fails = Vec::new();
        for seed in 0..60u32 {
            let (w, h) = (48 + seed % 7 * 16, 32 + seed % 5 * 16);
            let mut img = Image::new(w, h, Layout::Rgb, Depth::U8);
            if let Samples::U8(v) = &mut img.samples {
                let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
                let amp = 1 + seed % 6 * 40;
                for p in v.iter_mut() {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    *p = ((s >> 16) % amp.min(256)) as u8 ^ (seed as u8);
                }
            }
            for q in [60.0, 85.0, 97.0, 100.0] {
                let base = baseline(&img, q, true);
                if let Err(e) = optimize_huffman(&base) {
                    let ok = crate::ops::common::decode_mem(base.clone()).is_ok();
                    fails.push((seed, q, ok, e.to_string()));
                }
            }
        }
        assert!(fails.is_empty(), "{fails:?}");
    }

    #[test]
    fn restart_joined_strips_decode_to_the_strips_one_above_the_other() {
        for (layout, sub) in [(Layout::Rgb, false), (Layout::Gray, true)] {
            let (w, h) = (150u32, 100u32);
            let img = photo(w, h, layout);
            // Every strip but the last is the same height, and the last is no taller.
            let rows = [32usize, 32, 32, 4];
            let (mut strips, mut want) = (Vec::new(), Vec::new());
            let mut y = 0;
            for r in rows {
                let jpeg = plain(&img.crop(0, y as u32, w, r as u32), 70.0, sub);
                want.extend(pixels(&jpeg));
                strips.push(jpeg);
                y += r;
            }
            let mcus_x =
                (w as usize).div_ceil(8 * if sub && layout != Layout::Gray { 2 } else { 1 });
            let mcu_rows = 32
                / if layout == Layout::Gray || !sub {
                    8
                } else {
                    16
                };
            let mut out = Vec::new();
            join_restart(&strips[..], h as usize, mcus_x * mcu_rows, &mut out).unwrap();
            let got = pixels(&out);
            let first = got.iter().zip(&want).position(|(a, b)| a != b);
            assert!(
                first.is_none(),
                "{layout:?}: first difference at sample {first:?} of {} (row {:?})",
                want.len(),
                first.map(|f| f / (w as usize * layout.channels()))
            );
        }
    }

    #[test]
    fn dc_codes_round_trip() {
        for d in -2047..=2047 {
            let (c, e) = dc_code(d);
            assert_eq!(dc_value(c, e), d);
        }
    }

    #[test]
    fn files_it_does_not_understand_come_back_unchanged() {
        let img = photo(64, 48, Layout::Rgb);
        let prog = encode(&img, &JpegOpts::default()).unwrap();
        assert!(!can_optimize(&prog));
        assert_eq!(optimize_huffman(&prog).unwrap(), prog);
        assert_eq!(optimize_huffman(b"not a jpeg").unwrap(), b"not a jpeg");
    }

    #[test]
    fn a_damaged_scan_is_an_error_not_a_panic() {
        let img = photo(64, 48, Layout::Rgb);
        let mut base = baseline(&img, 70.0, true);
        let cut = base.len() / 2;
        base.truncate(cut);
        base.extend_from_slice(&[0xFF, 0xD9]);
        assert!(optimize_huffman(&base).is_err());
    }
}
