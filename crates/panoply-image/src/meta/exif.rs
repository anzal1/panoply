//! EXIF (TIFF block) reading and in-place editing.
//!
//! The block is never rebuilt. Entries are deleted from their directory and
//! their bytes are overwritten with zeros, so every kept offset stays valid.

use super::{Gps, MetaError, Report, corrupt};
use std::ops::Range;

pub(crate) const TAG_ORIENTATION: u16 = 0x0112;
pub(crate) const TAG_EXIF_IFD: u16 = 0x8769;
pub(crate) const TAG_GPS_IFD: u16 = 0x8825;
pub(crate) const TAG_INTEROP_IFD: u16 = 0xA005;
pub(crate) const TAG_SUB_IFDS: u16 = 0x014A;
pub(crate) const TAG_MAKER_NOTE: u16 = 0x927C;
const TAG_THUMB_OFFSET: u16 = 0x0201;
const TAG_THUMB_LEN: u16 = 0x0202;
const TAG_STRIP_OFFSETS: u16 = 0x0111;
const TAG_STRIP_COUNTS: u16 = 0x0117;

/// Entries that identify a body, lens or image.
pub(crate) const SERIAL_TAGS: &[(u16, &str)] = &[
    (0xA431, "BodySerialNumber"),
    (0xA435, "LensSerialNumber"),
    (0xC62F, "CameraSerialNumber"),
    (0xA420, "ImageUniqueID"),
];
/// Entries that name a person.
pub(crate) const OWNER_TAGS: &[(u16, &str)] = &[
    (0x013B, "Artist"),
    (0x8298, "Copyright"),
    (0xA430, "CameraOwnerName"),
    (0xA437, "Photographer"),
    (0xA438, "ImageEditor"),
    (0x9C9D, "XPAuthor"),
];
/// Free text entries that can hold names, places or notes.
pub(crate) const COMMENT_TAGS: &[u16] = &[0x010E, 0x9286, 0x9C9B, 0x9C9C, 0x9C9E, 0x9C9F];

const MAX_IFDS: usize = 4096;
const MAX_DEPTH: u8 = 6;

/// Byte order of a TIFF block.
#[derive(Clone, Copy)]
pub(crate) struct Order(pub bool);

