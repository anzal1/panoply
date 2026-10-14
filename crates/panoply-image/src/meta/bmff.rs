//! ISO BMFF images: HEIC, HEIF and AVIF.
//!
//! No byte moves, so every `iloc` offset stays valid. Exif and XMP items are
//! neutralized in place, padded to their original length.

use super::exif::{self, Policy};
use super::{Log, MetaError, Report, StripLevel, be16, be32, corrupt, gather, scatter, xmp};
use std::ops::Range;

const IMAGE_BRANDS: &[&[u8; 4]] = &[
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1",
    b"mif2", b"avif", b"avis", b"avio", b"heif", b"miaf",
];
const C2PA_UUID: [u8; 16] = [
    0xD8, 0xFE, 0xC3, 0xD6, 0x1B, 0x0E, 0x48, 0x3C, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7E, 0xC4, 0x81,
];
const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];
const MAX_BOXES: usize = 200_000;

pub(crate) fn is_image_ftyp(b: &[u8]) -> bool {
    if b.len() < 16 || &b[4..8] != b"ftyp" {
        return false;
    }
    let size = (be32(b, 0).unwrap_or(0) as usize).clamp(16, b.len());
    if IMAGE_BRANDS.iter().any(|x| &b[8..12] == *x) {
        return true;
    }
    let mut i = 16;
    while i + 4 <= size {
        if IMAGE_BRANDS.iter().any(|x| &b[i..i + 4] == *x) {
            return true;
        }
        i += 4;
    }
    false
}

#[derive(Clone, Debug)]
struct BoxHdr {
    typ: [u8; 4],
    start: usize,
    hdr: usize,
    end: usize,
}

impl BoxHdr {
    fn data(&self) -> Range<usize> {
        self.start + self.hdr..self.end
    }
}

fn boxes(b: &[u8], range: Range<usize>) -> Result<Vec<BoxHdr>, MetaError> {
    let mut v = Vec::new();
    let mut i = range.start;
    while i + 8 <= range.end {
        let size32 = be32(b, i).unwrap_or(0) as u64;
        let typ = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        let (hdr, size) = match size32 {
            1 => {
                if i + 16 > range.end {
                    return Err(corrupt("a box header is cut short"));
                }
                let hi = be32(b, i + 8).unwrap_or(0) as u64;
                let lo = be32(b, i + 12).unwrap_or(0) as u64;
                (16usize, (hi << 32) | lo)
            }
            0 => (8usize, (range.end - i) as u64),
            n => (8usize, n),
        };
        if size < hdr as u64 {
            return Err(corrupt("a box has an impossible size"));
        }
        let end = i as u64 + size;
        if end > range.end as u64 {
            return Err(corrupt("a box runs past the end of its parent"));
        }
        v.push(BoxHdr {
            typ,
            start: i,
            hdr,
            end: end as usize,
        });
        if v.len() > MAX_BOXES {
            return Err(corrupt("the file has too many boxes"));
        }
        i = end as usize;
    }
    Ok(v)
}

fn uint(b: &[u8], pos: usize, size: usize) -> Option<u64> {
    if size > 8 {
        return None;
    }
    let s = b.get(pos..pos.checked_add(size)?)?;
    Some(s.iter().fold(0u64, |a, &x| (a << 8) | x as u64))
}

fn cstr(b: &[u8], pos: usize, end: usize) -> (String, usize) {
    let end = end.min(b.len());
    if pos >= end {
        return (String::new(), end);
    }
    match b[pos..end].iter().position(|&c| c == 0) {
        Some(p) => (
            String::from_utf8_lossy(&b[pos..pos + p]).to_string(),
            pos + p + 1,
        ),
        None => (String::from_utf8_lossy(&b[pos..end]).to_string(), end),
    }
}

struct Item {
    id: u32,
    typ: [u8; 4],
    content_type: String,
}

struct Loc {
    id: u32,
    method: u8,
    base: u64,
    extents: Vec<(u64, u64)>,
}

struct Reference {
    kind: [u8; 4],
    from: u32,
}

