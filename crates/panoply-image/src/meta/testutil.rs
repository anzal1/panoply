//! Builders and helpers for synthetic test inputs.

use std::ops::Range;
use std::path::PathBuf;

pub(crate) const SERIAL_BODY: &str = "BODYSERIAL-8841-XYZ";
pub(crate) const SERIAL_LENS: &str = "LENSSERIAL-5520-QRS";
pub(crate) const UNIQUE_ID: &str = "UNIQUEID-0123456789ABCDEF";
pub(crate) const OWNER: &str = "OWNER-JANE-DOE";
pub(crate) const ARTIST: &str = "ARTIST-JANE-Q";
pub(crate) const COPYRIGHT: &str = "COPYRIGHT-JANE-2026";
pub(crate) const COMMENT: &str = "USERCOMMENT-SECRET-NOTE";
pub(crate) const MAKER: &[u8] = b"MAKERNOTE-SECRET-BYTES-0123456789";
pub(crate) const THUMB_MARK: &[u8] = b"THUMBNAIL-SECRET-PIXELS";
pub(crate) const XMP_CREATOR: &str = "XMPCREATOR-JANE";

/// Find a corpus file, or print why the test is skipped.
pub(crate) fn corpus(rel: &str) -> Option<Vec<u8>> {
    let base = match std::env::var("PANOPLY_REPO") {
        Ok(r) => PathBuf::from(r).join("tests/corpus/cache/image"),
        Err(_) => PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/corpus/cache/image"
        )),
    };
    match std::fs::read(base.join(rel)) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("skipping: corpus file {rel} is missing");
            None
        }
    }
}

pub(crate) fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------------------
// TIFF / EXIF builder

pub(crate) struct E {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    pub payload: Vec<u8>,
}

fn w16(le: bool, v: u16) -> [u8; 2] {
    if le { v.to_le_bytes() } else { v.to_be_bytes() }
}
fn w32(le: bool, v: u32) -> [u8; 4] {
    if le { v.to_le_bytes() } else { v.to_be_bytes() }
}

pub(crate) fn e_ascii(le: bool, tag: u16, s: &str) -> E {
    let _ = le;
    let mut p = s.as_bytes().to_vec();
    p.push(0);
    E {
        tag,
        typ: 2,
        count: p.len() as u32,
        payload: p,
    }
}
pub(crate) fn e_short(le: bool, tag: u16, v: u16) -> E {
    E {
        tag,
        typ: 3,
        count: 1,
        payload: w16(le, v).to_vec(),
    }
}
pub(crate) fn e_long(le: bool, tag: u16, v: u32) -> E {
    E {
        tag,
        typ: 4,
        count: 1,
        payload: w32(le, v).to_vec(),
    }
}
pub(crate) fn e_rats(le: bool, tag: u16, r: &[(u32, u32)]) -> E {
    let mut p = Vec::new();
    for (n, d) in r {
        p.extend_from_slice(&w32(le, *n));
        p.extend_from_slice(&w32(le, *d));
    }
    E {
        tag,
        typ: 5,
        count: r.len() as u32,
        payload: p,
    }
}
pub(crate) fn e_undef(tag: u16, b: &[u8]) -> E {
    E {
        tag,
        typ: 7,
        count: b.len() as u32,
        payload: b.to_vec(),
    }
}
pub(crate) fn e_bytes(tag: u16, b: &[u8]) -> E {
    E {
        tag,
        typ: 1,
        count: b.len() as u32,
        payload: b.to_vec(),
    }
}

pub(crate) struct Opts {
    pub le: bool,
    pub orientation: u16,
    pub thumbnail: bool,
    pub maker_note: Option<Vec<u8>>,
    pub gps: bool,
    pub exif_ifd: bool,
    /// Pixel strip bytes for a TIFF file. Adds the image tags.
    pub strip: Option<Vec<u8>>,
    pub extra_ifd0: Vec<E>,
    pub ifd0_next_self: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            le: false,
            orientation: 6,
            thumbnail: true,
            maker_note: Some(MAKER.to_vec()),
            gps: true,
            exif_ifd: true,
            strip: None,
            extra_ifd0: Vec::new(),
            ifd0_next_self: false,
        }
    }
}

