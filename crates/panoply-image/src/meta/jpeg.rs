//! JPEG segment walking. Entropy coded data is never looked into: it is found
//! by marker scanning and copied byte for byte.

use super::exif::{self, Policy};
use super::{Log, MetaError, Report, StripLevel, be16, be32, corrupt, gather, scatter, xmp};
use std::ops::Range;

const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXT_NS: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const EXT_HEADER: usize = 35 + 32 + 4 + 4;
const MAX_SECONDARY: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PKind {
    Soi,
    /// A marker segment with a length field.
    Seg(u8),
    /// Entropy coded scan data.
    Scan,
    Eoi,
    /// Stray bytes or standalone markers, copied through.
    Raw,
}

#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub kind: PKind,
    pub range: Range<usize>,
    /// Position of the marker byte (after any fill bytes) for `Seg`.
    pub mpos: usize,
}

impl Piece {
    pub(crate) fn payload(&self) -> Range<usize> {
        self.mpos + 3..self.range.end
    }
}

pub(crate) struct Parsed {
    pub pieces: Vec<Piece>,
    /// One past the first EOI, or the end of the data when there is none.
    pub end: usize,
}

fn skip_entropy(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        match b[i..].iter().position(|&c| c == 0xFF) {
            None => return b.len(),
            Some(p) => i += p,
        }
        match b.get(i + 1) {
            None => return b.len(),
            Some(0) | Some(0xD0..=0xD7) => i += 2,
            Some(0xFF) => i += 1,
            Some(_) => return i,
        }
    }
    b.len()
}