#[derive(Default)]
struct Info {
    items: Vec<Item>,
    locs: Vec<Loc>,
    refs: Vec<Reference>,
    idat: Option<Range<usize>>,
    props: Vec<([u8; 4], Range<usize>)>,
    assoc: Vec<(u32, Vec<u16>)>,
    /// Top-level uuid boxes: (box, usertype).
    uuids: Vec<(BoxHdr, [u8; 16])>,
}

fn parse(b: &[u8]) -> Result<Info, MetaError> {
    let top = boxes(b, 0..b.len())?;
    let mut info = Info::default();
    for bx in &top {
        if &bx.typ == b"uuid" && bx.end >= bx.start + bx.hdr + 16 {
            let d = bx.data();
            let mut u = [0u8; 16];
            u.copy_from_slice(&b[d.start..d.start + 16]);
            info.uuids.push((bx.clone(), u));
        }
    }
    let meta = top
        .iter()
        .find(|x| &x.typ == b"meta")
        .ok_or_else(|| corrupt("the file has no meta box"))?;
    let md = meta.data();
    if md.len() < 4 {
        return Err(corrupt("the meta box is too short"));
    }
    for bx in boxes(b, md.start + 4..md.end)? {
        let d = bx.data();
        match &bx.typ {
            b"iinf" => parse_iinf(b, d, &mut info)?,
            b"iloc" => parse_iloc(b, d, &mut info),
            b"iref" => parse_iref(b, d, &mut info)?,
            b"idat" => info.idat = Some(d),
            b"iprp" => parse_iprp(b, d, &mut info)?,
            _ => {}
        }
    }
    Ok(info)
}

fn parse_iinf(b: &[u8], d: Range<usize>, info: &mut Info) -> Result<(), MetaError> {
    let Some(&ver) = b.get(d.start) else {
        return Ok(());
    };
    let skip = if ver == 0 { 4 + 2 } else { 4 + 4 };
    if d.len() < skip {
        return Ok(());
    }
    for bx in boxes(b, d.start + skip..d.end)? {
        if &bx.typ != b"infe" {
            continue;
        }
        let x = bx.data();
        let Some(&v) = b.get(x.start) else { continue };
        let s = x.start;
        let (id, typ, rest) = match v {
            0 | 1 => (be16(b, s + 4).map(u32::from), [0u8; 4], s + 8),
            2 => {
                let t = b.get(s + 8..s + 12);
                (
                    be16(b, s + 4).map(u32::from),
                    t.map(|t| [t[0], t[1], t[2], t[3]]).unwrap_or_default(),
                    s + 12,
                )
            }
            _ => {
                let t = b.get(s + 10..s + 14);
                (
                    be32(b, s + 4),
                    t.map(|t| [t[0], t[1], t[2], t[3]]).unwrap_or_default(),
                    s + 14,
                )
            }
        };
        let Some(id) = id else { continue };
        let mut content_type = String::new();
        if &typ == b"mime" || v < 2 {
            let (_, next) = cstr(b, rest, x.end);
            content_type = cstr(b, next, x.end).0;
        }
        info.items.push(Item {
            id,
            typ,
            content_type,
        });
    }
    Ok(())
}

fn parse_iloc(b: &[u8], d: Range<usize>, info: &mut Info) {
    let Some(&ver) = b.get(d.start) else { return };
    let (Some(&a), Some(&c)) = (b.get(d.start + 4), b.get(d.start + 5)) else {
        return;
    };
    let off_size = (a >> 4) as usize;
    let len_size = (a & 15) as usize;
    let base_size = (c >> 4) as usize;
    let idx_size = if ver >= 1 { (c & 15) as usize } else { 0 };
    let mut p = d.start + 6;
    let count = if ver < 2 {
        let v = be16(b, p).map(u32::from);
        p += 2;
        v
    } else {
        let v = be32(b, p);
        p += 4;
        v
    };
    let Some(count) = count else { return };
    for _ in 0..count {
        if p >= d.end {
            return;
        }
        let id = if ver < 2 {
            let v = be16(b, p).map(u32::from);
            p += 2;
            v
        } else {
            let v = be32(b, p);
            p += 4;
            v
        };
        let Some(id) = id else { return };
        let mut method = 0u8;
        if ver >= 1 {
            method = (be16(b, p).unwrap_or(0) & 15) as u8;
            p += 2;
        }
        p += 2;
        let Some(base) = uint(b, p, base_size) else {
            return;
        };
        p += base_size;
        let Some(n) = be16(b, p) else { return };
        p += 2;
        let mut extents = Vec::new();
        for _ in 0..n {
            p += idx_size;
            let (Some(o), Some(l)) = (uint(b, p, off_size), uint(b, p + off_size, len_size)) else {
                return;
            };
            p += off_size + len_size;
            if p > d.end {
                return;
            }
            extents.push((o, l));
        }
        info.locs.push(Loc {
            id,
            method,
            base,
            extents,
        });
    }
}

