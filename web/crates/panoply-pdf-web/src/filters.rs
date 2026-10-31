//! Stream filters: the lossless ones PDF stores text, fonts and raw images with.
//!
//! Image codecs (DCT, JPX, JBIG2, CCITT) are not decoded here. [`decode`] stops at the first one
//! and says so, which is what lets callers treat those streams as opaque bytes.

use miniz_oxide::deflate::compress_to_vec_zlib;
use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_zlib_with_limit};

use crate::error::{PdfError, Result};
use crate::obj::{Dict, Obj};

/// The name of one filter, with the abbreviations inline images use folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// zlib deflate.
    Flate,
    /// Lempel-Ziv-Welch.
    Lzw,
    /// ASCII hexadecimal.
    AsciiHex,
    /// ASCII base 85.
    Ascii85,
    /// Run-length.
    RunLength,
    /// JPEG.
    Dct,
    /// JPEG 2000.
    Jpx,
    /// JBIG2.
    Jbig2,
    /// CCITT fax.
    Ccitt,
    /// A crypt filter.
    Crypt,
    /// Something else.
    Unknown,
}

impl Filter {
    /// Map a filter name to the enum.
    pub fn from_name(n: &[u8]) -> Filter {
        match n {
            b"FlateDecode" | b"Fl" => Filter::Flate,
            b"LZWDecode" | b"LZW" => Filter::Lzw,
            b"ASCIIHexDecode" | b"AHx" => Filter::AsciiHex,
            b"ASCII85Decode" | b"A85" => Filter::Ascii85,
            b"RunLengthDecode" | b"RL" => Filter::RunLength,
            b"DCTDecode" | b"DCT" => Filter::Dct,
            b"JPXDecode" => Filter::Jpx,
            b"JBIG2Decode" => Filter::Jbig2,
            b"CCITTFaxDecode" | b"CCF" => Filter::Ccitt,
            b"Crypt" => Filter::Crypt,
            _ => Filter::Unknown,
        }
    }

    /// True for codecs that produce pixels, which this module does not decode.
    pub fn is_image_codec(self) -> bool {
        matches!(
            self,
            Filter::Dct | Filter::Jpx | Filter::Jbig2 | Filter::Ccitt
        )
    }
}

/// The filter chain of a stream dictionary with its decode parameters. References inside the
/// chain must already be resolved by the caller (`Doc::filters`).
pub fn chain(dict: &Dict, resolve: &dyn Fn(&Obj) -> Obj) -> Vec<(Filter, Dict)> {
    let f = dict
        .get("Filter")
        .or_else(|| {
            dict.get("F")
                .filter(|o| !matches!(o, Obj::Str(_) | Obj::Dict(_)))
        })
        .map(resolve)
        .unwrap_or(Obj::Null);
    let p = dict
        .get("DecodeParms")
        .or_else(|| dict.get("DP"))
        .map(resolve)
        .unwrap_or(Obj::Null);
    let names: Vec<Obj> = match f {
        Obj::Name(_) => vec![f],
        Obj::Array(a) => a.iter().map(resolve).collect(),
        _ => Vec::new(),
    };
    let parms: Vec<Obj> = match p {
        Obj::Dict(_) => vec![p],
        Obj::Array(a) => a.iter().map(resolve).collect(),
        _ => Vec::new(),
    };
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let filter = n
                .as_name()
                .map(Filter::from_name)
                .unwrap_or(Filter::Unknown);
            let d = match parms.get(i) {
                Some(Obj::Dict(d)) => d.clone(),
                _ => Dict::new(),
            };
            (filter, d)
        })
        .collect()
}

/// Decode `data` through the lossless filters of `chain`, stopping with
/// [`PdfError::Unsupported`] at an image codec. `limit` bounds the size of any one result, which
/// is what keeps a decompression bomb from taking the machine's memory. With `lenient`, a Flate
/// stream that is cut off or corrupt yields what could be decoded instead of an error.
pub fn decode(
    chain: &[(Filter, Dict)],
    data: &[u8],
    limit: usize,
    lenient: bool,
) -> Result<Vec<u8>> {
    let mut cur: Vec<u8> = Vec::new();
    let mut first = true;
    for (f, parms) in chain {
        let input: &[u8] = if first { data } else { &cur };
        first = false;
        let mut next = match f {
            Filter::Flate => inflate(input, limit, lenient)?,
            Filter::Lzw => lzw(input, parms, limit)?,
            Filter::AsciiHex => ascii_hex(input),
            Filter::Ascii85 => ascii85(input)?,
            Filter::RunLength => run_length(input, limit)?,
            other => {
                return Err(PdfError::Unsupported(format!(
                    "This stream uses the {other:?} filter, which the engine does not decode."
                )));
            }
        };
        if matches!(f, Filter::Flate | Filter::Lzw) {
            next = unpredict(next, parms)?;
        }
        cur = next;
    }
    if first {
        return Ok(data.to_vec());
    }
    Ok(cur)
}