pub(crate) fn thumb_jpeg() -> Vec<u8> {
    let mut t = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0x00, 0x00];
    t.extend_from_slice(THUMB_MARK);
    t.extend_from_slice(&[0xFF, 0xD9]);
    t
}

/// The raw rational bytes of the latitude, longitude and altitude values.
pub(crate) fn gps_needles(le: bool) -> Vec<Vec<u8>> {
    let lat = e_rats(le, 2, &[(37, 1), (46, 1), (297, 10)]).payload;
    let lon = e_rats(le, 4, &[(122, 1), (25, 1), (984, 100)]).payload;
    let alt = e_rats(le, 6, &[(1234, 10)]).payload;
    vec![lat, lon, alt]
}

fn ifd_size(n: usize) -> usize {
    2 + n * 12 + 4
}

pub(crate) fn build_exif(o: &Opts) -> Vec<u8> {
    let le = o.le;
    let mut ifd0: Vec<E> = vec![
        e_ascii(le, 0x010F, "TestCam"),
        e_ascii(le, 0x0110, "TestCam Model X"),
        e_short(le, 0x0112, o.orientation),
        e_ascii(le, 0x0131, "SynthSoft 1.0"),
        e_ascii(le, 0x0132, "2026:01:02 03:04:05"),
        e_ascii(le, 0x013B, ARTIST),
        e_ascii(le, 0x8298, COPYRIGHT),
    ];
    let mut exif: Vec<E> = vec![
        e_ascii(le, 0x9003, "2026:01:02 03:04:05"),
        e_ascii(le, 0x9286, COMMENT),
        e_long(le, 0xA002, 4000),
        e_long(le, 0xA003, 3000),
        e_ascii(le, 0xA420, UNIQUE_ID),
        e_ascii(le, 0xA430, OWNER),
        e_ascii(le, 0xA431, SERIAL_BODY),
        e_ascii(le, 0xA434, "TestLens 50mm"),
        e_ascii(le, 0xA435, SERIAL_LENS),
    ];
    if let Some(m) = &o.maker_note {
        exif.push(e_undef(0x927C, m));
    }
    let gps: Vec<E> = vec![
        e_bytes(0, &[2, 3, 0, 0]),
        e_ascii(le, 1, "N"),
        e_rats(le, 2, &[(37, 1), (46, 1), (297, 10)]),
        e_ascii(le, 3, "W"),
        e_rats(le, 4, &[(122, 1), (25, 1), (984, 100)]),
        e_bytes(5, &[0]),
        e_rats(le, 6, &[(1234, 10)]),
    ];
    let ifd1: Vec<E> = vec![
        e_short(le, 0x0103, 6),
        e_long(le, 0x0201, 0),
        e_long(le, 0x0202, 0),
    ];
    let with_exif = o.exif_ifd;
    // Placeholder pointers so the counts are right. They are patched below.
    if with_exif {
        ifd0.push(e_long(le, 0x8769, 0));
    }
    if o.gps {
        ifd0.push(e_long(le, 0x8825, 0));
    }
    if let Some(strip) = &o.strip {
        let n = strip.len() as u32;
        ifd0.push(e_long(le, 0x0100, 2));
        ifd0.push(e_long(le, 0x0101, 2));
        ifd0.push(e_short(le, 0x0102, 8));
        ifd0.push(e_short(le, 0x0103, 1));
        ifd0.push(e_short(le, 0x0106, 1));
        ifd0.push(e_long(le, 0x0111, 0));
        ifd0.push(e_short(le, 0x0115, 1));
        ifd0.push(e_long(le, 0x0116, 2));
        ifd0.push(e_long(le, 0x0117, n));
        ifd0.push(e_undef(0x8773, &[0x49; 64]));
        ifd0.push(e_ascii(le, 0x010E, "IMAGEDESC-SECRET"));
    }
    ifd0.extend(o.extra_ifd0.iter().map(|e| E {
        tag: e.tag,
        typ: e.typ,
        count: e.count,
        payload: e.payload.clone(),
    }));
    ifd0.sort_by_key(|e| e.tag);

    let mut cursor = 8usize;
    let ifd0_off = cursor;
    cursor += ifd_size(ifd0.len());
    let exif_off = cursor;
    if with_exif {
        cursor += ifd_size(exif.len());
    }
    let gps_off = cursor;
    if o.gps {
        cursor += ifd_size(gps.len());
    }
    let ifd1_off = cursor;
    if o.thumbnail {
        cursor += ifd_size(ifd1.len());
    }
    let data_base = cursor;
    let mut data: Vec<u8> = Vec::new();
    let mut strip_off = 0usize;
    if let Some(s) = &o.strip {
        strip_off = data_base + data.len();
        data.extend_from_slice(s);
        if data.len() % 2 == 1 {
            data.push(0);
        }
    }
    let thumb = thumb_jpeg();
    let mut thumb_off = 0usize;
    if o.thumbnail {
        thumb_off = data_base + data.len();
        data.extend_from_slice(&thumb);
        if data.len() % 2 == 1 {
            data.push(0);
        }
    }

    let patch = |list: &mut Vec<E>, tag: u16, v: u32| {
        for e in list.iter_mut() {
            if e.tag == tag {
                e.payload = w32(le, v).to_vec();
            }
        }
    };
    patch(&mut ifd0, 0x8769, exif_off as u32);
    patch(&mut ifd0, 0x8825, gps_off as u32);
    patch(&mut ifd0, 0x0111, strip_off as u32);
    let mut ifd1 = ifd1;
    patch(&mut ifd1, 0x0201, thumb_off as u32);
    patch(&mut ifd1, 0x0202, thumb.len() as u32);

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(if le { b"II*\0" } else { b"MM\0*" });
    out.extend_from_slice(&w32(le, ifd0_off as u32));
    let write_ifd = |out: &mut Vec<u8>, list: &[E], next: u32, data: &mut Vec<u8>| {
        out.extend_from_slice(&w16(le, list.len() as u16));
        for e in list {
            out.extend_from_slice(&w16(le, e.tag));
            out.extend_from_slice(&w16(le, e.typ));
            out.extend_from_slice(&w32(le, e.count));
            if e.payload.len() <= 4 {
                let mut v = e.payload.clone();
                v.resize(4, 0);
                out.extend_from_slice(&v);
            } else {
                let off = data_base + data.len();
                out.extend_from_slice(&w32(le, off as u32));
                data.extend_from_slice(&e.payload);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        out.extend_from_slice(&w32(le, next));
    };
    let next0 = if o.ifd0_next_self {
        ifd0_off as u32
    } else if o.thumbnail {
        ifd1_off as u32
    } else {
        0
    };
    write_ifd(&mut out, &ifd0, next0, &mut data);
    if with_exif {
        write_ifd(&mut out, &exif, 0, &mut data);
    }
    if o.gps {
        write_ifd(&mut out, &gps, 0, &mut data);
    }
    if o.thumbnail {
        write_ifd(&mut out, &ifd1, 0, &mut data);
    }
    assert_eq!(out.len(), data_base);
    out.extend_from_slice(&data);
    out
}

/// An Apple style MakerNote holding a Live Photo content identifier.
pub(crate) fn apple_maker_note(uuid: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"Apple iOS\0\0\x01MM");
    v.extend_from_slice(&1u16.to_be_bytes());
    let mut id = uuid.as_bytes().to_vec();
    id.push(0);
    v.extend_from_slice(&0x0011u16.to_be_bytes());
    v.extend_from_slice(&2u16.to_be_bytes());
    v.extend_from_slice(&(id.len() as u32).to_be_bytes());
    let data_off = 14 + 2 + 12 + 4;
    v.extend_from_slice(&(data_off as u32).to_be_bytes());
    v.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(v.len(), data_off);
    v.extend_from_slice(&id);
    v
}

// ---------------------------------------------------------------------------
// JPEG builder and an independent marker walker

pub(crate) fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

pub(crate) fn xmp_packet() -> Vec<u8> {
    let s = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n<rdf:Description rdf:about=\"\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n exif:GPSLatitude=\"37,46.495N\" exif:GPSLongitude=\"122,25.164W\" xmp:CreatorTool=\"SynthSoft\">\n<dc:creator><rdf:Seq><rdf:li>{XMP_CREATOR}</rdf:li></rdf:Seq></dc:creator>\n<exif:GPSAltitude>1234/10</exif:GPSAltitude>\n</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"
    );
    s.into_bytes()
}