fn parse_iref(b: &[u8], d: Range<usize>, info: &mut Info) -> Result<(), MetaError> {
    let Some(&ver) = b.get(d.start) else {
        return Ok(());
    };
    if d.len() < 4 {
        return Ok(());
    }
    for bx in boxes(b, d.start + 4..d.end)? {
        let x = bx.data();
        let from = if ver == 0 {
            be16(b, x.start).map(u32::from)
        } else {
            be32(b, x.start)
        };
        if let Some(from) = from {
            info.refs.push(Reference { kind: bx.typ, from });
        }
    }
    Ok(())
}

fn parse_iprp(b: &[u8], d: Range<usize>, info: &mut Info) -> Result<(), MetaError> {
    for bx in boxes(b, d)? {
        let x = bx.data();
        match &bx.typ {
            b"ipco" => {
                for p in boxes(b, x)? {
                    info.props.push((p.typ, p.data()));
                }
            }
            b"ipma" => {
                let (Some(&ver), Some(&f2)) = (b.get(x.start), b.get(x.start + 3)) else {
                    continue;
                };
                let wide = f2 & 1 == 1;
                let Some(n) = be32(b, x.start + 4) else {
                    continue;
                };
                let mut p = x.start + 8;
                for _ in 0..n {
                    if p >= x.end {
                        break;
                    }
                    let id = if ver < 1 {
                        let v = be16(b, p).map(u32::from);
                        p += 2;
                        v
                    } else {
                        let v = be32(b, p);
                        p += 4;
                        v
                    };
                    let Some(id) = id else { break };
                    let Some(&cnt) = b.get(p) else { break };
                    p += 1;
                    let mut list = Vec::new();
                    for _ in 0..cnt {
                        if wide {
                            let Some(v) = be16(b, p) else { break };
                            list.push(v & 0x7FFF);
                            p += 2;
                        } else {
                            let Some(&v) = b.get(p) else { break };
                            list.push((v & 0x7F) as u16);
                            p += 1;
                        }
                    }
                    info.assoc.push((id, list));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl Info {
    fn ranges(&self, b: &[u8], id: u32) -> Option<Vec<Range<usize>>> {
        let loc = self.locs.iter().find(|l| l.id == id)?;
        let origin = match loc.method {
            0 => 0u64,
            1 => self.idat.as_ref()?.start as u64,
            _ => return None,
        };
        let mut out = Vec::new();
        for &(o, l) in &loc.extents {
            if l == 0 {
                return None;
            }
            let s = origin.checked_add(loc.base)?.checked_add(o)?;
            let e = s.checked_add(l)?;
            if e > b.len() as u64 {
                return None;
            }
            out.push(s as usize..e as usize);
        }
        if out.is_empty() { None } else { Some(out) }
    }

    fn exif_items(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|i| &i.typ == b"Exif")
            .map(|i| i.id)
            .collect()
    }

    fn xmp_items(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|i| {
                &i.typ == b"mime" && i.content_type.eq_ignore_ascii_case("application/rdf+xml")
            })
            .map(|i| i.id)
            .collect()
    }

    fn thumbnails(&self) -> usize {
        self.refs.iter().filter(|r| &r.kind == b"thmb").count()
    }

    fn props_of(&self, id: u32) -> Vec<usize> {
        self.assoc
            .iter()
            .filter(|(i, _)| *i == id)
            .flat_map(|(_, l)| l.iter().map(|&x| x as usize))
            .collect()
    }

    fn aux_name(&self, b: &[u8], id: u32) -> Option<String> {
        for idx in self.props_of(id) {
            let (typ, d) = self.props.get(idx.checked_sub(1)?)?;
            if typ == b"auxC" {
                let (t, _) = cstr(b, d.start + 4, d.end);
                let l = t.to_lowercase();
                let name = if l.contains("gainmap") {
                    "gain map"
                } else if l.contains("matte") || l.contains("mask") || l.contains("semantic") {
                    "portrait matte"
                } else if l.contains("depth") || l.ends_with("auxid:2") {
                    "depth map"
                } else if l.contains("disparity") {
                    "disparity map"
                } else if l.ends_with("auxid:1") || l.contains("alpha") {
                    return None;
                } else {
                    "auxiliary image"
                };
                return Some(name.to_string());
            }
        }
        None
    }

    fn secondary_names(&self, b: &[u8]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut add = |n: String| {
            if !names.contains(&n) {
                names.push(n);
            }
        };
        for r in self.refs.iter().filter(|r| &r.kind == b"auxl") {
            if let Some(n) = self.aux_name(b, r.from) {
                add(n);
            }
        }
        if self.items.iter().any(|i| &i.typ == b"tmap") {
            add("gain map".to_string());
        }
        names
    }
}

/// Find the TIFF header inside an Exif item payload.
fn tiff_start(p: &[u8]) -> Option<usize> {
    let off = be32(p, 0)? as usize;
    let mut cands = vec![4usize.saturating_add(off), 10, 4];
    cands.dedup();
    for mut c in cands {
        if p.get(c..).is_some_and(|s| s.starts_with(b"Exif\0\0")) {
            c += 6;
        }
        if p.get(c..).is_some_and(|s| exif::parse_header(s).is_some()) {
            return Some(c);
        }
    }
    None
}

fn exif_payload(b: &[u8], info: &Info) -> Option<Vec<u8>> {
    let id = *info.exif_items().first()?;
    let r = info.ranges(b, id)?;
    Some(gather(b, &r))
}

pub(crate) fn extract_exif(b: &[u8]) -> Option<Vec<u8>> {
    let info = parse(b).ok()?;
    let p = exif_payload(b, &info)?;
    let s = tiff_start(&p)?;
    Some(p[s..].to_vec())
}

pub(crate) fn extract_xmp(b: &[u8]) -> Option<Vec<u8>> {
    let info = parse(b).ok()?;
    let id = *info.xmp_items().first()?;
    Some(gather(b, &info.ranges(b, id)?))
}

pub(crate) fn extract_icc(b: &[u8]) -> Option<Vec<u8>> {
    let info = parse(b).ok()?;
    for (typ, d) in &info.props {
        if typ == b"colr" {
            let t = b.get(d.start..d.start + 4)?;
            if t == b"prof" || t == b"rICC" {
                return Some(b.get(d.start + 4..d.end)?.to_vec());
            }
        }
    }
    None
}

pub(crate) fn inspect(b: &[u8], r: &mut Report) -> Result<(), MetaError> {
    let info = parse(b)?;
    if let Some(p) = exif_payload(b, &info) {
        r.exif_present = true;
        if let Some(s) = tiff_start(&p) {
            exif::inspect(&p[s..], false, r);
        }
    }
    if let Some(id) = info.xmp_items().first() {
        if let Some(rg) = info.ranges(b, *id) {
            let packet = gather(b, &rg);
            if !xmp::is_empty_packet(&packet) {
                xmp::inspect(&packet, r);
            }
        } else {
            r.xmp = true;
        }
    }
    for (u, uu) in &info.uuids {
        if *uu == C2PA_UUID {
            r.c2pa = true;
        }
        if *uu == XMP_UUID {
            r.xmp = true;
        }
        let _ = u;
    }
    if info.props.iter().any(|(t, d)| {
        t == b"colr"
            && b.get(d.start..d.start + 4)
                .is_some_and(|x| x == b"prof" || x == b"rICC")
    }) {
        r.icc = true;
    }
    r.thumbnails += info.thumbnails();
    for n in info.secondary_names(b) {
        if !r.secondary_images.contains(&n) {
            r.secondary_images.push(n);
        }
    }
    Ok(())
}

fn neutralize_exif(payload: &mut [u8]) {
    payload.fill(0);
    let t = exif::empty_tiff();
    if payload.len() >= 4 + t.len() {
        payload[4..4 + t.len()].copy_from_slice(&t);
    }
}

pub(crate) fn strip(b: &[u8], level: StripLevel) -> Result<(Vec<u8>, Log), MetaError> {
    let info = parse(b)?;
    let mut out = b.to_vec();
    let mut log = Log::default();
    let mut edited = false;

    for id in info.exif_items() {
        let ranges = info
            .ranges(b, id)
            .ok_or_else(|| corrupt("the Exif item points outside the file"))?;
        let mut payload = gather(&out, &ranges);
        match level {
            StripLevel::Everything => {
                neutralize_exif(&mut payload);
                edited = true;
            }
            _ => {
                let Some(ts) = tiff_start(&payload) else {
                    if level == StripLevel::Gps {
                        return Err(corrupt("the Exif item has no valid TIFF header"));
                    }
                    neutralize_exif(&mut payload);
                    log.warn("The EXIF block was damaged, so all of it was removed.");
                    scatter(&mut out, &ranges, &payload);
                    edited = true;
                    continue;
                };
                let pol = if level == StripLevel::Gps {
                    Policy::gps_only()
                } else {
                    Policy::private()
                };
                match exif::filter(&mut payload[ts..], &pol, None, 0) {
                    Ok((t, zeroed)) => {
                        log.tally.add(&t);
                        if level != StripLevel::Gps {
                            let keep = exif::trimmed_len(&payload[ts..], &zeroed);
                            payload[ts + keep..].fill(0);
                        }
                    }
                    Err(e) => {
                        if level == StripLevel::Gps {
                            return Err(e);
                        }
                        neutralize_exif(&mut payload);
                        log.warn("The EXIF block was damaged, so all of it was removed.");
                    }
                }
                edited = true;
            }
        }
        scatter(&mut out, &ranges, &payload);
    }

    for id in info.xmp_items() {
        let ranges = info
            .ranges(b, id)
            .ok_or_else(|| corrupt("the XMP item points outside the file"))?;
        let mut payload = gather(&out, &ranges);
        if level == StripLevel::Gps {
            log.xmp_gps += xmp::blank_gps(&mut payload);
        } else {
            payload = xmp::empty_packet(payload.len());
            log.xmp = true;
        }
        scatter(&mut out, &ranges, &payload);
        edited = true;
    }

    if level != StripLevel::Gps {
        for (bx, uu) in &info.uuids {
            if *uu == C2PA_UUID || *uu == XMP_UUID {
                let d = bx.data();
                out[bx.start + 4..bx.start + 8].copy_from_slice(b"free");
                out[d.start..d.end].fill(0);
                if *uu == C2PA_UUID {
                    log.c2pa = true;
                } else {
                    log.xmp = true;
                }
                edited = true;
            }
        }
    } else {
        for (bx, uu) in &info.uuids {
            if *uu == XMP_UUID {
                let d = bx.data();
                log.xmp_gps += xmp::blank_gps(&mut out[d.start + 16..d.end]);
            }
        }
    }

    if edited {
        log.warn("HEIC/AVIF: the Exif and XMP items were edited in place, so the file size did not change.");
    }
    if level != StripLevel::Gps {
        let t = info.thumbnails();
        if t > 0 {
            log.warn("HEIC/AVIF: thumbnail images stay in the file because no byte can move. They show the photo, not hidden data.");
        }
        let aux = info.secondary_names(b);
        if !aux.is_empty() {
            log.warn("HEIC/AVIF: depth, matte and gain map images stay in the file. They carry no personal metadata.");
        }
    }
    if info.props.iter().any(|(t, _)| t == b"irot" || t == b"imir") {
        log.kept_extra.push("rotation (irot, imir)".to_string());
    }
    Ok((out, log))
}

/// Byte ranges a strip may change: Exif and XMP item payloads and the boxes
/// that carry C2PA or XMP in a uuid box.
#[cfg(test)]
pub(crate) fn metadata_ranges(b: &[u8]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let Ok(info) = parse(b) else { return out };
    for id in info.exif_items().into_iter().chain(info.xmp_items()) {
        if let Some(r) = info.ranges(b, id) {
            out.extend(r);
        }
    }
    for (bx, uu) in &info.uuids {
        if *uu == C2PA_UUID || *uu == XMP_UUID {
            out.push(bx.start + 4..bx.end);
        }
    }
    out
}
