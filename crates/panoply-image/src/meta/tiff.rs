//! Classic TIFF files. The metadata lives in IFD0, so removal is done by tag.
//! Strip offsets, tile offsets, compression and photometric tags are never
//! touched, and no pixel byte moves.

use super::exif::{self, Editor, Kind, Order, Policy};
use super::{Log, MetaError, Report, StripLevel, corrupt, jpeg, xmp};

const TAG_XMP: u16 = 0x02BC;
const TAG_IPTC: u16 = 0x83BB;
const TAG_PHOTOSHOP: u16 = 0x8649;
const TAG_ICC: u16 = 0x8773;
const TAG_DNG_VERSION: u16 = 0xC612;
const MAX_PAGES: usize = 4096;

/// Tags that change how pixels are read or shown. Level Everything keeps only these.
const KEEP: &[u16] = &[
    0x00FE, 0x00FF, 0x0100, 0x0101, 0x0102, 0x0103, 0x0106, 0x0107, 0x0108, 0x0109, 0x010A, 0x0111,
    0x0112, 0x0115, 0x0116, 0x0117, 0x0118, 0x0119, 0x011A, 0x011B, 0x011C, 0x0120, 0x0121, 0x0122,
    0x0123, 0x0124, 0x0125, 0x0128, 0x0129, 0x012D, 0x013D, 0x013E, 0x013F, 0x0140, 0x0141, 0x0142,
    0x0143, 0x0144, 0x0145, 0x014A, 0x014C, 0x014D, 0x014E, 0x0150, 0x0152, 0x0153, 0x0154, 0x0155,
    0x0156, 0x015B, 0x0200, 0x0201, 0x0202, 0x0203, 0x0205, 0x0206, 0x0207, 0x0208, 0x0209, 0x0211,
    0x0212, 0x0213, 0x0214, TAG_ICC,
];

/// Extra tags removed at level Private: names, captions, XMP, IPTC, Photoshop.
const PRIVATE_EXTRA: &[u16] = &[0x010D, 0x011D, TAG_XMP, TAG_IPTC, TAG_PHOTOSHOP];

fn is_bigtiff(b: &[u8]) -> bool {
    b.starts_with(b"II+\0") || b.starts_with(b"MM\0+")
}

fn bigtiff_error() -> MetaError {
    MetaError::Unsupported(
        "This is a BigTIFF file, which is not supported for metadata work. Save it as a regular TIFF and try again."
            .to_string(),
    )
}

fn ifd0(b: &[u8]) -> Result<(Order, exif::Ifd), MetaError> {
    if is_bigtiff(b) {
        return Err(bigtiff_error());
    }
    let (o, off) = exif::parse_header(b).ok_or_else(|| corrupt("the TIFF header is invalid"))?;
    Ok((o, exif::read_ifd(b, o, off)?))
}

pub(crate) fn inspect(b: &[u8], r: &mut Report) -> Result<(), MetaError> {
    let (_, ifd) = ifd0(b)?;
    exif::inspect(b, true, r);
    r.exif_present = ifd.entries.iter().any(|e| {
        matches!(
            e.tag,
            exif::TAG_EXIF_IFD | exif::TAG_GPS_IFD | 0x010F | 0x0110
        )
    });
    for e in &ifd.entries {
        let Some(d) = e.data.clone().and_then(|d| b.get(d)) else {
            continue;
        };
        match e.tag {
            TAG_XMP => xmp::inspect(d, r),
            TAG_IPTC => r.iptc = true,
            TAG_PHOTOSHOP => {
                let (iptc, t) = jpeg::photoshop_info(d);
                r.iptc |= iptc;
                r.thumbnails += t;
            }
            TAG_ICC => r.icc = true,
            _ => {}
        }
    }
    Ok(())
}

fn tag_data(b: &[u8], tag: u16) -> Option<Vec<u8>> {
    let (_, ifd) = ifd0(b).ok()?;
    let e = ifd.entries.iter().find(|e| e.tag == tag)?;
    Some(b.get(e.data.clone()?)?.to_vec())
}