/// Inflate zlib data (falling back to raw deflate), with a size limit.
pub fn inflate(data: &[u8], limit: usize, lenient: bool) -> Result<Vec<u8>> {
    match decompress_to_vec_zlib_with_limit(data, limit) {
        Ok(v) => Ok(v),
        Err(e) => {
            if e.status == TINFLStatus::HasMoreOutput {
                return Err(PdfError::Unsupported(
                    "A compressed stream in this PDF expands to more than the safety limit, so it was not opened."
                        .into(),
                ));
            }
            // Some writers omit the zlib header: try raw deflate.
            if let Ok(v) = miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit) {
                return Ok(v);
            }
            if lenient && !e.output.is_empty() {
                return Ok(e.output);
            }
            if lenient {
                // Try after skipping leading junk (a stray newline before the zlib header).
                let skip = data
                    .iter()
                    .take_while(|b| matches!(**b, b'\r' | b'\n'))
                    .count();
                if skip > 0
                    && let Ok(v) = decompress_to_vec_zlib_with_limit(&data[skip..], limit)
                {
                    return Ok(v);
                }
            }
            Err(PdfError::damaged("a compressed stream is corrupt"))
        }
    }
}

/// Deflate with zlib framing at the best level miniz offers.
pub fn deflate(data: &[u8], level: u8) -> Vec<u8> {
    compress_to_vec_zlib(data, level)
}

fn lzw(data: &[u8], parms: &Dict, limit: usize) -> Result<Vec<u8>> {
    let early = parms.int("EarlyChange").unwrap_or(1) != 0;
    let mut dec = if early {
        weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
    } else {
        weezl::decode::Decoder::new(weezl::BitOrder::Msb, 8)
    };
    let mut out = Vec::new();
    let mut input = data;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let r = dec.decode_bytes(input, &mut buf);
        out.extend_from_slice(&buf[..r.consumed_out]);
        if out.len() > limit {
            return Err(PdfError::Unsupported(
                "A compressed stream in this PDF expands to more than the safety limit.".into(),
            ));
        }
        input = &input[r.consumed_in..];
        match r.status {
            Ok(weezl::LzwStatus::Ok) => {
                if r.consumed_in == 0 && r.consumed_out == 0 {
                    break;
                }
            }
            Ok(weezl::LzwStatus::NoProgress) | Ok(weezl::LzwStatus::Done) => break,
            Err(_) => return Err(PdfError::damaged("an LZW stream is corrupt")),
        }
    }
    Ok(out)
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut hi: Option<u8> = None;
    for &c in data {
        if c == b'>' {
            break;
        }
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => continue,
        };
        match hi.take() {
            Some(h) => out.push(h << 4 | v),
            None => hi = Some(v),
        }
    }
    if let Some(h) = hi {
        out.push(h << 4);
    }
    out
}

fn ascii85(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5);
    let mut group = [0u32; 5];
    let mut n = 0;
    let mut i = 0;
    // Skip the optional `<~` prefix.
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let c = data[i];
        i += 1;
        match c {
            b'~' => break,
            b'z' if n == 0 => out.extend_from_slice(&[0, 0, 0, 0]),
            b'!'..=b'u' => {
                group[n] = u32::from(c - b'!');
                n += 1;
                if n == 5 {
                    let v = group.iter().fold(0u64, |a, &g| a * 85 + u64::from(g));
                    let v = v as u32;
                    out.extend_from_slice(&v.to_be_bytes());
                    n = 0;
                }
            }
            c if crate::obj::is_ws(c) => {}
            _ => return Err(PdfError::damaged("an ASCII85 stream is corrupt")),
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |a, &g| a * 85 + u64::from(g)) as u32;
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
    }
    Ok(out)
}

fn run_length(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let l = data[i];
        i += 1;
        if l == 128 {
            break;
        }
        if l < 128 {
            let n = usize::from(l) + 1;
            let end = (i + n).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else {
            let n = 257 - usize::from(l);
            if let Some(&b) = data.get(i) {
                out.extend(std::iter::repeat_n(b, n));
            }
            i += 1;
        }
        if out.len() > limit {
            return Err(PdfError::Unsupported(
                "A compressed stream in this PDF expands to more than the safety limit.".into(),
            ));
        }
    }
    Ok(out)
}

