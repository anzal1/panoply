//! The box layer of ISO base media files (MP4, MOV, M4A): headers, and a walk of the top level
//! that tells where the index (`moov`) and the media data (`mdat`) are, whether the file is cut
//! off, and whether anything is left over after the last box.
//!
//! This is the file layer ffmpeg hides (spec 4.33, "own MP4 box walker for moov position and
//! atoms ffmpeg hides"). It reads only box headers, so it costs a few reads for a file of any
//! size.

use std::io;

use super::source::Source;

/// A four character code.
pub type FourCc = [u8; 4];

/// Big-endian reads.
pub fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

/// Big-endian reads.
pub fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Big-endian reads.
pub fn be64(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_be_bytes(a)
}

/// A box header found in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxHeader {
    /// The four character code.
    pub typ: FourCc,
    /// Offset of the header in the file.
    pub start: u64,
    /// Header length: 8, or 16 with a 64-bit size.
    pub header_len: u8,
    /// Size of the whole box as stored (0 in the file means "to the end", stored here as the
    /// distance to the end of the file).
    pub size: u64,
    /// True when the stored size runs past the end of the file.
    pub truncated: bool,
}

impl BoxHeader {
    /// Offset of the first payload byte.
    pub fn payload(&self) -> u64 {
        self.start + u64::from(self.header_len)
    }

    /// Offset one past the box, clamped to the file.
    pub fn end(&self) -> u64 {
        self.start + self.size
    }

    /// The code as text.
    pub fn name(&self) -> String {
        fourcc_text(&self.typ)
    }
}

/// "moov".
pub fn fourcc_text(t: &FourCc) -> String {
    t.iter()
        .map(|&c| {
            if c.is_ascii_graphic() || c == b' ' {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}

fn plausible_type(t: &FourCc) -> bool {
    t.iter()
        .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b' ' | b'-' | b'_' | b'.' | 0xA9))
}

/// Read the header at `at`, or `None` when there is no sensible box there.
pub fn read_header(src: &Source, at: u64) -> io::Result<Option<BoxHeader>> {
    if at + 8 > src.len() {
        return Ok(None);
    }
    let h = src.read_exact_at(at, 8)?;
    let size32 = u64::from(be32(&h, 0));
    let typ: FourCc = [h[4], h[5], h[6], h[7]];
    if !plausible_type(&typ) {
        return Ok(None);
    }
    let remaining = src.len() - at;
    let (size, header_len) = match size32 {
        0 => (remaining, 8u8),
        1 => {
            if at + 16 > src.len() {
                return Ok(None);
            }
            let ext = src.read_exact_at(at + 8, 8)?;
            (be64(&ext, 0), 16u8)
        }
        n => (n, 8u8),
    };
    if size < u64::from(header_len) {
        return Ok(None);
    }
    let truncated = size > remaining;
    Ok(Some(BoxHeader {
        typ,
        start: at,
        header_len,
        size: size.min(remaining),
        truncated,
    }))
}

/// The top level of a file.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Every top-level box in order.
    pub boxes: Vec<BoxHeader>,
    /// File length.
    pub len: u64,
    /// Where bytes that are not a box begin, when the walk stopped before the end.
    pub garbage_at: Option<u64>,
    /// The brands in `ftyp`: the major brand first.
    pub brands: Vec<String>,
}

impl Layout {
    /// The first box with this code.
    pub fn find(&self, typ: &[u8; 4]) -> Option<&BoxHeader> {
        self.boxes.iter().find(|b| &b.typ == typ)
    }

    /// All boxes with this code.
    pub fn all<'a>(&'a self, typ: &'a [u8; 4]) -> impl Iterator<Item = &'a BoxHeader> {
        self.boxes.iter().filter(move |b| &b.typ == typ)
    }

    /// True when the index comes before the media data (the file starts streaming at once).
    pub fn moov_first(&self) -> Option<bool> {
        let moov = self.find(b"moov")?;
        let first_mdat = self.find(b"mdat")?;
        Some(moov.start < first_mdat.start)
    }

    /// True when the file has fragments (`moof`).
    pub fn is_fragmented(&self) -> bool {
        self.find(b"moof").is_some()
    }

    /// Total bytes of all `mdat` boxes.
    pub fn mdat_bytes(&self) -> u64 {
        self.all(b"mdat")
            .map(|b| b.size - u64::from(b.header_len))
            .sum()
    }
}

/// Walk the top-level boxes. Stops, without error, at the first byte that is not a box.
pub fn scan(src: &Source) -> io::Result<Layout> {
    let mut boxes = Vec::new();
    let mut at = 0u64;
    let mut garbage_at = None;
    while at < src.len() {
        match read_header(src, at)? {
            Some(h) => {
                let end = h.end();
                boxes.push(h);
                if h.truncated || end <= at {
                    break;
                }
                at = end;
            }
            None => {
                garbage_at = Some(at);
                break;
            }
        }
    }
    let mut brands = Vec::new();
    if let Some(f) = boxes.iter().find(|b| &b.typ == b"ftyp") {
        let n = (f.size - u64::from(f.header_len)).min(256) as usize;
        let body = src.read_up_to(f.payload(), n)?;
        if body.len() >= 4 {
            brands.push(String::from_utf8_lossy(&body[0..4]).trim().to_string());
            let mut i = 8;
            while i + 4 <= body.len() {
                brands.push(String::from_utf8_lossy(&body[i..i + 4]).trim().to_string());
                i += 4;
            }
        }
    }
    Ok(Layout {
        boxes,
        len: src.len(),
        garbage_at,
        brands,
    })
}