pub(crate) fn extract_xmp(b: &[u8]) -> Option<Vec<u8>> {
    tag_data(b, TAG_XMP)
}

pub(crate) fn extract_icc(b: &[u8]) -> Option<Vec<u8>> {
    tag_data(b, TAG_ICC)
}

pub(crate) fn strip(b: &[u8], level: StripLevel) -> Result<(Vec<u8>, Log), MetaError> {
    if is_bigtiff(b) {
        return Err(bigtiff_error());
    }
    let everything = level == StripLevel::Everything;
    let mut out = b.to_vec();
    let mut log = Log::default();
    let (mut ed, off0) = Editor::new(&mut out, 0)?;
    let pol = match level {
        StripLevel::Gps => Policy::gps_only(),
        _ => Policy::private(),
    };

    let mut off = off0;
    let mut pages = 0usize;
    while off != 0 {
        pages += 1;
        if pages > MAX_PAGES {
            return Err(corrupt("the TIFF has too many pages"));
        }
        ed.visit(off)?;
        let ifd = ed.read(off)?;
        let is_raw = ifd.entries.iter().any(|e| e.tag == TAG_DNG_VERSION);
        if everything && is_raw {
            return Err(MetaError::Unsupported(
                "This is a raw camera file. Removing everything would break it. Use the Private level, or export a TIFF or JPEG first."
                    .to_string(),
            ));
        }
        let mut kill = vec![false; ifd.entries.len()];
        let mut exif_sub = None;
        for (i, e) in ifd.entries.iter().enumerate() {
            let t = e.tag;
            if t == exif::TAG_GPS_IFD {
                kill[i] = true;
                ed.remove_gps(e);
                continue;
            }
            if level == StripLevel::Gps {
                if t == TAG_XMP
                    && let Some(r) = e.data.clone()
                    && let Some(s) = ed.b.get_mut(r)
                {
                    log.xmp_gps += xmp::blank_gps(s);
                }
                continue;
            }
            if t == exif::TAG_EXIF_IFD {
                if everything {
                    kill[i] = true;
                    if let Some(p) = ed.ptr_value(e) {
                        ed.zero_ifd(p, 0);
                    }
                } else {
                    exif_sub = ed.ptr_value(e);
                }
                continue;
            }
            let mut remove = false;
            if exif::SERIAL_TAGS.iter().any(|(s, _)| *s == t) {
                ed.tally.serials += 1;
                remove = true;
            } else if exif::OWNER_TAGS.iter().any(|(s, _)| *s == t) {
                ed.tally.owner += 1;
                remove = true;
            } else if exif::COMMENT_TAGS.contains(&t) || t == 0x010D || t == 0x011D {
                ed.tally.comments += 1;
                remove = true;
            } else if t == TAG_XMP {
                log.xmp = true;
                remove = true;
            } else if t == TAG_IPTC {
                log.iptc = true;
                remove = true;
            } else if t == TAG_PHOTOSHOP {
                let (iptc, th) = match e.data.clone().and_then(|d| ed.b.get(d)) {
                    Some(d) => jpeg::photoshop_info(d),
                    None => (false, 0),
                };
                log.iptc |= iptc;
                log.thumbs_extra += th;
                if !iptc {
                    log.extra("Photoshop data");
                }
                remove = true;
            } else if PRIVATE_EXTRA.contains(&t) || (everything && !KEEP.contains(&t)) {
                remove = true;
            }
            if remove {
                kill[i] = true;
            }
        }
        ed.remove_entries(&ifd, &kill);
        if let Some(p) = exif_sub {
            ed.process(p, Kind::Exif, &pol, 1)?;
        }
        off = ifd.next;
    }
    log.tally.add(&ed.tally);
    if everything {
        log.kept_extra.push("pixel layout tags".to_string());
    }
    Ok((out, log))
}