/// Undo the PNG or TIFF predictor named in the decode parameters.
pub fn unpredict(data: Vec<u8>, parms: &Dict) -> Result<Vec<u8>> {
    let predictor = parms.int("Predictor").unwrap_or(1);
    if predictor <= 1 {
        return Ok(data);
    }
    let colors = parms.int("Colors").unwrap_or(1).clamp(1, 64) as usize;
    let bpc = parms.int("BitsPerComponent").unwrap_or(8).clamp(1, 16) as usize;
    let columns = parms.int("Columns").unwrap_or(1).clamp(1, 1 << 24) as usize;
    let bpp = (colors * bpc).div_ceil(8).max(1);
    let row_bytes = (colors * bpc * columns).div_ceil(8);
    if predictor == 2 {
        if bpc != 8 {
            return Err(PdfError::Unsupported(
                "A stream in this PDF uses a TIFF predictor on samples that are not 8 bits.".into(),
            ));
        }
        let mut data = data;
        for row in data.chunks_mut(row_bytes) {
            for i in bpp..row.len() {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        return Ok(data);
    }
    // PNG predictors: every row starts with a filter type byte.
    let stride = row_bytes + 1;
    let rows = data.len().div_ceil(stride);
    let mut out = vec![0u8; rows * row_bytes];
    let mut prev = vec![0u8; row_bytes];
    let mut cur = vec![0u8; row_bytes];
    let mut written = 0;
    for r in 0..rows {
        let start = r * stride;
        let ft = data[start];
        let src = &data[start + 1..(start + stride).min(data.len())];
        cur[..src.len()].copy_from_slice(src);
        for b in cur[src.len()..].iter_mut() {
            *b = 0;
        }
        match ft {
            0 => {}
            1 => {
                for i in bpp..row_bytes {
                    cur[i] = cur[i].wrapping_add(cur[i - bpp]);
                }
            }
            2 => {
                for i in 0..row_bytes {
                    cur[i] = cur[i].wrapping_add(prev[i]);
                }
            }
            3 => {
                for i in 0..row_bytes {
                    let left = if i >= bpp { u16::from(cur[i - bpp]) } else { 0 };
                    cur[i] = cur[i].wrapping_add(((left + u16::from(prev[i])) / 2) as u8);
                }
            }
            4 => {
                for i in 0..row_bytes {
                    let a = if i >= bpp { cur[i - bpp] } else { 0 };
                    let b = prev[i];
                    let c = if i >= bpp { prev[i - bpp] } else { 0 };
                    cur[i] = cur[i].wrapping_add(paeth(a, b, c));
                }
            }
            _ => return Err(PdfError::damaged("a stream has an invalid PNG filter row")),
        }
        out[written..written + row_bytes].copy_from_slice(&cur);
        written += row_bytes;
        std::mem::swap(&mut prev, &mut cur);
    }
    out.truncate(written);
    Ok(out)
}

/// The Paeth predictor of the PNG specification.
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Apply the PNG "Up" predictor to every row (type byte 2). Used to compress xref streams.
pub fn predict_up(data: &[u8], row_bytes: usize) -> Vec<u8> {
    let rows = data.len() / row_bytes;
    let mut out = Vec::with_capacity(rows * (row_bytes + 1));
    for r in 0..rows {
        out.push(2);
        let cur = &data[r * row_bytes..(r + 1) * row_bytes];
        if r == 0 {
            out.extend_from_slice(cur);
        } else {
            let prev = &data[(r - 1) * row_bytes..r * row_bytes];
            out.extend(cur.iter().zip(prev).map(|(c, p)| c.wrapping_sub(*p)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flate_round_trip_and_limit() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 7) as u8).collect();
        let z = deflate(&data, 6);
        assert_eq!(inflate(&z, 1 << 20, false).unwrap(), data);
        assert!(inflate(&z, 1000, false).is_err());
    }

    #[test]
    fn truncated_flate_is_partial_only_when_lenient() {
        let data = vec![7u8; 50_000];
        let z = deflate(&data, 6);
        let cut = &z[..z.len() / 2];
        assert!(inflate(cut, 1 << 20, false).is_err());
        let _ = inflate(cut, 1 << 20, true); // must not panic
    }

    #[test]
    fn ascii_filters() {
        assert_eq!(ascii_hex(b"48 65 6C 6c 6F>"), b"Hello");
        assert_eq!(ascii85(b"87cURD]i,\"Ebo80~>").unwrap(), b"Hello World!");
        assert_eq!(
            run_length(&[2, 1, 2, 3, 254, 9, 128], 100).unwrap(),
            [1, 2, 3, 9, 9, 9]
        );
    }

    #[test]
    fn png_up_predictor_round_trips() {
        let row = 6;
        let raw: Vec<u8> = (0..60).map(|i| (i * 7 % 251) as u8).collect();
        let predicted = predict_up(&raw, row);
        let mut parms = Dict::new();
        parms.set("Predictor", Obj::Int(12));
        parms.set("Columns", Obj::Int(row as i64));
        assert_eq!(unpredict(predicted, &parms).unwrap(), raw);
    }

    #[test]
    fn paeth_matches_the_spec_cases() {
        assert_eq!(paeth(10, 20, 10), 20);
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(100, 100, 100), 100);
    }

    #[test]
    fn image_codecs_stop_the_chain() {
        let chain = [(Filter::Dct, Dict::new())];
        assert!(matches!(
            decode(&chain, b"x", 10, false),
            Err(PdfError::Unsupported(_))
        ));
    }
}