pub(crate) fn photoshop_block() -> Vec<u8> {
    let mut v = b"Photoshop 3.0\0".to_vec();
    // IPTC resource 0x0404 with a short record.
    let iptc = b"\x1c\x02\x78IPTC-SECRET-CAPTION";
    v.extend_from_slice(b"8BIM");
    v.extend_from_slice(&0x0404u16.to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
    v.extend_from_slice(iptc);
    if iptc.len() % 2 == 1 {
        v.push(0);
    }
    // Thumbnail resource 0x040C.
    let th = [0u8; 8];
    v.extend_from_slice(b"8BIM");
    v.extend_from_slice(&0x040Cu16.to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&(th.len() as u32).to_be_bytes());
    v.extend_from_slice(&th);
    v
}

pub(crate) const ENTROPY: &[u8] = &[
    0x11, 0x22, 0xFF, 0x00, 0x33, 0xFF, 0xD0, 0x44, 0x55, 0xFF, 0x00, 0xFE, 0x77, 0x88, 0x99,
];

pub(crate) struct JpegOpts {
    pub exif: Option<Vec<u8>>,
    pub xmp: bool,
    pub extras: bool,
}

/// A JPEG made from hand written segments. The entropy data is not real, but
/// the structure is valid for a marker walker.
pub(crate) fn synth_jpeg(o: &JpegOpts) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8];
    v.extend(seg(0xE0, b"JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00"));
    if let Some(e) = &o.exif {
        let mut p = b"Exif\0\0".to_vec();
        p.extend_from_slice(e);
        v.extend(seg(0xE1, &p));
    }
    if o.xmp {
        let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        p.extend_from_slice(&xmp_packet());
        v.extend(seg(0xE1, &p));
    }
    let mut icc = b"ICC_PROFILE\0\x01\x01".to_vec();
    icc.extend_from_slice(&[0x61; 128]);
    v.extend(seg(0xE2, &icc));
    if o.extras {
        v.extend(seg(0xED, &photoshop_block()));
        v.extend(seg(0xEB, b"JP\0\0\0\0\0\0jumbc2pa-manifest-secret"));
        v.extend(seg(0xFE, b"COMMENT-SECRET-TEXT"));
        v.extend(seg(0xEE, b"Adobe\0\x64\x00\x00\x00\x00\x00"));
    }
    v.extend(seg(0xDB, &[0x00; 65]));
    v.extend(seg(0xC0, &[8, 0, 8, 0, 8, 1, 1, 0x11, 0]));
    v.extend(seg(0xC4, &[0x00; 17]));
    v.extend(seg(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
    v.extend_from_slice(ENTROPY);
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}

/// Independent walker: markers other than APPn and COM with payloads, plus scan data.
pub(crate) fn jpeg_image_parts(b: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    assert_eq!(&b[0..2], &[0xFF, 0xD8]);
    let mut i = 2;
    while i + 1 < b.len() {
        assert_eq!(b[i], 0xFF, "marker expected at {i}");
        while b[i + 1] == 0xFF {
            i += 1;
        }
        let m = b[i + 1];
        if m == 0xD9 {
            break;
        }
        if m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let payload = &b[i + 4..i + 2 + len];
        if !((0xE0..=0xEF).contains(&m) || m == 0xFE) {
            out.push((m, payload.to_vec()));
        }
        i += 2 + len;
        if m == 0xDA {
            let s = i;
            loop {
                if i >= b.len() {
                    break;
                }
                if b[i] == 0xFF
                    && i + 1 < b.len()
                    && b[i + 1] != 0
                    && !(0xD0..=0xD7).contains(&b[i + 1])
                    && b[i + 1] != 0xFF
                {
                    break;
                }
                i += 1;
            }
            out.push((0, b[s..i].to_vec()));
        }
    }
    out
}

pub(crate) fn jpeg_marker_list(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 2;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            break;
        }
        let m = b[i + 1];
        out.push(m);
        if m == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        i += 2 + len;
        if m == 0xDA {
            while i + 1 < b.len()
                && !(b[i] == 0xFF && b[i + 1] != 0 && !(0xD0..=0xD7).contains(&b[i + 1]))
            {
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// PNG helpers

pub(crate) fn png_chunk(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(data.len() as u32).to_be_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(data);
    v.extend_from_slice(&super::crc32(&[typ, data]).to_be_bytes());
    v
}

pub(crate) fn synth_png(exif: Option<&[u8]>) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend(png_chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]));
    v.extend(png_chunk(b"gAMA", &[0, 1, 0x86, 0xA0]));
    v.extend(png_chunk(b"iCCP", b"synth\0\0\x78\x9cICCDATA"));
    v.extend(png_chunk(b"pHYs", &[0, 0, 0x0B, 0x13, 0, 0, 0x0B, 0x13, 1]));
    v.extend(png_chunk(b"tIME", &[0x07, 0xEA, 1, 2, 3, 4, 5]));
    v.extend(png_chunk(b"tEXt", b"Comment\0PNGTEXT-SECRET"));
    v.extend(png_chunk(b"tEXt", b"Software\0SynthSoft"));
    v.extend(png_chunk(b"zTXt", b"Raw profile type exif\0\0\x78\x9cZZZ"));
    let mut x = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
    x.extend_from_slice(&xmp_packet());
    v.extend(png_chunk(b"iTXt", &x));
    if let Some(e) = exif {
        v.extend(png_chunk(b"eXIf", e));
    }
    v.extend(png_chunk(b"caBX", b"jumbc2pa-secret"));
    v.extend(png_chunk(b"IDAT", &[0x78, 0x01, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
    v.extend(png_chunk(b"IDAT", &[10, 11, 12]));
    v.extend(png_chunk(b"IEND", &[]));
    v
}

/// Every chunk of a PNG as (type, data), checking CRCs on the way.
pub(crate) fn png_chunks_checked(b: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let typ = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        let data = &b[i + 8..i + 8 + len];
        let crc = u32::from_be_bytes([
            b[i + 8 + len],
            b[i + 9 + len],
            b[i + 10 + len],
            b[i + 11 + len],
        ]);
        assert_eq!(
            crc,
            super::crc32(&[&typ, data]),
            "bad CRC in {}",
            String::from_utf8_lossy(&typ)
        );
        out.push((typ, data.to_vec()));
        i += 12 + len;
    }
    out
}

pub(crate) fn png_data_parts(b: &[u8]) -> Vec<Vec<u8>> {
    png_chunks_checked(b)
        .into_iter()
        .filter(|(t, _)| {
            matches!(
                t,
                b"IHDR"
                    | b"IDAT"
                    | b"PLTE"
                    | b"tRNS"
                    | b"gAMA"
                    | b"cHRM"
                    | b"sRGB"
                    | b"iCCP"
                    | b"pHYs"
                    | b"bKGD"
                    | b"sBIT"
                    | b"acTL"
                    | b"fcTL"
                    | b"fdAT"
            )
        })
        .map(|(t, d)| {
            let mut v = t.to_vec();
            v.extend(d);
            v
        })
        .collect()
}

// ---------------------------------------------------------------------------
// WebP helpers

pub(crate) fn riff_chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(data);
    if data.len() % 2 == 1 {
        v.push(0);
    }
    v
}

pub(crate) fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut v = b"RIFF".to_vec();
    v.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    v.extend_from_slice(b"WEBP");
    v.extend(body);
    v
}

