//! PNG chunk walking. IDAT and every chunk a decoder needs are copied with
//! their original CRC. Rewritten chunks get a CRC computed here.

use super::exif::{self, Policy};
use super::{Log, MetaError, Report, StripLevel, be32, corrupt, crc32, xmp};

const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const XMP_KEY: &[u8] = b"XML:com.adobe.xmp";
const RAW_PROFILE: &[u8] = b"Raw profile type";

/// Chunks a decoder needs to show the image the same way.
const DECODER: &[&[u8; 4]] = &[
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"iCCP", b"sBIT",
    b"bKGD", b"pHYs", b"cICP", b"mDCV", b"cLLI", b"acTL", b"fcTL", b"fdAT",
];

struct Chunk {
    typ: [u8; 4],
    start: usize,
    len: usize,
}

impl Chunk {
    fn data<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        &b[self.start + 8..self.start + 8 + self.len]
    }
    fn whole<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        &b[self.start..self.start + self.len + 12]
    }
}

struct Parsed {
    chunks: Vec<Chunk>,
    /// One past the last byte of the last chunk (IEND when present).
    end: usize,
}

fn parse(b: &[u8]) -> Result<Parsed, MetaError> {
    if !b.starts_with(SIG) {
        return Err(corrupt("the PNG signature is missing"));
    }
    let mut chunks = Vec::new();
    let mut i = SIG.len();
    while i < b.len() {
        if i + 12 > b.len() {
            return Err(corrupt("the PNG ends in the middle of a chunk"));
        }
        let len = be32(b, i).unwrap_or(0) as usize;
        let end = i + 12 + len;
        if end > b.len() {
            return Err(corrupt("a PNG chunk runs past the end of the file"));
        }
        let typ = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        chunks.push(Chunk { typ, start: i, len });
        i = end;
        if &typ == b"IEND" {
            break;
        }
    }
    Ok(Parsed { chunks, end: i })
}

/// Split a text chunk into keyword and the rest.
fn keyword(data: &[u8]) -> Option<(&[u8], usize)> {
    let k = data.iter().position(|&c| c == 0)?;
    if k == 0 || k > 79 {
        return None;
    }
    Some((&data[..k], k + 1))
}

struct Itxt {
    compressed: bool,
    text: std::ops::Range<usize>,
}

fn itxt(data: &[u8]) -> Option<(&[u8], Itxt)> {
    let (kw, at) = keyword(data)?;
    let compressed = *data.get(at)? != 0;
    let mut p = at + 2;
    for _ in 0..2 {
        p += data.get(p..)?.iter().position(|&c| c == 0)? + 1;
    }
    Some((
        kw,
        Itxt {
            compressed,
            text: p..data.len(),
        },
    ))
}

fn push_chunk(out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[typ, data]).to_be_bytes());
}