pub(crate) fn parse(b: &[u8]) -> Result<Parsed, MetaError> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return Err(corrupt("the JPEG start marker is missing"));
    }
    let mut pieces = vec![Piece {
        kind: PKind::Soi,
        range: 0..2,
        mpos: 0,
    }];
    let mut i = 2;
    let mut end = b.len();
    while i < b.len() {
        if b[i] != 0xFF {
            let s = i;
            i += b[i..]
                .iter()
                .position(|&c| c == 0xFF)
                .unwrap_or(b.len() - i);
            pieces.push(Piece {
                kind: PKind::Raw,
                range: s..i,
                mpos: s,
            });
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b[j] == 0xFF {
            j += 1;
        }
        if j >= b.len() {
            pieces.push(Piece {
                kind: PKind::Raw,
                range: i..b.len(),
                mpos: i,
            });
            break;
        }
        let m = b[j];
        match m {
            0x00 | 0x01 | 0xD0..=0xD8 => {
                pieces.push(Piece {
                    kind: PKind::Raw,
                    range: i..j + 1,
                    mpos: j,
                });
                i = j + 1;
            }
            0xD9 => {
                pieces.push(Piece {
                    kind: PKind::Eoi,
                    range: i..j + 1,
                    mpos: j,
                });
                i = j + 1;
                end = i;
                break;
            }
            _ => {
                let l =
                    be16(b, j + 1).ok_or_else(|| corrupt("a JPEG segment is cut short"))? as usize;
                if l < 2 {
                    return Err(corrupt("a JPEG segment has an impossible length"));
                }
                let seg_end = j + 1 + l;
                if seg_end > b.len() {
                    return Err(corrupt("a JPEG segment runs past the end of the file"));
                }
                pieces.push(Piece {
                    kind: PKind::Seg(m),
                    range: i..seg_end,
                    mpos: j,
                });
                i = seg_end;
                if m == 0xDA {
                    let s = i;
                    i = skip_entropy(b, i);
                    pieces.push(Piece {
                        kind: PKind::Scan,
                        range: s..i,
                        mpos: s,
                    });
                }
            }
        }
    }
    Ok(Parsed { pieces, end })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SegKind {
    Jfif,
    Jfxx,
    Exif,
    Xmp,
    XmpExt,
    Icc,
    Mpf,
    Flashpix,
    Jumbf,
    Photoshop,
    Adobe,
    Com,
    OtherApp,
    NotApp,
}

pub(crate) fn classify(m: u8, p: &[u8]) -> SegKind {
    match m {
        0xE0 => {
            if p.starts_with(b"JFIF\0") {
                SegKind::Jfif
            } else if p.starts_with(b"JFXX\0") {
                SegKind::Jfxx
            } else {
                SegKind::OtherApp
            }
        }
        0xE1 => {
            if p.starts_with(b"Exif\0\0") {
                SegKind::Exif
            } else if p.starts_with(XMP_NS) {
                SegKind::Xmp
            } else if p.starts_with(XMP_EXT_NS) {
                SegKind::XmpExt
            } else {
                SegKind::OtherApp
            }
        }
        0xE2 => {
            if p.starts_with(b"ICC_PROFILE\0") {
                SegKind::Icc
            } else if p.starts_with(b"MPF\0") {
                SegKind::Mpf
            } else if p.starts_with(b"FPXR\0") {
                SegKind::Flashpix
            } else {
                SegKind::OtherApp
            }
        }
        0xEB => {
            if p.starts_with(b"JP") {
                SegKind::Jumbf
            } else {
                SegKind::OtherApp
            }
        }
        0xED => SegKind::Photoshop,
        0xEE => {
            if p.starts_with(b"Adobe") {
                SegKind::Adobe
            } else {
                SegKind::OtherApp
            }
        }
        0xFE => SegKind::Com,
        0xE3..=0xEF => SegKind::OtherApp,
        _ => SegKind::NotApp,
    }
}

/// Look inside an APP13 Photoshop block. Returns (has IPTC, thumbnail count).
pub(crate) fn photoshop_info(p: &[u8]) -> (bool, usize) {
    let mut i = if p.starts_with(b"Photoshop 3.0\0") {
        14
    } else {
        0
    };
    let mut iptc = false;
    let mut thumbs = 0;
    let mut guard = 0;
    while i + 12 <= p.len() && guard < 4096 {
        guard += 1;
        if &p[i..i + 4] != b"8BIM" {
            break;
        }
        let id = be16(p, i + 4).unwrap_or(0);
        let nlen = p[i + 6] as usize;
        let mut at = i + 7 + nlen;
        if (1 + nlen) % 2 == 1 {
            at += 1;
        }
        let Some(size) = be32(p, at) else { break };
        at += 4;
        match id {
            0x0404 => iptc = true,
            0x0409 | 0x040C => thumbs += 1,
            _ => {}
        }
        let padded = (size as usize).saturating_add((size as usize) & 1);
        i = match at.checked_add(padded) {
            Some(n) => n,
            None => break,
        };
    }
    (iptc, thumbs)
}

fn has_c2pa(p: &[u8]) -> bool {
    super::find(p, b"c2pa").is_some()
}

// ---------------------------------------------------------------------------
// MPF and secondary images

pub(crate) struct MpEntry {
    pub attr: u32,
    pub abs: usize,
}

fn find_mpf(b: &[u8], parsed: &Parsed) -> Vec<MpEntry> {
    for p in &parsed.pieces {
        if let PKind::Seg(0xE2) = p.kind {
            let pl = p.payload();
            if b.get(pl.clone()).is_some_and(|s| s.starts_with(b"MPF\0")) {
                return parse_mpf(b, pl.start + 4, pl.end);
            }
        }
    }
    Vec::new()
}

fn parse_mpf(b: &[u8], base: usize, end: usize) -> Vec<MpEntry> {
    let mut out = Vec::new();
    let Some(blob) = b.get(base..end) else {
        return out;
    };
    let Some((o, off0)) = exif::parse_header(blob) else {
        return out;
    };
    let Ok(ifd) = exif::read_ifd(blob, o, off0) else {
        return out;
    };
    for e in &ifd.entries {
        if e.tag == 0xB002 {
            let Some(r) = e.data.clone() else { continue };
            let n = (r.len() / 16).min(MAX_SECONDARY);
            for k in 0..n {
                let at = r.start + k * 16;
                let (Some(attr), Some(off)) = (o.u32(blob, at), o.u32(blob, at + 8)) else {
                    continue;
                };
                if k == 0 {
                    continue;
                }
                out.push(MpEntry {
                    attr,
                    abs: base + off as usize,
                });
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SecKind {
    GainMap,
    Preview,
    Image,
    Video,
    Trailing,
}

pub(crate) struct Secondary {
    pub range: Range<usize>,
    pub kind: SecKind,
}

fn xmp_mentions_gain_map(b: &[u8], parsed: &Parsed) -> bool {
    for p in &parsed.pieces {
        if let PKind::Seg(0xE1) = p.kind {
            let pl = p.payload();
            if let Some(s) = b.get(pl)
                && s.starts_with(XMP_NS)
                && (xmp::mentions_gain_map(s) || super::find(s, b"GainMap").is_some())
            {
                return true;
            }
        }
    }
    false
}

/// Find the images and data stored after the first EOI.
pub(crate) fn secondaries(b: &[u8], parsed: &Parsed, mpf: &[MpEntry]) -> Vec<Secondary> {
    let mut out = Vec::new();
    let mut pos = parsed.end;
    let primary_gain = xmp_mentions_gain_map(b, parsed);
    let mut gain_assigned = false;
    while pos < b.len() && out.len() < MAX_SECONDARY {
        let Some(nz) = b[pos..].iter().position(|&c| c != 0) else {
            break;
        };
        if b[pos + nz..].starts_with(&[0xFF, 0xD8, 0xFF]) {
            pos += nz;
        }
        if b[pos..].starts_with(&[0xFF, 0xD8, 0xFF])
            && let Ok(p) = parse(&b[pos..])
        {
            let end = pos + p.end;
            let own_gain = xmp_mentions_gain_map(&b[pos..end], &p);
            let attr = mpf
                .iter()
                .find(|m| m.abs == pos)
                .map(|m| m.attr & 0x00FF_FFFF);
            let kind = if own_gain {
                SecKind::GainMap
            } else if matches!(attr, Some(0x010001) | Some(0x010002)) {
                SecKind::Preview
            } else if primary_gain
                && !gain_assigned
                && attr.is_none_or(|a| a != 0x010001 && a != 0x010002)
            {
                SecKind::GainMap
            } else {
                SecKind::Image
            };
            if kind == SecKind::GainMap {
                gain_assigned = true;
            }
            out.push(Secondary {
                range: pos..end,
                kind,
            });
            pos = end;
            continue;
        }
        let rest = &b[pos..];
        let kind = if rest.len() >= 8 && &rest[4..8] == b"ftyp" {
            SecKind::Video
        } else {
            SecKind::Trailing
        };
        out.push(Secondary {
            range: pos..b.len(),
            kind,
        });
        break;
    }
    out
}

// ---------------------------------------------------------------------------
// Inspect

pub(crate) fn inspect(b: &[u8], r: &mut Report) -> Result<(), MetaError> {
    let parsed = parse(b)?;
    let mut exif_done = false;
    let mut xmp_done = false;
    for p in &parsed.pieces {
        let PKind::Seg(m) = p.kind else { continue };
        let pl = p.payload();
        let Some(s) = b.get(pl.clone()) else { continue };
        match classify(m, s) {
            SegKind::Exif => {
                if !exif_done {
                    exif_done = true;
                    r.exif_present = true;
                    exif::inspect(&s[6..], false, r);
                }
            }
            SegKind::Xmp => {
                if !xmp_done {
                    xmp_done = true;
                    xmp::inspect(&s[XMP_NS.len()..], r);
                }
            }
            SegKind::XmpExt => r.xmp = true,
            SegKind::Icc => r.icc = true,
            SegKind::Jumbf => {
                if has_c2pa(s) {
                    r.c2pa = true;
                }
            }
            SegKind::Photoshop => {
                let (iptc, t) = photoshop_info(s);
                r.iptc |= iptc;
                r.thumbnails += t;
            }
            SegKind::Jfxx => r.thumbnails += 1,
            SegKind::Jfif if s.len() > 14 && (s[12] != 0 || s[13] != 0) => {
                r.thumbnails += 1;
            }
            _ => {}
        }
    }
    let mpf = find_mpf(b, &parsed);
    let has_mpf = parsed.pieces.iter().any(|p| match p.kind {
        PKind::Seg(0xE2) => b.get(p.payload()).is_some_and(|s| s.starts_with(b"MPF\0")),
        _ => false,
    });
    for s in secondaries(b, &parsed, &mpf) {
        let tag = if has_mpf { " (MPF)" } else { "" };
        match s.kind {
            SecKind::GainMap => r.secondary_images.push(format!("gain map{tag}")),
            SecKind::Preview => {
                r.thumbnails += 1;
                r.secondary_images.push(format!("preview{tag}"));
            }
            SecKind::Image => r.secondary_images.push(format!("secondary image{tag}")),
            SecKind::Video => {
                if !r.secondary_images.iter().any(|x| x == "motion photo video") {
                    r.secondary_images.push("motion photo video".to_string());
                }
            }
            SecKind::Trailing => {}
        }
    }
    Ok(())
}

pub(crate) fn extract_exif(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    for p in &parsed.pieces {
        if let PKind::Seg(0xE1) = p.kind {
            let s = b.get(p.payload())?;
            if classify(0xE1, s) == SegKind::Exif && exif::parse_header(&s[6..]).is_some() {
                return Some(s[6..].to_vec());
            }
        }
    }
    None
}

pub(crate) fn extract_xmp(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    for p in &parsed.pieces {
        if let PKind::Seg(0xE1) = p.kind {
            let s = b.get(p.payload())?;
            if classify(0xE1, s) == SegKind::Xmp {
                return Some(s[XMP_NS.len()..].to_vec());
            }
        }
    }
    None
}

pub(crate) fn extract_icc(b: &[u8]) -> Option<Vec<u8>> {
    let parsed = parse(b).ok()?;
    let mut chunks: Vec<(u8, &[u8])> = Vec::new();
    for p in &parsed.pieces {
        if let PKind::Seg(0xE2) = p.kind {
            let s = b.get(p.payload())?;
            if s.starts_with(b"ICC_PROFILE\0") && s.len() >= 14 {
                chunks.push((s[12], &s[14..]));
            }
        }
    }
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|c| c.0);
    let mut out = Vec::new();
    for (_, d) in chunks {
        out.extend_from_slice(d);
    }
    Some(out)
}

pub(crate) fn app1_exif_segment(exif: &[u8]) -> Vec<u8> {
    let total = 2 + 6 + exif.len();
    if exif.is_empty() || total > 65535 {
        return Vec::new();
    }
    let mut v = Vec::with_capacity(total + 2);
    v.extend_from_slice(&[0xFF, 0xE1]);
    v.extend_from_slice(&(total as u16).to_be_bytes());
    v.extend_from_slice(b"Exif\0\0");
    v.extend_from_slice(exif);
    v
}

// ---------------------------------------------------------------------------
// Strip

/// Remove GPS from every EXIF block and XMP packet in place, including
/// secondary images. Nothing changes length, so MPF offsets stay valid.
pub(crate) fn gps_in_place(buf: &mut [u8], depth: u8, log: &mut Log) -> Result<(), MetaError> {
    let parsed = parse(buf)?;
    let mpf = find_mpf(buf, &parsed);
    let secs = if depth < 3 {
        secondaries(buf, &parsed, &mpf)
    } else {
        Vec::new()
    };
    let mut ext: Vec<Range<usize>> = Vec::new();
    for p in &parsed.pieces {
        let PKind::Seg(m) = p.kind else { continue };
        let pl = p.payload();
        let kind = match buf.get(pl.clone()) {
            Some(s) => classify(m, s),
            None => continue,
        };
        match kind {
            SegKind::Exif => {
                let r = pl.start + 6..pl.end;
                let (t, _) = exif::filter(&mut buf[r], &Policy::gps_only(), None, depth)?;
                log.tally.add(&t);
            }
            SegKind::Xmp => {
                let r = pl.start + XMP_NS.len()..pl.end;
                log.xmp_gps += xmp::blank_gps(&mut buf[r]);
            }
            SegKind::XmpExt if pl.len() > EXT_HEADER => {
                ext.push(pl.start + EXT_HEADER..pl.end);
            }
            _ => {}
        }
    }
    if !ext.is_empty() {
        let mut g = gather(buf, &ext);
        let n = xmp::blank_gps(&mut g);
        if n > 0 {
            scatter(buf, &ext, &g);
            log.xmp_gps += n;
        }
    }
    for s in secs {
        if matches!(s.kind, SecKind::Video | SecKind::Trailing) {
            continue;
        }
        if gps_in_place(&mut buf[s.range], depth + 1, log).is_err() {
            log.warn("An extra image inside the file could not be read, so its location data was not checked.");
        }
    }
    Ok(())
}

fn push_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) -> bool {
    let len = payload.len() + 2;
    if len > 65535 {
        return false;
    }
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(payload);
    true
}

pub(crate) fn strip(b: &[u8], level: StripLevel) -> Result<(Vec<u8>, Log), MetaError> {
    let mut log = Log::default();
    if level == StripLevel::Gps {
        let mut out = b.to_vec();
        gps_in_place(&mut out, 0, &mut log)?;
        return Ok((out, log));
    }
    let everything = level == StripLevel::Everything;
    let parsed = parse(b)?;
    let mpf = find_mpf(b, &parsed);
    let secs = secondaries(b, &parsed, &mpf);

    let mut orient = 1u8;
    let mut fresh: Vec<u8> = Vec::new();
    if everything {
        for p in &parsed.pieces {
            if let PKind::Seg(0xE1) = p.kind
                && let Some(s) = b.get(p.payload())
                && classify(0xE1, s) == SegKind::Exif
                && let Some(o) = exif::orientation(&s[6..])
            {
                orient = o;
                break;
            }
        }
        if orient > 1 {
            fresh = app1_exif_segment(&exif::orientation_only(orient));
        }
    }
    let mut inserted = fresh.is_empty();

    let mut out = Vec::with_capacity(parsed.end);
    for p in &parsed.pieces {
        let PKind::Seg(m) = p.kind else {
            out.extend_from_slice(&b[p.range.clone()]);
            continue;
        };
        let pl = p.payload();
        let s = &b[pl.clone()];
        let kind = classify(m, s);
        if !inserted && kind != SegKind::Jfif {
            out.extend_from_slice(&fresh);
            inserted = true;
        }
        let whole = &b[p.range.clone()];
        match kind {
            SegKind::Jfif => {
                if s.len() > 14 && (s[12] != 0 || s[13] != 0) {
                    let mut head = s[..14].to_vec();
                    head[12] = 0;
                    head[13] = 0;
                    push_segment(&mut out, m, &head);
                    log.thumbs_extra += 1;
                } else {
                    out.extend_from_slice(whole);
                }
            }
            SegKind::Icc | SegKind::Adobe | SegKind::NotApp => out.extend_from_slice(whole),
            SegKind::Exif => {
                if everything {
                    continue;
                }
                let mut blob = s[6..].to_vec();
                match exif::filter(&mut blob, &Policy::private(), None, 0) {
                    Ok((t, zeroed)) => {
                        let keep = exif::trimmed_len(&blob, &zeroed);
                        blob.truncate(keep);
                        log.tally.add(&t);
                        if !push_segment(&mut out, 0xE1, &[b"Exif\0\0".as_slice(), &blob].concat())
                        {
                            return Err(corrupt("the EXIF block is too large"));
                        }
                    }
                    Err(_) => {
                        log.warn("The EXIF block was damaged, so all of it was removed. The photo may lose its rotation.");
                    }
                }
            }
            SegKind::Xmp | SegKind::XmpExt => log.xmp = true,
            SegKind::Mpf => {}
            SegKind::Jfxx => log.thumbs_extra += 1,
            SegKind::Flashpix => log.extra("FlashPix data"),
            SegKind::Jumbf => {
                if has_c2pa(s) {
                    log.c2pa = true;
                } else {
                    log.extra("embedded JUMBF data");
                }
            }
            SegKind::Photoshop => {
                let (iptc, t) = photoshop_info(s);
                log.iptc |= iptc;
                log.thumbs_extra += t;
                if !iptc {
                    log.extra("Photoshop data");
                }
            }
            SegKind::Com => log.comments = true,
            SegKind::OtherApp => {
                if everything {
                    log.extra("other application data");
                } else {
                    out.extend_from_slice(whole);
                }
            }
        }
    }
    for s in &secs {
        match s.kind {
            SecKind::GainMap => {
                log.extra("gain map");
                log.warn("The HDR gain map was dropped, so the photo now shows as standard dynamic range.");
            }
            SecKind::Preview => log.thumbs_extra += 1,
            SecKind::Image => {
                log.extra("secondary image");
                log.warn("An extra image stored after the main image was dropped.");
            }
            SecKind::Video => {
                log.extra("motion photo video");
                log.warn("The motion photo video stored after the image was dropped.");
            }
            SecKind::Trailing => log.extra("trailing data"),
        }
    }
    Ok((out, log))
}