pub(crate) fn vp8_chunk(w: u16, h: u16) -> Vec<u8> {
    let mut d = vec![0x10, 0x02, 0x00, 0x9D, 0x01, 0x2A];
    d.extend_from_slice(&w.to_le_bytes());
    d.extend_from_slice(&h.to_le_bytes());
    d.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
    riff_chunk(b"VP8 ", &d)
}

pub(crate) fn vp8l_chunk(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let bits: u32 = (w - 1) | ((h - 1) << 14) | (u32::from(alpha) << 28);
    let mut d = vec![0x2F];
    d.extend_from_slice(&bits.to_le_bytes());
    d.extend_from_slice(&[9, 8, 7, 6, 5]);
    riff_chunk(b"VP8L", &d)
}

pub(crate) fn webp_parts(b: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = b[i..i + 4].to_vec();
        let n = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let data = b[i + 8..i + 8 + n].to_vec();
        if matches!(
            id.as_slice(),
            b"VP8 " | b"VP8L" | b"ALPH" | b"ANMF" | b"ANIM" | b"ICCP"
        ) {
            out.push((id, data));
        }
        i += 8 + n + (n & 1);
    }
    out
}

pub(crate) fn webp_ids(b: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 12;
    while i + 8 <= b.len() {
        out.push(String::from_utf8_lossy(&b[i..i + 4]).to_string());
        let n = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        i += 8 + n + (n & 1);
    }
    out
}

