//! WebP RIFF chunk walking. Image chunks (VP8, VP8L, ALPH, ANIM, ANMF) are
//! copied untouched.

use super::exif::{self, Policy};
use super::{Log, MetaError, Report, StripLevel, corrupt, le32, xmp};

const FLAG_ICC: u8 = 0x20;
const FLAG_ALPHA: u8 = 0x10;
const FLAG_EXIF: u8 = 0x08;
const FLAG_XMP: u8 = 0x04;
const FLAG_ANIM: u8 = 0x02;

struct Chunk {
    id: [u8; 4],
    start: usize,
    size: usize,
}

impl Chunk {
    fn data<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        &b[self.start + 8..self.start + 8 + self.size]
    }
    /// Header, data and the pad byte when the size is odd and the pad exists.
    fn whole<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        let end = (self.start + 8 + self.size + (self.size & 1)).min(b.len());
        &b[self.start..end]
    }
}

struct Parsed {
    chunks: Vec<Chunk>,
    /// End of the RIFF payload as declared (clamped to the file).
    end: usize,
}

fn parse(b: &[u8]) -> Result<Parsed, MetaError> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return Err(corrupt("the WebP header is missing"));
    }
    let declared = le32(b, 4).unwrap_or(0) as usize;
    let end = declared.saturating_add(8).min(b.len());
    if end < 12 {
        return Err(corrupt("the WebP size field is wrong"));
    }
    let mut chunks = Vec::new();
    let mut i = 12;
    while i + 8 <= end {
        let size = le32(b, i + 4).unwrap_or(0) as usize;
        let data_end = i + 8 + size;
        if data_end > end {
            return Err(corrupt("a WebP chunk runs past the end of the file"));
        }
        chunks.push(Chunk {
            id: [b[i], b[i + 1], b[i + 2], b[i + 3]],
            start: i,
            size,
        });
        i = data_end + (size & 1);
    }
    Ok(Parsed { chunks, end })
}

fn exif_payload(d: &[u8]) -> (usize, &[u8]) {
    if d.starts_with(b"Exif\0\0") {
        (6, &d[6..])
    } else {
        (0, d)
    }
}