pub(crate) fn inspect(b: &[u8], r: &mut Report) -> Result<(), MetaError> {
    let parsed = parse(b)?;
    let mut exif_done = false;
    for c in &parsed.chunks {
        let d = c.data(b);
        match &c.typ {
            b"eXIf" => {
                if !exif_done {
                    exif_done = true;
                    r.exif_present = true;
                    exif::inspect(d, false, r);
                }
            }
            b"iCCP" => r.icc = true,
            b"caBX" => r.c2pa = true,
            b"iTXt" => {
                if let Some((kw, it)) = itxt(d) {
                    if kw == XMP_KEY {
                        if it.compressed {
                            r.xmp = true;
                        } else {
                            xmp::inspect(&d[it.text], r);
                        }
                    } else if kw.starts_with(RAW_PROFILE) {
                        raw_profile(kw, r);
                    }
                }
            }
            b"tEXt" | b"zTXt" => {
                if let Some((kw, at)) = keyword(d) {
                    if kw.starts_with(RAW_PROFILE) {
                        raw_profile(kw, r);
                    } else if &c.typ == b"tEXt" {
                        let text = String::from_utf8_lossy(&d[at..]).trim().to_string();
                        match kw {
                            b"Software" if r.software.is_none() => r.software = Some(text),
                            b"Creation Time" if r.date_taken.is_none() => r.date_taken = Some(text),
                            b"Author" => r.owner.push(("Author".to_string(), text)),
                            b"Copyright" => r.owner.push(("Copyright".to_string(), text)),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn raw_profile(kw: &[u8], r: &mut Report) {
    let k = String::from_utf8_lossy(kw).to_lowercase();
    if k.ends_with("exif") || k.ends_with("app1") {
        r.exif_present = true;
    } else if k.ends_with("xmp") {
        r.xmp = true;
    } else if k.ends_with("iptc") {
        r.iptc = true;
    }
}

pub(crate) fn extract_exif(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    parsed
        .chunks
        .iter()
        .find(|c| &c.typ == b"eXIf")
        .map(|c| c.data(b))
        .filter(|d| exif::parse_header(d).is_some())
        .map(<[u8]>::to_vec)
}

pub(crate) fn extract_xmp(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    for c in parsed.chunks.iter().filter(|c| &c.typ == b"iTXt") {
        let d = c.data(b);
        if let Some((kw, it)) = itxt(d)
            && kw == XMP_KEY
            && !it.compressed
        {
            return Some(d[it.text].to_vec());
        }
    }
    None
}

pub(crate) fn icc_compressed(b: &[u8]) -> Option<(String, Vec<u8>)> {
    let parsed = parse(b).ok()?;
    let c = parsed.chunks.iter().find(|c| &c.typ == b"iCCP")?;
    let d = c.data(b);
    let (name, at) = keyword(d)?;
    // One byte of compression method follows the name.
    let rest = d.get(at + 1..)?;
    Some((String::from_utf8_lossy(name).to_string(), rest.to_vec()))
}

pub(crate) fn strip(b: &[u8], level: StripLevel) -> Result<(Vec<u8>, Log), MetaError> {
    let parsed = parse(b)?;
    let mut log = Log::default();
    let everything = level == StripLevel::Everything;

    let mut fresh: Option<Vec<u8>> = None;
    if everything
        && let Some(c) = parsed.chunks.iter().find(|c| &c.typ == b"eXIf")
        && let Some(o) = exif::orientation(c.data(b))
        && o > 1
    {
        fresh = Some(exif::orientation_only(o));
    }

    let mut out = Vec::with_capacity(b.len());
    out.extend_from_slice(SIG);
    for c in &parsed.chunks {
        let d = c.data(b);
        if &c.typ == b"IDAT"
            && let Some(f) = fresh.take()
        {
            push_chunk(&mut out, b"eXIf", &f);
        }
        if DECODER.iter().any(|t| **t == c.typ) {
            out.extend_from_slice(c.whole(b));
            continue;
        }
        match &c.typ {
            b"eXIf" => {
                if everything {
                    continue;
                }
                let mut blob = d.to_vec();
                let pol = if level == StripLevel::Gps {
                    Policy::gps_only()
                } else {
                    Policy::private()
                };
                match exif::filter(&mut blob, &pol, None, 0) {
                    Ok((t, zeroed)) => {
                        log.tally.add(&t);
                        if level != StripLevel::Gps {
                            let keep = exif::trimmed_len(&blob, &zeroed);
                            blob.truncate(keep);
                        }
                        push_chunk(&mut out, b"eXIf", &blob);
                    }
                    Err(e) => {
                        if level == StripLevel::Gps {
                            return Err(e);
                        }
                        log.warn("The EXIF block was damaged, so all of it was removed. The photo may lose its rotation.");
                    }
                }
            }
            b"caBX" => {
                if level == StripLevel::Gps {
                    out.extend_from_slice(c.whole(b));
                } else {
                    log.c2pa = true;
                }
            }
            b"tIME" => {
                if everything {
                    log.extra("modification time");
                } else if level == StripLevel::Gps {
                    out.extend_from_slice(c.whole(b));
                } else {
                    log.extra("modification time");
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" => {
                text_chunk(&mut out, b, c, level, &mut log);
            }
            _ => {
                let ancillary = c.typ[0] & 0x20 != 0;
                if everything && ancillary {
                    log.extra("other PNG chunks");
                } else {
                    out.extend_from_slice(c.whole(b));
                }
            }
        }
    }
    if parsed.end < b.len() {
        let tail = &b[parsed.end..];
        if level == StripLevel::Gps {
            out.extend_from_slice(tail);
        } else if tail.iter().any(|&x| x != 0) {
            log.extra("trailing data");
        }
    }
    Ok((out, log))
}

fn text_chunk(out: &mut Vec<u8>, b: &[u8], c: &Chunk, level: StripLevel, log: &mut Log) {
    let d = c.data(b);
    let kw = keyword(d).map(|(k, _)| k);
    let is_raw = kw.is_some_and(|k| k.starts_with(RAW_PROFILE));
    match level {
        StripLevel::Gps => {
            if is_raw {
                log.extra("embedded metadata text block");
                log.warn("A text block that can carry EXIF could not be edited, so it was removed whole.");
                return;
            }
            if &c.typ == b"iTXt"
                && let Some((k, it)) = itxt(d)
                && k == XMP_KEY
            {
                if it.compressed {
                    log.xmp = true;
                    log.warn(
                        "A compressed XMP block could not be edited, so it was removed whole.",
                    );
                    return;
                }
                let mut nd = d.to_vec();
                log.xmp_gps += xmp::blank_gps(&mut nd[it.text]);
                push_chunk(out, &c.typ, &nd);
                return;
            }
            out.extend_from_slice(c.whole(b));
        }
        StripLevel::Private => {
            if &c.typ == b"iTXt"
                && let Some((k, _)) = itxt(d)
                && k == XMP_KEY
            {
                log.xmp = true;
                return;
            }
            if &c.typ == b"tEXt" && matches!(kw, Some(b"Software") | Some(b"Creation Time")) {
                out.extend_from_slice(c.whole(b));
                return;
            }
            if is_raw {
                log.extra("embedded metadata text block");
            } else {
                log.comments = true;
            }
        }
        StripLevel::Everything => {
            if &c.typ == b"iTXt" && kw.is_some_and(|k| k == XMP_KEY) {
                log.xmp = true;
            } else if is_raw {
                log.extra("embedded metadata text block");
            } else {
                log.comments = true;
            }
        }
    }
}