// ---------------------------------------------------------------------------
// BMFF builder

fn bx(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = ((data.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(typ);
    v.extend_from_slice(data);
    v
}

fn fullbox(typ: &[u8; 4], ver: u8, data: &[u8]) -> Vec<u8> {
    let mut d = vec![ver, 0, 0, 0];
    d.extend_from_slice(data);
    bx(typ, &d)
}

pub(crate) struct Heic {
    pub bytes: Vec<u8>,
    /// Ranges of the Exif item payload, the XMP item payload and the image items.
    pub exif: Range<usize>,
    pub xmp: Range<usize>,
    pub image: Range<usize>,
}

pub(crate) fn synth_heic(tiff: &[u8]) -> Heic {
    synth_heic_with(tiff, false)
}

/// Like [`synth_heic`]. With `idat` the Exif item lives in an `idat` box
/// (construction method 1) instead of `mdat`.
pub(crate) fn synth_heic_with(tiff: &[u8], idat: bool) -> Heic {
    let mut exif_payload = 6u32.to_be_bytes().to_vec();
    exif_payload.extend_from_slice(b"Exif\0\0");
    exif_payload.extend_from_slice(tiff);
    let xmp = xmp_packet();
    let image: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();
    let thumb: Vec<u8> = vec![0xAB; 40];

    let build = |mdat_start: usize| -> (Vec<u8>, usize) {
        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        let hdlr = fullbox(b"hdlr", 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
        let pitm = fullbox(b"pitm", 0, &1u16.to_be_bytes());
        let infe = |id: u16, typ: &[u8; 4], extra: &[u8]| {
            let mut d = id.to_be_bytes().to_vec();
            d.extend_from_slice(&[0, 0]);
            d.extend_from_slice(typ);
            d.extend_from_slice(b"\0");
            d.extend_from_slice(extra);
            fullbox(b"infe", 2, &d)
        };
        let mut iinf_d = 4u16.to_be_bytes().to_vec();
        iinf_d.extend(infe(1, b"hvc1", b""));
        iinf_d.extend(infe(2, b"Exif", b""));
        iinf_d.extend(infe(3, b"mime", b"application/rdf+xml\0"));
        iinf_d.extend(infe(4, b"hvc1", b""));
        let iinf = fullbox(b"iinf", 0, &iinf_d);
        // iloc v1, offset_size 4, length_size 4, base_offset_size 0, index_size 0.
        let mut iloc_d = vec![0x44, 0x00];
        iloc_d.extend_from_slice(&4u16.to_be_bytes());
        let mut at = mdat_start;
        for (id, len) in [
            (1u16, image.len()),
            (2, exif_payload.len()),
            (3, xmp.len()),
            (4, thumb.len()),
        ] {
            let in_idat = idat && id == 2;
            iloc_d.extend_from_slice(&id.to_be_bytes());
            iloc_d.extend_from_slice(&u16::from(in_idat).to_be_bytes());
            iloc_d.extend_from_slice(&0u16.to_be_bytes());
            iloc_d.extend_from_slice(&1u16.to_be_bytes());
            let off = if in_idat { 0 } else { at };
            iloc_d.extend_from_slice(&(off as u32).to_be_bytes());
            iloc_d.extend_from_slice(&(len as u32).to_be_bytes());
            if !in_idat {
                at += len;
            }
        }
        let iloc = fullbox(b"iloc", 1, &iloc_d);
        let mut iref_d = Vec::new();
        let mut thmb = 4u16.to_be_bytes().to_vec();
        thmb.extend_from_slice(&1u16.to_be_bytes());
        thmb.extend_from_slice(&1u16.to_be_bytes());
        iref_d.extend(bx(b"thmb", &thmb));
        let iref = fullbox(b"iref", 0, &iref_d);
        let ipco = bx(
            b"ipco",
            &[bx(b"irot", &[1]), bx(b"colr", b"prof\x01\x02\x03\x04")].concat(),
        );
        let iprp = bx(b"iprp", &ipco);
        let mut children = vec![hdlr, pitm, iinf, iloc, iref, iprp];
        if idat {
            children.push(bx(b"idat", &exif_payload));
        }
        let meta = fullbox(b"meta", 0, &children.concat());
        let total = ftyp.len() + meta.len();
        ([ftyp, meta].concat(), total)
    };
    let (_, head_len) = build(0);
    let mdat_start = head_len + 8;
    let (mut bytes, _) = build(mdat_start);
    let mut mdat_data = image.clone();
    if !idat {
        mdat_data.extend_from_slice(&exif_payload);
    }
    mdat_data.extend_from_slice(&xmp);
    mdat_data.extend_from_slice(&thumb);
    bytes.extend(bx(b"mdat", &mdat_data));
    let image_r = mdat_start..mdat_start + image.len();
    let (exif_r, xmp_start) = if idat {
        let at = bytes
            .windows(exif_payload.len())
            .position(|w| w == &exif_payload[..])
            .unwrap();
        (at..at + exif_payload.len(), image_r.end)
    } else {
        (
            image_r.end..image_r.end + exif_payload.len(),
            image_r.end + exif_payload.len(),
        )
    };
    let xmp_r = xmp_start..xmp_start + xmp.len();
    Heic {
        bytes,
        exif: exif_r,
        xmp: xmp_r,
        image: image_r,
    }
}

/// Positions where two equal length buffers differ.
pub(crate) fn diff_positions(a: &[u8], b: &[u8]) -> Vec<usize> {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect()
}

/// A tiny deterministic random number generator for fuzz tests.
pub(crate) struct Lcg(pub u64);

impl Lcg {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}