pub(crate) fn inspect(b: &[u8], r: &mut Report) -> Result<(), MetaError> {
    let parsed = parse(b)?;
    let mut exif_done = false;
    for c in &parsed.chunks {
        let d = c.data(b);
        match &c.id {
            b"EXIF" => {
                if !exif_done {
                    exif_done = true;
                    r.exif_present = true;
                    exif::inspect(exif_payload(d).1, false, r);
                }
            }
            b"XMP " => xmp::inspect(d, r),
            b"ICCP" => r.icc = true,
            b"C2PA" => r.c2pa = true,
            b"VP8X" => {
                if let Some(&f) = d.first()
                    && f & FLAG_ICC != 0
                {
                    r.icc = true;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn extract_exif(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    let c = parsed.chunks.iter().find(|c| &c.id == b"EXIF")?;
    let (_, blob) = exif_payload(c.data(b));
    exif::parse_header(blob)?;
    Some(blob.to_vec())
}

pub(crate) fn extract_xmp(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    let c = parsed.chunks.iter().find(|c| &c.id == b"XMP ")?;
    Some(c.data(b).to_vec())
}

pub(crate) fn extract_icc(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    let c = parsed.chunks.iter().find(|c| &c.id == b"ICCP")?;
    Some(c.data(b).to_vec())
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() & 1 == 1 {
        out.push(0);
    }
}

fn finish_riff(out: &mut [u8]) -> Result<(), MetaError> {
    let size = out.len() - 8;
    let size = u32::try_from(size).map_err(|_| {
        MetaError::Unsupported(
            "This WebP is larger than 4 GB, which the format does not allow.".to_string(),
        )
    })?;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(())
}

pub(crate) fn strip(b: &[u8], level: StripLevel) -> Result<(Vec<u8>, Log), MetaError> {
    let parsed = parse(b)?;
    let mut log = Log::default();
    let everything = level == StripLevel::Everything;

    let mut fresh: Option<Vec<u8>> = None;
    if everything
        && let Some(c) = parsed.chunks.iter().find(|c| &c.id == b"EXIF")
        && let Some(o) = exif::orientation(exif_payload(c.data(b)).1)
        && o > 1
    {
        fresh = Some(exif::orientation_only(o));
    }
    let has_vp8x = parsed.chunks.iter().any(|c| &c.id == b"VP8X");

    let mut out = Vec::with_capacity(b.len());
    out.extend_from_slice(b"RIFF\0\0\0\0WEBP");
    let mut kept_exif = false;
    let mut kept_xmp = false;
    let mut vp8x_at = None;
    for c in &parsed.chunks {
        let d = c.data(b);
        match &c.id {
            b"VP8X" => {
                vp8x_at = Some(out.len() + 8);
                out.extend_from_slice(c.whole(b));
            }
            b"EXIF" => {
                if everything {
                    if has_vp8x && let Some(f) = fresh.take() {
                        push_chunk(&mut out, b"EXIF", &f);
                        kept_exif = true;
                    }
                    continue;
                }
                let (skip, blob) = exif_payload(d);
                let mut blob = blob.to_vec();
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
                        let mut payload = d[..skip].to_vec();
                        payload.extend_from_slice(&blob);
                        push_chunk(&mut out, b"EXIF", &payload);
                        kept_exif = true;
                    }
                    Err(e) => {
                        if level == StripLevel::Gps {
                            return Err(e);
                        }
                        log.warn("The EXIF block was damaged, so all of it was removed. The photo may lose its rotation.");
                    }
                }
            }
            b"XMP " => {
                if level == StripLevel::Gps {
                    let mut nd = d.to_vec();
                    log.xmp_gps += xmp::blank_gps(&mut nd);
                    push_chunk(&mut out, b"XMP ", &nd);
                    kept_xmp = true;
                } else {
                    log.xmp = true;
                }
            }
            b"C2PA" => {
                if level == StripLevel::Gps {
                    out.extend_from_slice(c.whole(b));
                } else {
                    log.c2pa = true;
                }
            }
            b"ICCP" | b"ALPH" | b"ANIM" | b"ANMF" | b"VP8 " | b"VP8L" => {
                out.extend_from_slice(c.whole(b));
            }
            _ => {
                if everything {
                    log.extra("other WebP chunks");
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
    if let Some(at) = vp8x_at
        && let Some(f) = out.get_mut(at)
    {
        if kept_exif {
            *f |= FLAG_EXIF;
        } else {
            *f &= !FLAG_EXIF;
        }
        if kept_xmp {
            *f |= FLAG_XMP;
        } else {
            *f &= !FLAG_XMP;
        }
    }
    if level == StripLevel::Gps && parsed.end < b.len() {
        // Trailing bytes after the RIFF payload are not part of the RIFF size.
        let riff = out.len() - (b.len() - parsed.end);
        let size = (riff - 8) as u32;
        out[4..8].copy_from_slice(&size.to_le_bytes());
    } else {
        finish_riff(&mut out)?;
    }
    if let Some(f) = fresh {
        // A bare VP8 or VP8L file needs a VP8X header before it can hold EXIF.
        out = embed(&out, None, Some(&f), None)?;
    }
    Ok((out, log))
}

// ---------------------------------------------------------------------------
// Embed

struct Canvas {
    w: u32,
    h: u32,
    alpha: bool,
}

fn canvas_of(b: &[u8], parsed: &Parsed) -> Result<Canvas, MetaError> {
    let mut alpha = false;
    for c in &parsed.chunks {
        let d = c.data(b);
        match &c.id {
            b"ALPH" => alpha = true,
            b"VP8L" => {
                if d.len() < 5 || d[0] != 0x2F {
                    return Err(corrupt("the lossless WebP header is invalid"));
                }
                let bits = le32(d, 1).unwrap_or(0);
                return Ok(Canvas {
                    w: (bits & 0x3FFF) + 1,
                    h: ((bits >> 14) & 0x3FFF) + 1,
                    alpha: alpha || (bits >> 28) & 1 == 1,
                });
            }
            b"VP8 " => {
                if d.len() < 10 || d[3..6] != [0x9D, 0x01, 0x2A] {
                    return Err(corrupt("the lossy WebP header is invalid"));
                }
                let w = u16::from_le_bytes([d[6], d[7]]) as u32 & 0x3FFF;
                let h = u16::from_le_bytes([d[8], d[9]]) as u32 & 0x3FFF;
                return Ok(Canvas { w, h, alpha });
            }
            _ => {}
        }
    }
    Err(corrupt("the WebP has no image data"))
}

pub(crate) fn embed(
    webp: &[u8],
    icc: Option<&[u8]>,
    exif_blob: Option<&[u8]>,
    xmp_blob: Option<&[u8]>,
) -> Result<Vec<u8>, MetaError> {
    if icc.is_none() && exif_blob.is_none() && xmp_blob.is_none() {
        return Ok(webp.to_vec());
    }
    let parsed = parse(webp)?;
    let old_vp8x = parsed.chunks.iter().find(|c| &c.id == b"VP8X");
    let (mut flags, cw, ch) = match old_vp8x {
        Some(c) => {
            let d = c.data(webp);
            if d.len() < 10 {
                return Err(corrupt("the WebP VP8X header is too short"));
            }
            let w = u32::from_le_bytes([d[4], d[5], d[6], 0]) + 1;
            let h = u32::from_le_bytes([d[7], d[8], d[9], 0]) + 1;
            (d[0], w, h)
        }
        None => {
            let cv = canvas_of(webp, &parsed)?;
            (if cv.alpha { FLAG_ALPHA } else { 0 }, cv.w, cv.h)
        }
    };
    if parsed.chunks.iter().any(|c| &c.id == b"ANIM") {
        flags |= FLAG_ANIM;
    }
    if icc.is_some() {
        flags |= FLAG_ICC;
    }
    if exif_blob.is_some() {
        flags |= FLAG_EXIF;
    }
    if xmp_blob.is_some() {
        flags |= FLAG_XMP;
    }
    let mut out = Vec::with_capacity(webp.len() + 64);
    out.extend_from_slice(b"RIFF\0\0\0\0WEBP");
    let mut vp8x = [0u8; 10];
    vp8x[0] = flags;
    vp8x[4..7].copy_from_slice(&(cw - 1).to_le_bytes()[..3]);
    vp8x[7..10].copy_from_slice(&(ch - 1).to_le_bytes()[..3]);
    push_chunk(&mut out, b"VP8X", &vp8x);
    let old_icc = parsed.chunks.iter().find(|c| &c.id == b"ICCP");
    match (icc, old_icc) {
        (Some(p), _) => push_chunk(&mut out, b"ICCP", p),
        (None, Some(c)) => out.extend_from_slice(c.whole(webp)),
        (None, None) => {}
    }
    for c in &parsed.chunks {
        if matches!(&c.id, b"VP8X" | b"ICCP" | b"EXIF" | b"XMP ") {
            continue;
        }
        out.extend_from_slice(c.whole(webp));
    }
    match exif_blob {
        Some(p) => push_chunk(&mut out, b"EXIF", p),
        None => {
            if let Some(c) = parsed.chunks.iter().find(|c| &c.id == b"EXIF") {
                out.extend_from_slice(c.whole(webp));
            }
        }
    }
    match xmp_blob {
        Some(p) => push_chunk(&mut out, b"XMP ", p),
        None => {
            if let Some(c) = parsed.chunks.iter().find(|c| &c.id == b"XMP ") {
                out.extend_from_slice(c.whole(webp));
            }
        }
    }
    finish_riff(&mut out)?;
    Ok(out)
}