/// Split the payload of a container box into `(type, start, end)` child ranges within `data`.
pub fn children(data: &[u8]) -> Vec<(FourCc, usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let size32 = be32(data, at) as usize;
        let typ: FourCc = [data[at + 4], data[at + 5], data[at + 6], data[at + 7]];
        let (size, hdr) = match size32 {
            0 => (data.len() - at, 8),
            1 => {
                if at + 16 > data.len() {
                    break;
                }
                (be64(data, at + 8) as usize, 16)
            }
            n => (n, 8),
        };
        if size < hdr || at + size > data.len() {
            break;
        }
        out.push((typ, at + hdr, at + size));
        at += size;
    }
    out
}

/// The payload of the first child with this code.
pub fn child<'a>(data: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    children(data)
        .into_iter()
        .find(|(t, _, _)| t == typ)
        .map(|(_, s, e)| &data[s..e])
}

/// A video sample entry without the child boxes that only describe the stream's statistics
/// (`btrt`), so two entries that configure a decoder the same way compare equal. A video entry is
/// 8 bytes of box header and 78 bytes of fixed fields, then child boxes.
pub fn config_key(entry: &[u8]) -> Vec<u8> {
    const FIXED: usize = 86;
    if entry.len() <= FIXED {
        return entry.to_vec();
    }
    let mut out = entry[..FIXED].to_vec();
    for (typ, s, e) in children(&entry[FIXED..]) {
        if &typ != b"btrt" {
            out.extend_from_slice(&entry[FIXED + s - 8..FIXED + e]);
        }
    }
    out
}

/// A box as bytes: size, type, payload.
pub fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = 8 + payload.len();
    let mut v = Vec::with_capacity(size);
    v.extend_from_slice(&(size as u32).to_be_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(payload);
    v
}

/// A full box (version and flags) as bytes.
pub fn full(typ: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + payload.len());
    p.push(version);
    p.extend_from_slice(&flags.to_be_bytes()[1..]);
    p.extend_from_slice(payload);
    bx(typ, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str, bytes: &[u8]) -> (std::path::PathBuf, Source) {
        let p = std::env::temp_dir().join(format!("panoply-boxes-{tag}-{}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        let s = Source::open(&p).unwrap();
        (p, s)
    }

    fn sample_file() -> Vec<u8> {
        let mut v = bx(b"ftyp", b"isom\0\0\x02\0isomiso2mp41");
        v.extend(bx(b"mdat", &[7u8; 100]));
        v.extend(bx(b"moov", &bx(b"mvhd", &[0u8; 100])));
        v
    }

    #[test]
    fn the_walk_finds_the_index_after_the_media_data() {
        let (p, s) = tmp("walk", &sample_file());
        let l = scan(&s).unwrap();
        let names: Vec<_> = l.boxes.iter().map(|b| b.name()).collect();
        assert_eq!(names, ["ftyp", "mdat", "moov"]);
        assert_eq!(l.moov_first(), Some(false));
        assert_eq!(l.brands[0], "isom");
        assert_eq!(l.mdat_bytes(), 100);
        assert!(l.garbage_at.is_none());
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn a_cut_off_file_is_flagged_not_an_error() {
        let mut v = sample_file();
        v.truncate(v.len() - 60);
        let (p, s) = tmp("cut", &v);
        let l = scan(&s).unwrap();
        assert!(l.find(b"moov").unwrap().truncated);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn a_zero_size_mdat_runs_to_the_end_and_garbage_is_located() {
        let mut v = bx(b"ftyp", b"isom\0\0\x02\0");
        let mut m = vec![0u8; 8];
        m[4..8].copy_from_slice(b"mdat");
        m.extend_from_slice(&[1u8; 50]);
        v.extend(m);
        let (p, s) = tmp("zero", &v);
        let l = scan(&s).unwrap();
        assert_eq!(l.find(b"mdat").unwrap().size, 58);
        std::fs::remove_file(p).unwrap();
        let mut g = sample_file();
        g.extend_from_slice(&[0xFF; 20]);
        let (p, s) = tmp("garbage", &g);
        let l = scan(&s).unwrap();
        assert!(l.garbage_at.is_some());
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn children_split_a_container() {
        let mut inner = bx(b"aaaa", b"12");
        inner.extend(bx(b"bbbb", b"345"));
        let c = children(&inner);
        assert_eq!(c.len(), 2);
        assert_eq!(&inner[c[1].1..c[1].2], b"345");
        assert_eq!(child(&inner, b"aaaa"), Some(&b"12"[..]));
    }
}