impl Order {
    pub(crate) fn u16(self, b: &[u8], o: usize) -> Option<u16> {
        if self.0 {
            super::le16(b, o)
        } else {
            super::be16(b, o)
        }
    }
    pub(crate) fn u32(self, b: &[u8], o: usize) -> Option<u32> {
        if self.0 {
            super::le32(b, o)
        } else {
            super::be32(b, o)
        }
    }
    fn put16(self, b: &mut [u8], o: usize, v: u16) -> bool {
        let bytes = if self.0 {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        match b.get_mut(o..o.saturating_add(2)) {
            Some(s) if s.len() == 2 => {
                s.copy_from_slice(&bytes);
                true
            }
            _ => false,
        }
    }
    fn put32(self, b: &mut [u8], o: usize, v: u32) -> bool {
        let bytes = if self.0 {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        match b.get_mut(o..o.saturating_add(4)) {
            Some(s) if s.len() == 4 => {
                s.copy_from_slice(&bytes);
                true
            }
            _ => false,
        }
    }
}

/// Check the TIFF header. Returns the byte order and the IFD0 offset.
pub(crate) fn parse_header(b: &[u8]) -> Option<(Order, usize)> {
    if b.len() < 8 {
        return None;
    }
    let le = match &b[0..4] {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let o = Order(le);
    let off = o.u32(b, 4)? as usize;
    Some((o, off))
}

/// One directory entry.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    /// Position of the 12 byte entry.
    pub pos: usize,
    /// Where the value bytes are, when they are in bounds.
    pub data: Option<Range<usize>>,
}

/// One directory.
#[derive(Debug)]
pub(crate) struct Ifd {
    pub off: usize,
    pub entries: Vec<Entry>,
    /// Next directory offset, 0 when none.
    pub next: usize,
    /// True when the 4 byte next pointer fits in the buffer.
    pub has_next: bool,
    /// One past the last byte of the directory (entries plus next pointer).
    pub end: usize,
}

fn type_size(t: u16) -> usize {
    match t {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16 | 17 | 18 => 8,
        _ => 0,
    }
}

pub(crate) fn read_ifd(b: &[u8], o: Order, off: usize) -> Result<Ifd, MetaError> {
    if off < 8 {
        return Err(corrupt("EXIF directory offset is invalid"));
    }
    let n = o
        .u16(b, off)
        .ok_or_else(|| corrupt("EXIF directory is past the end of the block"))?
        as usize;
    let array_end = off + 2 + n * 12;
    if array_end > b.len() {
        return Err(corrupt(
            "EXIF directory claims more entries than the block holds",
        ));
    }
    let has_next = array_end + 4 <= b.len();
    let next = if has_next {
        o.u32(b, array_end).unwrap_or(0) as usize
    } else {
        0
    };
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        let pos = off + 2 + i * 12;
        let tag = o.u16(b, pos).unwrap_or(0);
        let typ = o.u16(b, pos + 2).unwrap_or(0);
        let count = o.u32(b, pos + 4).unwrap_or(0);
        let size = (type_size(typ) as u64) * (count as u64);
        let data = if type_size(typ) == 0 {
            None
        } else if size <= 4 {
            Some(pos + 8..pos + 8 + size as usize)
        } else {
            let voff = o.u32(b, pos + 8).unwrap_or(0) as u64;
            let vend = voff + size;
            if vend <= b.len() as u64 && voff >= 8 {
                Some(voff as usize..vend as usize)
            } else {
                None
            }
        };
        entries.push(Entry {
            tag,
            typ,
            count,
            pos,
            data,
        });
    }
    let end = if has_next { array_end + 4 } else { array_end };
    Ok(Ifd {
        off,
        entries,
        next,
        has_next,
        end,
    })
}

fn clean_text(bytes: &[u8]) -> String {
    let cut = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    let s = String::from_utf8_lossy(&bytes[..cut]);
    let t: String = s.trim().chars().take(512).collect();
    t
}

pub(crate) fn ascii(b: &[u8], e: &Entry) -> Option<String> {
    if !matches!(e.typ, 1 | 2 | 7) {
        return None;
    }
    let r = e.data.clone()?;
    let s = clean_text(b.get(r)?);
    if s.is_empty() { None } else { Some(s) }
}

fn utf16le_text(b: &[u8], e: &Entry) -> Option<String> {
    let r = e.data.clone()?;
    let raw = b.get(r)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .take(512)
        .collect();
    let s = String::from_utf16_lossy(&units);
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn uint_value(b: &[u8], o: Order, e: &Entry) -> Option<u32> {
    let r = e.data.clone()?;
    if e.count < 1 {
        return None;
    }
    match e.typ {
        3 => o.u16(b, r.start).map(u32::from),
        4 | 13 => o.u32(b, r.start),
        1 | 7 => b.get(r.start).map(|&v| v as u32),
        _ => None,
    }
}

fn rational(b: &[u8], o: Order, pos: usize) -> Option<f64> {
    let n = o.u32(b, pos)? as f64;
    let d = o.u32(b, pos + 4)? as f64;
    if d == 0.0 { None } else { Some(n / d) }
}

fn parse_gps(b: &[u8], o: Order, ifd: &Ifd) -> Option<Gps> {
    let find = |t: u16| ifd.entries.iter().find(|e| e.tag == t);
    let status_void = find(9)
        .and_then(|e| ascii(b, e))
        .map(|s| s.eq_ignore_ascii_case("V"))
        .unwrap_or(false);
    if status_void {
        return None;
    }
    let coord = |tag: u16| -> Option<f64> {
        let e = find(tag)?;
        if e.typ != 5 || e.count == 0 || e.count > 3 {
            return None;
        }
        let r = e.data.clone()?;
        let d = rational(b, o, r.start)?;
        let m = if e.count >= 2 {
            rational(b, o, r.start + 8)?
        } else {
            0.0
        };
        let s = if e.count >= 3 {
            rational(b, o, r.start + 16)?
        } else {
            0.0
        };
        let v = d + m / 60.0 + s / 3600.0;
        if v.is_finite() { Some(v) } else { None }
    };
    let mut lat = coord(2)?;
    let mut lon = coord(4)?;
    if let Some(r) = find(1).and_then(|e| ascii(b, e))
        && r.eq_ignore_ascii_case("S")
    {
        lat = -lat;
    }
    if let Some(r) = find(3).and_then(|e| ascii(b, e))
        && r.eq_ignore_ascii_case("W")
    {
        lon = -lon;
    }
    if lat.abs() > 90.0 || lon.abs() > 180.0 || (lat == 0.0 && lon == 0.0) {
        return None;
    }
    let mut alt = find(6).and_then(|e| {
        if e.typ != 5 || e.count < 1 {
            return None;
        }
        rational(b, o, e.data.clone()?.start)
    });
    if let (Some(a), Some(e)) = (alt, find(5))
        && uint_value(b, o, e) == Some(1)
    {
        alt = Some(-a);
    }
    Some(Gps { lat, lon, alt })
}

/// Apple stores the Live Photo content identifier in its MakerNote.
fn apple_content_id(mn: &[u8]) -> Option<String> {
    if !mn.starts_with(b"Apple iOS\0") || mn.len() < 16 {
        return None;
    }
    let o = Order(match &mn[12..14] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    });
    let ifd = read_ifd_at(mn, o, 14).ok()?;
    for e in &ifd.entries {
        if e.tag == 0x0011 && e.typ == 2 {
            let size = e.count as usize;
            let range = if size <= 4 {
                e.pos + 8..e.pos + 8 + size
            } else {
                let off = o.u32(mn, e.pos + 8)? as usize;
                off..off.checked_add(size)?
            };
            let s = clean_text(mn.get(range)?);
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// Like [`read_ifd`] but allows offsets below 8 (maker notes use their own base).
fn read_ifd_at(b: &[u8], o: Order, off: usize) -> Result<Ifd, MetaError> {
    let n = o
        .u16(b, off)
        .ok_or_else(|| corrupt("directory is past the end"))? as usize;
    let array_end = off + 2 + n * 12;
    if array_end > b.len() {
        return Err(corrupt("directory is too long"));
    }
    let mut entries = Vec::new();
    for i in 0..n {
        let pos = off + 2 + i * 12;
        entries.push(Entry {
            tag: o.u16(b, pos).unwrap_or(0),
            typ: o.u16(b, pos + 2).unwrap_or(0),
            count: o.u32(b, pos + 4).unwrap_or(0),
            pos,
            data: None,
        });
    }
    Ok(Ifd {
        off,
        entries,
        next: 0,
        has_next: false,
        end: array_end,
    })
}

fn push_unique(v: &mut Vec<(String, String)>, name: &str, value: String) {
    if !v.iter().any(|(n, x)| n == name && *x == value) {
        v.push((name.to_string(), value));
    }
}

/// Fill a report from an EXIF block (or a whole TIFF file when `tiff_file`).
/// It is best effort: a damaged block leaves the fields it could not read empty.
pub(crate) fn inspect(blob: &[u8], tiff_file: bool, r: &mut Report) {
    let Some((o, off0)) = parse_header(blob) else {
        return;
    };
    r.exif_present = true;
    let Ok(ifd0) = read_ifd(blob, o, off0) else {
        return;
    };
    let mut make = None;
    let mut model = None;
    let mut exif_ptr = None;
    let mut gps_ptr = None;
    for e in &ifd0.entries {
        match e.tag {
            TAG_ORIENTATION => {
                if let Some(v) = uint_value(blob, o, e)
                    && (1..=8).contains(&v)
                {
                    r.orientation = v as u8;
                }
            }
            0x010F => make = ascii(blob, e),
            0x0110 => model = ascii(blob, e),
            0x0131 => r.software = ascii(blob, e),
            0x0132 => {
                if r.date_taken.is_none() {
                    r.date_taken = ascii(blob, e);
                }
            }
            TAG_EXIF_IFD => exif_ptr = uint_value(blob, o, e),
            TAG_GPS_IFD => gps_ptr = uint_value(blob, o, e),
            _ => {
                for (t, name) in OWNER_TAGS {
                    if e.tag == *t {
                        let v = if *t == 0x9C9D {
                            utf16le_text(blob, e)
                        } else {
                            ascii(blob, e)
                        };
                        if let Some(v) = v {
                            push_unique(&mut r.owner, name, v);
                        }
                    }
                }
                for (t, name) in SERIAL_TAGS {
                    if e.tag == *t
                        && let Some(v) = ascii(blob, e)
                    {
                        push_unique(&mut r.serials, name, v);
                    }
                }
            }
        }
    }
    r.camera = match (make, model) {
        (Some(mk), Some(md)) => {
            if md.to_lowercase().starts_with(&mk.to_lowercase()) {
                Some(md)
            } else {
                Some(format!("{mk} {md}"))
            }
        }
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    };
    if let Some(p) = exif_ptr
        && let Ok(ex) = read_ifd(blob, o, p as usize)
    {
        let mut dto = None;
        let mut dtd = None;
        for e in &ex.entries {
            match e.tag {
                0x9003 => dto = ascii(blob, e),
                0x9004 => dtd = ascii(blob, e),
                0xA434 => r.lens = ascii(blob, e),
                TAG_MAKER_NOTE => {
                    r.maker_note_bytes = e.count as usize;
                    if let Some(rng) = e.data.clone()
                        && let Some(mn) = blob.get(rng)
                        && let Some(id) = apple_content_id(mn)
                    {
                        r.live_photo_id = Some(id);
                    }
                }
                t => {
                    for (st, name) in SERIAL_TAGS {
                        if t == *st
                            && let Some(v) = ascii(blob, e)
                        {
                            push_unique(&mut r.serials, name, v);
                        }
                    }
                    for (ot, name) in OWNER_TAGS {
                        if t == *ot
                            && let Some(v) = ascii(blob, e)
                        {
                            push_unique(&mut r.owner, name, v);
                        }
                    }
                }
            }
        }
        if let Some(d) = dto.or(dtd) {
            r.date_taken = Some(d);
        }
    }
    if let Some(p) = gps_ptr
        && let Ok(g) = read_ifd(blob, o, p as usize)
    {
        r.gps_tags += g.entries.len();
        r.gps = parse_gps(blob, o, &g);
    }
    if !tiff_file
        && ifd0.next != 0
        && ifd0.next != off0
        && let Ok(t) = read_ifd(blob, o, ifd0.next)
    {
        let has_jpeg = t.entries.iter().any(|e| e.tag == TAG_THUMB_OFFSET)
            && t.entries.iter().any(|e| e.tag == TAG_THUMB_LEN);
        let has_strips = t.entries.iter().any(|e| e.tag == TAG_STRIP_OFFSETS);
        if has_jpeg || has_strips {
            r.thumbnails += 1;
        }
    }
}

/// The orientation tag of an EXIF block, 1 to 8.
pub(crate) fn orientation(blob: &[u8]) -> Option<u8> {
    let (o, off0) = parse_header(blob)?;
    let ifd = read_ifd(blob, o, off0).ok()?;
    let e = ifd.entries.iter().find(|e| e.tag == TAG_ORIENTATION)?;
    let v = uint_value(blob, o, e)?;
    if (1..=8).contains(&v) {
        Some(v as u8)
    } else {
        None
    }
}

/// The JPEG thumbnail an EXIF block carries in IFD1, when it has one.
pub(crate) fn thumbnail(blob: &[u8]) -> Option<&[u8]> {
    let (o, off0) = parse_header(blob)?;
    let ifd0 = read_ifd(blob, o, off0).ok()?;
    if !ifd0.has_next || ifd0.next == 0 {
        return None;
    }
    let ifd1 = read_ifd(blob, o, ifd0.next).ok()?;
    let value = |tag: u16| {
        ifd1.entries
            .iter()
            .find(|e| e.tag == tag)
            .and_then(|e| uint_value(blob, o, e))
    };
    let (off, len) = (
        value(TAG_THUMB_OFFSET)? as usize,
        value(TAG_THUMB_LEN)? as usize,
    );
    let end = off.checked_add(len)?;
    let t = blob.get(off..end)?;
    t.starts_with(&[0xFF, 0xD8]).then_some(t)
}

/// `blob` with its IFD0 Orientation set to `orientation` (1 to 8), when the block has that tag.
pub(crate) fn with_orientation(blob: &[u8], orientation: u8) -> Option<Vec<u8>> {
    if !(1..=8).contains(&orientation) {
        return None;
    }
    let (o, off0) = parse_header(blob)?;
    let ifd = read_ifd(blob, o, off0).ok()?;
    let e = ifd.entries.iter().find(|e| e.tag == TAG_ORIENTATION)?;
    // A SHORT, count 1: the value sits in the first two bytes of the entry's value field.
    if e.typ != 3 || e.count != 1 {
        return None;
    }
    let at = e.pos + 8;
    let mut out = blob.to_vec();
    let v = u16::from(orientation);
    let bytes = if o.0 {
        v.to_le_bytes()
    } else {
        v.to_be_bytes()
    };
    out.get_mut(at..at + 2)?.copy_from_slice(&bytes);
    Some(out)
}

/// A minimal TIFF block holding only IFD0 Orientation.
pub(crate) fn orientation_only(orientation: u8) -> Vec<u8> {
    let v = if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    };
    let mut b = Vec::with_capacity(26);
    b.extend_from_slice(b"MM\0*");
    b.extend_from_slice(&8u32.to_be_bytes());
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&TAG_ORIENTATION.to_be_bytes());
    b.extend_from_slice(&3u16.to_be_bytes());
    b.extend_from_slice(&1u32.to_be_bytes());
    b.extend_from_slice(&[0, v, 0, 0]);
    b.extend_from_slice(&0u32.to_be_bytes());
    b
}

/// A TIFF block with an empty IFD0. Used to neutralize HEIC Exif items.
pub(crate) fn empty_tiff() -> Vec<u8> {
    let mut b = Vec::with_capacity(14);
    b.extend_from_slice(b"MM\0*");
    b.extend_from_slice(&8u32.to_be_bytes());
    b.extend_from_slice(&0u16.to_be_bytes());
    b.extend_from_slice(&0u32.to_be_bytes());
    b
}

// ---------------------------------------------------------------------------
// Editing

/// What to remove.
#[derive(Clone, Copy, Default)]
pub(crate) struct Policy {
    pub gps: bool,
    pub serials: bool,
    pub owner: bool,
    pub comments: bool,
    pub thumbnails: bool,
    pub maker_note: bool,
}

impl Policy {
    pub(crate) fn gps_only() -> Policy {
        Policy {
            gps: true,
            ..Policy::default()
        }
    }
    pub(crate) fn private() -> Policy {
        Policy {
            gps: true,
            serials: true,
            owner: true,
            comments: true,
            thumbnails: true,
            maker_note: true,
        }
    }
}

/// How many things of each kind were removed.
#[derive(Default, Clone, Debug)]
pub(crate) struct Tally {
    pub gps_tags: usize,
    pub serials: usize,
    pub owner: usize,
    pub comments: usize,
    pub thumbnails: usize,
    pub maker_note_bytes: usize,
}

impl Tally {
    pub(crate) fn add(&mut self, o: &Tally) {
        self.gps_tags += o.gps_tags;
        self.serials += o.serials;
        self.owner += o.owner;
        self.comments += o.comments;
        self.thumbnails += o.thumbnails;
        self.maker_note_bytes += o.maker_note_bytes;
    }
}

/// Changes for a re-encoded image.
#[derive(Clone, Copy)]
pub(crate) struct Fix {
    pub dims: (u32, u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Main,
    Exif,
}

/// Edits an EXIF block in place.
pub(crate) struct Editor<'a> {
    pub b: &'a mut [u8],
    pub o: Order,
    pub zeroed: Vec<Range<usize>>,
    visited: Vec<usize>,
    zero_seen: Vec<usize>,
    pub tally: Tally,
    fix: Option<Fix>,
    jdepth: u8,
}

impl<'a> Editor<'a> {
    /// Check the header. Returns the editor and the IFD0 offset.
    pub(crate) fn new(b: &'a mut [u8], jdepth: u8) -> Result<(Editor<'a>, usize), MetaError> {
        let (o, off0) =
            parse_header(b).ok_or_else(|| corrupt("EXIF block has no valid TIFF header"))?;
        Ok((
            Editor {
                b,
                o,
                zeroed: Vec::new(),
                visited: Vec::new(),
                zero_seen: Vec::new(),
                tally: Tally::default(),
                fix: None,
                jdepth,
            },
            off0,
        ))
    }

    pub(crate) fn read(&self, off: usize) -> Result<Ifd, MetaError> {
        read_ifd(self.b, self.o, off)
    }

    pub(crate) fn visit(&mut self, off: usize) -> Result<(), MetaError> {
        if self.visited.contains(&off) {
            return Err(corrupt("EXIF directories loop back on themselves"));
        }
        if self.visited.len() >= MAX_IFDS {
            return Err(corrupt("EXIF has too many directories"));
        }
        self.visited.push(off);
        Ok(())
    }

    pub(crate) fn zero(&mut self, r: Range<usize>) {
        let end = r.end.min(self.b.len());
        if r.start >= end {
            return;
        }
        self.b[r.start..end].fill(0);
        self.zeroed.push(r.start..end);
    }

    pub(crate) fn ptr_value(&self, e: &Entry) -> Option<usize> {
        if !matches!(e.typ, 4 | 13) || e.count != 1 {
            return None;
        }
        let v = uint_value(self.b, self.o, e)? as usize;
        if v >= 8 { Some(v) } else { None }
    }

    /// Zero a directory, its values and any sub-directories it owns.
    pub(crate) fn zero_ifd(&mut self, off: usize, depth: u8) {
        if depth > MAX_DEPTH
            || self.visited.contains(&off)
            || self.zero_seen.contains(&off)
            || self.zero_seen.len() > 10_000
        {
            return;
        }
        self.zero_seen.push(off);
        let Ok(ifd) = self.read(off) else { return };
        for e in &ifd.entries {
            if matches!(
                e.tag,
                TAG_EXIF_IFD | TAG_GPS_IFD | TAG_INTEROP_IFD | TAG_SUB_IFDS
            ) && matches!(e.typ, 4 | 13)
                && let Some(r) = e.data.clone()
            {
                for k in 0..(e.count as usize).min(16) {
                    if let Some(p) = self.o.u32(self.b, r.start + k * 4)
                        && p >= 8
                    {
                        self.zero_ifd(p as usize, depth + 1);
                    }
                }
            }
            if let Some(r) = e.data.clone() {
                self.zero(r);
            }
        }
        self.zero(ifd.off..ifd.end);
    }

    /// Delete entries from a directory. Returns the position of the next-IFD
    /// pointer after the change.
    pub(crate) fn remove_entries(&mut self, ifd: &Ifd, kill: &[bool]) -> usize {
        let n = ifd.entries.len();
        let old_array_end = ifd.off + 2 + n * 12;
        if !kill.iter().any(|&k| k) {
            return old_array_end;
        }
        for (e, &k) in ifd.entries.iter().zip(kill) {
            if k && let Some(r) = e.data.clone() {
                let inline = r.start >= e.pos && r.end <= e.pos + 12;
                if !inline {
                    self.zero(r);
                }
            }
        }
        let base = ifd.off + 2;
        let mut w = 0usize;
        for (e, &k) in ifd.entries.iter().zip(kill) {
            if !k {
                self.b.copy_within(e.pos..e.pos + 12, base + w * 12);
                w += 1;
            }
        }
        let new_array_end = base + w * 12;
        if ifd.has_next {
            self.b
                .copy_within(old_array_end..old_array_end + 4, new_array_end);
            self.zero(new_array_end + 4..old_array_end + 4);
        } else {
            self.zero(new_array_end..old_array_end);
        }
        self.o.put16(self.b, ifd.off, w as u16);
        new_array_end
    }

    pub(crate) fn remove_gps(&mut self, e: &Entry) {
        if let Some(p) = self.ptr_value(e) {
            if let Ok(g) = self.read(p) {
                self.tally.gps_tags += g.entries.len();
            }
            self.zero_ifd(p, 0);
        } else {
            self.tally.gps_tags += 1;
        }
    }

    fn set_dim(&mut self, e: &Entry, v: u32) {
        if e.count != 1 {
            return;
        }
        match e.typ {
            3 => {
                if v <= 65535 {
                    self.o.put16(self.b, e.pos + 8, v as u16);
                } else {
                    self.o.put16(self.b, e.pos + 2, 4);
                    self.o.put32(self.b, e.pos + 8, v);
                }
            }
            4 => {
                self.o.put32(self.b, e.pos + 8, v);
            }
            _ => {}
        }
    }

    fn thumb_ranges(&self, ifd: &Ifd) -> (Vec<Range<usize>>, bool) {
        let mut out = Vec::new();
        let mut had = false;
        let find = |t: u16| ifd.entries.iter().find(|e| e.tag == t);
        if let (Some(a), Some(l)) = (find(TAG_THUMB_OFFSET), find(TAG_THUMB_LEN)) {
            had = true;
            if let (Some(off), Some(len)) =
                (uint_value(self.b, self.o, a), uint_value(self.b, self.o, l))
            {
                let s = (off as usize).min(self.b.len());
                let e = s.saturating_add(len as usize).min(self.b.len());
                if s < e {
                    out.push(s..e);
                }
            }
        }
        if let (Some(a), Some(l)) = (find(TAG_STRIP_OFFSETS), find(TAG_STRIP_COUNTS)) {
            had = true;
            let n = (a.count as usize).min(4096);
            let (Some(ar), Some(lr)) = (a.data.clone(), l.data.clone()) else {
                return (out, had);
            };
            if a.count != l.count {
                return (out, had);
            }
            for k in 0..n {
                let read = |e: &Entry, r: &Range<usize>| -> Option<usize> {
                    match e.typ {
                        3 => self.o.u16(self.b, r.start + k * 2).map(|v| v as usize),
                        4 => self.o.u32(self.b, r.start + k * 4).map(|v| v as usize),
                        _ => None,
                    }
                };
                if let (Some(s), Some(len)) = (read(a, &ar), read(l, &lr)) {
                    let s = s.min(self.b.len());
                    let e = s.saturating_add(len).min(self.b.len());
                    if s < e {
                        out.push(s..e);
                    }
                }
            }
        }
        (out, had)
    }

    /// Process one directory of the main chain (IFD0) or the Exif directory.
    pub(crate) fn process(
        &mut self,
        off: usize,
        kind: Kind,
        pol: &Policy,
        depth: u8,
    ) -> Result<(), MetaError> {
        if depth > MAX_DEPTH {
            return Err(corrupt("EXIF directories are nested too deeply"));
        }
        self.visit(off)?;
        let ifd = self.read(off)?;
        let mut kill = vec![false; ifd.entries.len()];
        let mut exif_sub = None;
        for (i, e) in ifd.entries.iter().enumerate() {
            let t = e.tag;
            if t == TAG_GPS_IFD {
                if pol.gps {
                    kill[i] = true;
                    self.remove_gps(e);
                }
                continue;
            }
            if t == TAG_EXIF_IFD && kind == Kind::Main {
                exif_sub = self.ptr_value(e);
                continue;
            }
            if pol.serials && SERIAL_TAGS.iter().any(|(s, _)| *s == t) {
                kill[i] = true;
                self.tally.serials += 1;
                continue;
            }
            if pol.owner && OWNER_TAGS.iter().any(|(s, _)| *s == t) {
                kill[i] = true;
                self.tally.owner += 1;
                continue;
            }
            if pol.comments && COMMENT_TAGS.contains(&t) {
                kill[i] = true;
                self.tally.comments += 1;
                continue;
            }
            if pol.maker_note && t == TAG_MAKER_NOTE {
                kill[i] = true;
                self.tally.maker_note_bytes += e.count as usize;
                continue;
            }
            if let Some(fix) = self.fix {
                let (w, h) = fix.dims;
                match (kind, t) {
                    (Kind::Main, TAG_ORIENTATION) => {
                        if e.typ == 3 && e.count == 1 {
                            self.o.put16(self.b, e.pos + 8, 1);
                        }
                    }
                    (Kind::Main, 0x0100) | (Kind::Exif, 0xA002) => self.set_dim(e, w),
                    (Kind::Main, 0x0101) | (Kind::Exif, 0xA003) => self.set_dim(e, h),
                    _ => {}
                }
            }
        }
        let next_pos = self.remove_entries(&ifd, &kill);
        if let Some(p) = exif_sub {
            self.process(p, Kind::Exif, pol, depth + 1)?;
        }
        if kind == Kind::Main && ifd.next != 0 && ifd.has_next {
            let nx = ifd.next;
            if self.visited.contains(&nx) {
                return Err(corrupt("EXIF directories loop back on themselves"));
            }
            if pol.thumbnails {
                if let Ok(t) = self.read(nx) {
                    let (ranges, had) = self.thumb_ranges(&t);
                    for r in ranges {
                        self.zero(r);
                    }
                    if had {
                        self.tally.thumbnails += 1;
                    }
                    self.zero_ifd(nx, 0);
                }
                self.o.put32(self.b, next_pos, 0);
            } else if pol.gps
                && self.jdepth < 3
                && let Ok(t) = self.read(nx)
            {
                let (ranges, _) = self.thumb_ranges(&t);
                let jd = self.jdepth + 1;
                for r in ranges {
                    if self
                        .b
                        .get(r.clone())
                        .is_some_and(|s| s.starts_with(&[0xFF, 0xD8]))
                    {
                        let mut log = super::Log::default();
                        // A damaged thumbnail is left alone. It is not part of the main image.
                        let _ = super::jpeg::gps_in_place(&mut self.b[r], jd, &mut log);
                        self.tally.gps_tags += log.tally.gps_tags;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Remove things from an EXIF block in place. Returns the tally and the
/// ranges that were zeroed.
pub(crate) fn filter(
    blob: &mut [u8],
    pol: &Policy,
    fix: Option<Fix>,
    jdepth: u8,
) -> Result<(Tally, Vec<Range<usize>>), MetaError> {
    let (mut ed, off0) = Editor::new(blob, jdepth)?;
    ed.fix = fix;
    ed.process(off0, Kind::Main, pol, 0)?;
    Ok((ed.tally, ed.zeroed))
}

/// The end of everything still reachable in the block.
fn used_end(b: &[u8]) -> usize {
    let Some((o, off0)) = parse_header(b) else {
        return b.len();
    };
    let mut stack = vec![off0];
    let mut seen: Vec<usize> = Vec::new();
    let mut end = 8usize;
    while let Some(off) = stack.pop() {
        if seen.contains(&off) || seen.len() > MAX_IFDS {
            continue;
        }
        seen.push(off);
        let Ok(ifd) = read_ifd(b, o, off) else {
            continue;
        };
        end = end.max(ifd.end);
        for e in &ifd.entries {
            if let Some(r) = &e.data {
                end = end.max(r.end);
            }
            if matches!(
                e.tag,
                TAG_EXIF_IFD | TAG_GPS_IFD | TAG_INTEROP_IFD | TAG_SUB_IFDS
            ) && matches!(e.typ, 4 | 13)
                && let Some(r) = &e.data
            {
                for k in 0..(e.count as usize).min(16) {
                    if let Some(p) = o.u32(b, r.start + k * 4) {
                        stack.push(p as usize);
                    }
                }
            }
        }
        // Thumbnail bytes referenced from this directory.
        let find = |t: u16| ifd.entries.iter().find(|e| e.tag == t);
        if let (Some(a), Some(l)) = (find(TAG_THUMB_OFFSET), find(TAG_THUMB_LEN))
            && let (Some(off), Some(len)) = (uint_value(b, o, a), uint_value(b, o, l))
        {
            end = end.max((off as usize).saturating_add(len as usize).min(b.len()));
        }
        if ifd.next != 0 {
            stack.push(ifd.next);
        }
    }
    end.min(b.len())
}

/// How many bytes can be cut from the end of a filtered block: only the tail
/// that was zeroed by removals, and never anything still referenced.
pub(crate) fn trimmed_len(blob: &[u8], zeroed: &[Range<usize>]) -> usize {
    let mut r: Vec<Range<usize>> = zeroed.to_vec();
    r.sort_by_key(|x| x.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for x in r {
        match merged.last_mut() {
            Some(last) if x.start <= last.end => last.end = last.end.max(x.end),
            _ => merged.push(x),
        }
    }
    let mut end = blob.len();
    if let Some(last) = merged.last()
        && last.end >= blob.len()
    {
        end = last.start;
    }
    end.max(used_end(blob)).max(8)
}

/// EXIF for a re-encoded image. See `exif_for_reencode` in the module docs.
pub(crate) fn for_reencode(
    exif: &[u8],
    new_w: u32,
    new_h: u32,
    keep_gps: bool,
    keep_maker_note: bool,
) -> Option<Vec<u8>> {
    let mut blob = exif.to_vec();
    let pol = Policy {
        gps: !keep_gps,
        thumbnails: true,
        maker_note: !keep_maker_note,
        ..Policy::default()
    };
    let (_, zeroed) = filter(
        &mut blob,
        &pol,
        Some(Fix {
            dims: (new_w, new_h),
        }),
        0,
    )
    .ok()?;
    let keep = trimmed_len(&blob, &zeroed);
    blob.truncate(keep);
    if !has_useful_content(&blob) {
        return None;
    }
    Some(blob)
}

fn has_useful_content(b: &[u8]) -> bool {
    let Some((o, off0)) = parse_header(b) else {
        return false;
    };
    let Ok(ifd0) = read_ifd(b, o, off0) else {
        return false;
    };
    let mut useful = 0usize;
    for e in &ifd0.entries {
        match e.tag {
            TAG_ORIENTATION | TAG_EXIF_IFD | TAG_INTEROP_IFD => {}
            TAG_GPS_IFD => {
                if let Some(p) = e.data.as_ref().and_then(|r| o.u32(b, r.start))
                    && let Ok(g) = read_ifd(b, o, p as usize)
                {
                    useful += g.entries.len();
                }
            }
            _ => useful += 1,
        }
    }
    if let Some(p) = ifd0
        .entries
        .iter()
        .find(|e| e.tag == TAG_EXIF_IFD)
        .and_then(|e| e.data.as_ref())
        .and_then(|r| o.u32(b, r.start))
        && let Ok(ex) = read_ifd(b, o, p as usize)
    {
        useful += ex
            .entries
            .iter()
            .filter(|e| !matches!(e.tag, TAG_INTEROP_IFD | 0xA002 | 0xA003))
            .count();
    }
    useful > 0
}
