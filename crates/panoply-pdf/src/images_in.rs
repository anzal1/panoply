//! Images to PDF.
//!
//! A JPEG is stored in the PDF exactly as it is, byte for byte, so the PDF is the JPEG bytes plus
//! a few hundred bytes of structure and the picture is never recompressed. A PNG that PDF can
//! carry directly (8-bit gray, RGB or palette, not interlaced) is stored by reusing its own
//! compressed data. Other PNGs (alpha, 16-bit, interlaced) are decoded and stored with a
//! separate soft mask for the alpha. JPEG orientation (EXIF) is applied with the page matrix, so
//! no pixel is touched.

use std::io::{Cursor, Write};

use panoply_core::CancelToken;

use crate::compress::image::png_filter;
use crate::error::{PdfError, Result};
use crate::filters;
use crate::jpeg::{self, JpegInfo};
use crate::obj::{Dict, Obj};
use crate::writer::{PdfWriter, WriterOptions};

const MM: f64 = 72.0 / 25.4;

/// Page size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    /// A4 or Letter by the user's region.
    Auto,
    /// A4.
    A4,
    /// US Letter.
    Letter,
    /// Each page is as big as its picture at the picture's own resolution.
    Image,
}

/// Settings for [`images_to_pdf`].
#[derive(Debug, Clone, Copy)]
pub struct ImageOptions {
    /// Page size.
    pub page: PageSize,
    /// Margin in millimetres on paper pages (ignored for [`PageSize::Image`]).
    pub margin_mm: f64,
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            page: PageSize::Auto,
            margin_mm: 10.0,
        }
    }
}

/// What was written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageReport {
    /// Pages.
    pub pages: usize,
    /// JPEGs stored byte for byte.
    pub jpeg_passthrough: usize,
    /// PNGs whose compressed data was reused.
    pub png_passthrough: usize,
    /// Images that had to be decoded and stored again.
    pub reencoded: usize,
    /// Bytes of the PDF.
    pub bytes: u64,
}

/// A4 or Letter from a locale name such as `en_US.UTF-8`.
pub fn paper_for_locale(locale: &str) -> PageSize {
    let region = locale
        .split(['.', '@'])
        .next()
        .and_then(|l| l.split('_').nth(1))
        .unwrap_or("");
    match region {
        "US" | "CA" | "MX" | "PH" | "CL" | "CO" | "VE" | "CR" | "GT" | "PA" | "DO" => {
            PageSize::Letter
        }
        _ => PageSize::A4,
    }
}

fn paper_points(p: PageSize) -> (f64, f64) {
    match p {
        PageSize::Letter => (612.0, 792.0),
        _ => (595.2756, 841.8898),
    }
}

/// What a decoded picture looks like to the page builder.
struct Picture {
    /// Pixel size as stored.
    w: usize,
    h: usize,
    /// EXIF orientation 1 to 8.
    orientation: u8,
    /// Resolution the file claims, pixels per inch.
    dpi: Option<f64>,
}

struct Placed {
    pic: Picture,
    /// Objects written for it: image XObject number.
    image: u32,
}

/// Write the images as a PDF, one page each. `load(i)` returns the name and bytes of image `i`;
/// only one image is held at a time.
pub fn images_to_pdf<W: Write>(
    out: W,
    count: usize,
    load: &mut dyn FnMut(usize) -> Result<(String, Vec<u8>)>,
    opts: &ImageOptions,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<ImageReport> {
    if count == 0 {
        return Err(PdfError::BadInput(
            "There are no images to put in the PDF.".into(),
        ));
    }
    let mut w = PdfWriter::new(out, WriterOptions::default())?;
    let catalog = w.alloc();
    let pages_root = w.alloc();
    let mut report = ImageReport::default();
    let mut page_ids = Vec::with_capacity(count);
    let paper = match opts.page {
        PageSize::Auto => paper_for_locale(
            &["LC_ALL", "LC_PAPER", "LANG"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
                .unwrap_or_default(),
        ),
        other => other,
    };
    for i in 0..count {
        if cancel.is_cancelled() {
            return Err(PdfError::Cancelled);
        }
        let (name, bytes) = load(i)?;
        let placed = write_image(&mut w, &name, &bytes, &mut report)?;
        drop(bytes);
        let page = w.alloc();
        let content = w.alloc();
        let (box_w, box_h, x, y, media) = layout(&placed.pic, paper, opts);
        let m = matrix(placed.pic.orientation, box_w, box_h, x, y);
        let body = format!(
            "q {} {} {} {} {} {} cm /Im0 Do Q",
            fmt(m[0]),
            fmt(m[1]),
            fmt(m[2]),
            fmt(m[3]),
            fmt(m[4]),
            fmt(m[5])
        );
        w.put_stream(content, &Dict::new(), body.as_bytes())?;
        let mut res = Dict::new();
        let mut xo = Dict::new();
        xo.set("Im0", Obj::r(placed.image));
        res.set("XObject", Obj::Dict(xo));
        let mut d = Dict::new();
        d.set("Type", Obj::name("Page"));
        d.set("Parent", Obj::r(pages_root));
        d.set(
            "MediaBox",
            Obj::Array(vec![
                Obj::Int(0),
                Obj::Int(0),
                Obj::Real(media.0),
                Obj::Real(media.1),
            ]),
        );
        d.set("Resources", Obj::Dict(res));
        d.set("Contents", Obj::r(content));
        w.put(page, &Obj::Dict(d))?;
        page_ids.push(page);
        report.pages += 1;
        progress((i + 1) as f32 / count as f32 * 0.95, "Adding images");
    }
    let mut p = Dict::new();
    p.set("Type", Obj::name("Pages"));
    p.set(
        "Kids",
        Obj::Array(page_ids.iter().map(|&n| Obj::r(n)).collect()),
    );
    p.set("Count", Obj::Int(page_ids.len() as i64));
    w.put(pages_root, &Obj::Dict(p))?;
    let mut c = Dict::new();
    c.set("Type", Obj::name("Catalog"));
    c.set("Pages", Obj::r(pages_root));
    w.put(catalog, &Obj::Dict(c))?;
    report.bytes = w.finish(catalog, None, format!("images:{count}").as_bytes())?;
    progress(1.0, "Done");
    Ok(report)
}

fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Displayed size of the picture in pixels after orientation.
fn display_px(p: &Picture) -> (f64, f64) {
    if p.orientation >= 5 {
        (p.h as f64, p.w as f64)
    } else {
        (p.w as f64, p.h as f64)
    }
}

/// The box the picture is drawn into, its origin, and the page size.
fn layout(p: &Picture, paper: PageSize, opts: &ImageOptions) -> (f64, f64, f64, f64, (f64, f64)) {
    let (dw, dh) = display_px(p);
    match opts.page {
        PageSize::Image => {
            // 72 is what most cameras write without meaning it: use a screen-like 150 then.
            let dpi = p
                .dpi
                .filter(|d| (96.0..=1200.0).contains(d))
                .unwrap_or(150.0);
            let (w, h) = (dw / dpi * 72.0, dh / dpi * 72.0);
            (w, h, 0.0, 0.0, (w, h))
        }
        _ => {
            let (pw, ph) = paper_points(paper);
            // The page turns to suit the picture.
            let (pw, ph) = if dw > dh { (ph, pw) } else { (pw, ph) };
            let m = opts.margin_mm.max(0.0) * MM;
            let (aw, ah) = ((pw - 2.0 * m).max(10.0), (ph - 2.0 * m).max(10.0));
            let s = (aw / dw).min(ah / dh);
            let (w, h) = (dw * s, dh * s);
            (w, h, (pw - w) / 2.0, (ph - h) / 2.0, (pw, ph))
        }
    }
}

/// The matrix that draws an image stored with EXIF orientation `o` into the box.
fn matrix(o: u8, w: f64, h: f64, ox: f64, oy: f64) -> [f64; 6] {
    match o {
        2 => [-w, 0.0, 0.0, h, ox + w, oy],
        3 => [-w, 0.0, 0.0, -h, ox + w, oy + h],
        4 => [w, 0.0, 0.0, -h, ox, oy + h],
        5 => [0.0, -h, -w, 0.0, ox + w, oy + h],
        6 => [0.0, -h, w, 0.0, ox, oy + h],
        7 => [0.0, h, w, 0.0, ox, oy],
        8 => [0.0, h, -w, 0.0, ox + w, oy],
        _ => [w, 0.0, 0.0, h, ox, oy],
    }
}

fn unsupported(name: &str) -> PdfError {
    PdfError::Unsupported(format!(
        "{name} is not a picture format Panoply can put in a PDF yet. Convert it to JPG or PNG first."
    ))
}

fn write_image<W: Write>(
    w: &mut PdfWriter<W>,
    name: &str,
    b: &[u8],
    rep: &mut ImageReport,
) -> Result<Placed> {
    if b.starts_with(&[0xFF, 0xD8]) {
        write_jpeg(w, name, b, rep)
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        write_png(w, name, b, rep)
    } else {
        Err(unsupported(name))
    }
}

fn icc_from_jpeg(b: &[u8]) -> Option<Vec<u8>> {
    // APP2 segments "ICC_PROFILE\0", sequence number, count, data.
    let mut parts: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut i = 2;
    while i + 4 <= b.len() && b[i] == 0xFF {
        let m = b[i + 1];
        if m == 0xDA || m == 0xD9 {
            break;
        }
        let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
        if len < 2 || i + 2 + len > b.len() {
            break;
        }
        let seg = &b[i + 4..i + 2 + len];
        if m == 0xE2 && seg.starts_with(b"ICC_PROFILE\0") && seg.len() > 14 {
            parts.push((seg[12], seg[14..].to_vec()));
        }
        i += 2 + len;
    }
    if parts.is_empty() {
        return None;
    }
    parts.sort_by_key(|p| p.0);
    Some(parts.into_iter().flat_map(|p| p.1).collect())
}

fn write_icc<W: Write>(w: &mut PdfWriter<W>, profile: &[u8], n: usize) -> Result<Obj> {
    let id = w.alloc();
    let mut d = Dict::new();
    d.set("N", Obj::Int(n as i64));
    d.set("Filter", Obj::name("FlateDecode"));
    w.put_stream(id, &d, &filters::deflate(profile, 6))?;
    Ok(Obj::Array(vec![Obj::name("ICCBased"), Obj::r(id)]))
}

/// True when the picture data is followed by an end-of-image marker. A JPEG cut short still has a
/// readable header, so storing its bytes as they are would write a page with half a picture or
/// none.
fn jpeg_is_complete(b: &[u8]) -> bool {
    let Some(sos) = b.windows(2).position(|w| w == [0xFF, 0xDA]) else {
        return false;
    };
    b[sos..].windows(2).any(|w| w == [0xFF, 0xD9])
}

fn write_jpeg<W: Write>(
    w: &mut PdfWriter<W>,
    name: &str,
    b: &[u8],
    rep: &mut ImageReport,
) -> Result<Placed> {
    let info: JpegInfo = jpeg::parse_info(b).ok_or_else(|| {
        PdfError::Unsupported(format!(
            "{name} is a JPEG that could not be read. It may be damaged."
        ))
    })?;
    if !jpeg_is_complete(b) {
        return Err(PdfError::BadInput(format!(
            "{name} is an incomplete JPEG: the file ends before the picture does, so the page would be blank. Download or export it again from the original."
        )));
    }
    if info.precision != 8 {
        return Err(PdfError::Unsupported(format!(
            "{name} is a {}-bit JPEG, which PDF cannot hold. Convert it to a normal JPG first.",
            info.precision
        )));
    }
    let n = usize::from(info.components);
    let (cs, decode): (Obj, Option<Vec<Obj>>) = match n {
        1 => (Obj::name("DeviceGray"), None),
        3 => (Obj::name("DeviceRGB"), None),
        4 => (
            Obj::name("DeviceCMYK"),
            // Adobe writes CMYK inverted.
            info.adobe_transform.is_some().then(|| {
                [1, 0, 1, 0, 1, 0, 1, 0]
                    .iter()
                    .map(|&v| Obj::Int(v))
                    .collect()
            }),
        ),
        _ => return Err(unsupported(name)),
    };
    let cs = match icc_from_jpeg(b) {
        Some(p) if n != 4 && p.len() > 128 => write_icc(w, &p, n).unwrap_or(cs),
        _ => cs,
    };
    let id = w.alloc();
    let mut d = Dict::new();
    d.set("Type", Obj::name("XObject"));
    d.set("Subtype", Obj::name("Image"));
    d.set("Width", Obj::Int(i64::from(info.width)));
    d.set("Height", Obj::Int(i64::from(info.height)));
    d.set("ColorSpace", cs);
    d.set("BitsPerComponent", Obj::Int(8));
    d.set("Filter", Obj::name("DCTDecode"));
    if let Some(dec) = decode {
        d.set("Decode", Obj::Array(dec));
    }
    w.put_stream(id, &d, b)?;
    rep.jpeg_passthrough += 1;
    Ok(Placed {
        pic: Picture {
            w: info.width as usize,
            h: info.height as usize,
            orientation: info.orientation.unwrap_or(1),
            dpi: info.dpi.map(|(x, y)| x.min(y)),
        },
        image: id,
    })
}

/// The concatenated data of all IDAT chunks, and the palette and transparency chunks.
struct PngChunks {
    idat: Vec<u8>,
    plte: Option<Vec<u8>>,
    trns: Option<Vec<u8>>,
}

fn png_chunks(b: &[u8]) -> Option<PngChunks> {
    let mut i = 8;
    let mut out = PngChunks {
        idat: Vec::new(),
        plte: None,
        trns: None,
    };
    while i + 12 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let ty = &b[i + 4..i + 8];
        let data = b.get(i + 8..i + 8 + len)?;
        match ty {
            b"IDAT" => out.idat.extend_from_slice(data),
            b"PLTE" => out.plte = Some(data.to_vec()),
            b"tRNS" => out.trns = Some(data.to_vec()),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    (!out.idat.is_empty()).then_some(out)
}

fn write_png<W: Write>(
    w: &mut PdfWriter<W>,
    name: &str,
    b: &[u8],
    rep: &mut ImageReport,
) -> Result<Placed> {
    let bad = |_| {
        PdfError::Unsupported(format!(
            "{name} is a PNG that could not be read. It may be damaged."
        ))
    };
    let mut dec = png::Decoder::new(Cursor::new(b));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(bad)?;
    let (width, height, ct, bd, interlaced) = {
        let i = reader.info();
        (
            i.width as usize,
            i.height as usize,
            i.color_type,
            i.bit_depth,
            i.interlaced,
        )
    };
    let dpi = reader.info().pixel_dims.and_then(|p| {
        (p.unit == png::Unit::Meter && p.xppu > 0).then(|| f64::from(p.xppu) * 0.0254)
    });
    let icc = reader.info().icc_profile.as_ref().map(|p| p.to_vec());
    let pic = Picture {
        w: width,
        h: height,
        orientation: 1,
        dpi,
    };
    let trns_present = reader.info().trns.is_some();
    // Reuse the file's own compressed rows when PDF can carry them as they are.
    let direct = !interlaced
        && matches!(
            (ct, bd),
            (png::ColorType::Grayscale, png::BitDepth::Eight)
                | (png::ColorType::Rgb, png::BitDepth::Eight)
                | (png::ColorType::Indexed, _)
        )
        && (!trns_present || ct == png::ColorType::Indexed);
    if direct && let Some(ch) = png_chunks(b) {
        let n = match ct {
            png::ColorType::Rgb => 3,
            _ => 1,
        };
        let bits = match bd {
            png::BitDepth::One => 1,
            png::BitDepth::Two => 2,
            png::BitDepth::Four => 4,
            _ => 8,
        };
        let mut d = Dict::new();
        d.set("Type", Obj::name("XObject"));
        d.set("Subtype", Obj::name("Image"));
        d.set("Width", Obj::Int(width as i64));
        d.set("Height", Obj::Int(height as i64));
        d.set("BitsPerComponent", Obj::Int(bits));
        let mut parts = Dict::new();
        parts.set("Predictor", Obj::Int(15));
        parts.set("Colors", Obj::Int(n));
        parts.set("BitsPerComponent", Obj::Int(bits));
        parts.set("Columns", Obj::Int(width as i64));
        d.set("Filter", Obj::name("FlateDecode"));
        d.set("DecodeParms", Obj::Dict(parts));
        let cs = match (ct, ch.plte) {
            (png::ColorType::Indexed, Some(pal)) => {
                let hival = (pal.len() / 3).saturating_sub(1) as i64;
                // A palette index that is fully transparent becomes a colour key.
                if let Some(t) = &ch.trns {
                    let transparent: Vec<usize> = t
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| **a == 0)
                        .map(|(i, _)| i)
                        .collect();
                    let partial = t.iter().any(|a| *a != 0 && *a != 255);
                    if partial {
                        return write_decoded(w, name, &mut reader, pic, rep);
                    }
                    if let [only] = transparent[..] {
                        d.set(
                            "Mask",
                            Obj::Array(vec![Obj::Int(only as i64), Obj::Int(only as i64)]),
                        );
                    } else if !transparent.is_empty() {
                        return write_decoded(w, name, &mut reader, pic, rep);
                    }
                }
                Obj::Array(vec![
                    Obj::name("Indexed"),
                    Obj::name("DeviceRGB"),
                    Obj::Int(hival),
                    Obj::Str(pal),
                ])
            }
            (png::ColorType::Indexed, None) => return Err(unsupported(name)),
            (png::ColorType::Rgb, _) => match &icc {
                Some(p) => write_icc(w, p, 3).unwrap_or(Obj::name("DeviceRGB")),
                None => Obj::name("DeviceRGB"),
            },
            _ => match &icc {
                Some(p) => write_icc(w, p, 1).unwrap_or(Obj::name("DeviceGray")),
                None => Obj::name("DeviceGray"),
            },
        };
        d.set("ColorSpace", cs);
        let id = w.alloc();
        w.put_stream(id, &d, &ch.idat)?;
        rep.png_passthrough += 1;
        return Ok(Placed { pic, image: id });
    }
    write_decoded(w, name, &mut reader, pic, rep)
}

/// Decode a PNG to 8-bit samples and store them, with a soft mask when there is alpha.
fn write_decoded<W: Write>(
    w: &mut PdfWriter<W>,
    name: &str,
    reader: &mut png::Reader<Cursor<&[u8]>>,
    pic: Picture,
    rep: &mut ImageReport,
) -> Result<Placed> {
    let bad = |_| {
        PdfError::Unsupported(format!(
            "{name} is a PNG that could not be read. It may be damaged."
        ))
    };
    if pic.w * pic.h > 120_000_000 {
        return Err(PdfError::Unsupported(format!(
            "{name} is too large to convert."
        )));
    }
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or_else(|| unsupported(name))?
    ];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(frame.buffer_size());
    let (ch, alpha) = match frame.color_type {
        png::ColorType::Grayscale => (1, false),
        png::ColorType::GrayscaleAlpha => (1, true),
        png::ColorType::Rgb => (3, false),
        png::ColorType::Rgba => (3, true),
        png::ColorType::Indexed => return Err(unsupported(name)),
    };
    let stride = ch + usize::from(alpha);
    let n = pic.w * pic.h;
    let mut colour = Vec::with_capacity(n * ch);
    let mut mask: Vec<u8> = Vec::new();
    let mut translucent = false;
    if alpha {
        mask.reserve(n);
        for px in buf.chunks_exact(stride) {
            colour.extend_from_slice(&px[..ch]);
            mask.push(px[ch]);
            translucent |= px[ch] != 255;
        }
    } else {
        colour = buf;
    }
    let pack = |samples: &[u8], c: usize| -> Vec<u8> {
        let f = png_filter(samples, pic.w * c, c);
        filters::deflate(&f, 6)
    };
    let mut d = Dict::new();
    d.set("Type", Obj::name("XObject"));
    d.set("Subtype", Obj::name("Image"));
    d.set("Width", Obj::Int(pic.w as i64));
    d.set("Height", Obj::Int(pic.h as i64));
    d.set(
        "ColorSpace",
        Obj::name(if ch == 3 { "DeviceRGB" } else { "DeviceGray" }),
    );
    d.set("BitsPerComponent", Obj::Int(8));
    let predictor = |c: usize| {
        let mut p = Dict::new();
        p.set("Predictor", Obj::Int(15));
        p.set("Colors", Obj::Int(c as i64));
        p.set("BitsPerComponent", Obj::Int(8));
        p.set("Columns", Obj::Int(pic.w as i64));
        Obj::Dict(p)
    };
    d.set("Filter", Obj::name("FlateDecode"));
    d.set("DecodeParms", predictor(ch));
    if translucent {
        let mid = w.alloc();
        let mut md = Dict::new();
        md.set("Type", Obj::name("XObject"));
        md.set("Subtype", Obj::name("Image"));
        md.set("Width", Obj::Int(pic.w as i64));
        md.set("Height", Obj::Int(pic.h as i64));
        md.set("ColorSpace", Obj::name("DeviceGray"));
        md.set("BitsPerComponent", Obj::Int(8));
        md.set("Filter", Obj::name("FlateDecode"));
        md.set("DecodeParms", predictor(1));
        w.put_stream(mid, &md, &pack(&mask, 1))?;
        d.set("SMask", Obj::r(mid));
    }
    let id = w.alloc();
    w.put_stream(id, &d, &pack(&colour, ch))?;
    rep.reencoded += 1;
    Ok(Placed { pic, image: id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::source::Source;

    fn jpeg_bytes(w: usize, h: usize) -> Vec<u8> {
        let px: Vec<u8> = (0..w * h * 3)
            .map(|i| ((i / 3 % w) * 255 / w) as u8)
            .collect();
        jpeg::encode(
            &px,
            w,
            h,
            3,
            jpeg::EncodeOptions {
                tables: jpeg::Tables::Standard,
                quality: 80,
                progressive: false,
                subsample: true,
            },
        )
        .unwrap()
    }

    fn png_bytes(w: u32, h: u32, ct: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(ct);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(data).unwrap();
        }
        out
    }

    fn build(images: Vec<Vec<u8>>, opts: ImageOptions) -> (Vec<u8>, ImageReport) {
        let mut out = Vec::new();
        let n = images.len();
        let rep = images_to_pdf(
            &mut out,
            n,
            &mut |i| Ok((format!("img{i}"), images[i].clone())),
            &opts,
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap();
        (out, rep)
    }

    #[test]
    fn a_jpeg_is_stored_byte_for_byte() {
        let j = jpeg_bytes(160, 120);
        let (pdf, rep) = build(vec![j.clone()], ImageOptions::default());
        assert_eq!(rep.jpeg_passthrough, 1);
        let doc = Doc::open(Source::from_bytes(pdf.clone())).unwrap();
        let mut found = None;
        for n in doc.used_objects() {
            if let Ok(l) = doc.load(n)
                && let (Obj::Dict(d), Some(raw)) = (&l.obj, l.stream)
                && d.name("Subtype") == Some(b"Image")
            {
                found = Some(doc.raw(raw).unwrap());
            }
        }
        assert_eq!(found.unwrap(), j);
        // Spec bar: the PDF is the picture plus under 2% structure for a normal-sized photo.
        assert!(pdf.len() < j.len() + 1500, "{} vs {}", pdf.len(), j.len());
    }

    #[test]
    fn a_jpeg_cut_short_is_refused_not_written_as_a_blank_page() {
        let j = jpeg_bytes(160, 120);
        let cut = j[..j.len() / 2].to_vec();
        let mut out = Vec::new();
        let err = images_to_pdf(
            &mut out,
            1,
            &mut |_| Ok(("cut.jpg".to_string(), cut.clone())),
            &ImageOptions::default(),
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("incomplete JPEG"), "{err}");
        assert!(jpeg_is_complete(&j));
    }

    #[test]
    fn paper_pages_turn_to_fit_and_exact_pages_follow_the_dpi() {
        let wide = jpeg_bytes(300, 100);
        let tall = jpeg_bytes(100, 300);
        let (pdf, _) = build(
            vec![wide.clone(), tall],
            ImageOptions {
                page: PageSize::A4,
                margin_mm: 10.0,
            },
        );
        let doc = Doc::open(Source::from_bytes(pdf)).unwrap();
        let pages = doc.pages().unwrap();
        let g0 = doc.page_geom(&pages[0]).display_size();
        let g1 = doc.page_geom(&pages[1]).display_size();
        assert!(g0.0 > g0.1, "wide picture, landscape page: {g0:?}");
        assert!(g1.0 < g1.1, "tall picture, portrait page: {g1:?}");
        let (pdf, _) = build(
            vec![wide],
            ImageOptions {
                page: PageSize::Image,
                margin_mm: 0.0,
            },
        );
        let doc = Doc::open(Source::from_bytes(pdf)).unwrap();
        // 300 px at the assumed 150 dpi is 2 inches.
        let g = doc.page_geom(&doc.pages().unwrap()[0]).display_size();
        assert!(
            (g.0 - 144.0).abs() < 0.5 && (g.1 - 48.0).abs() < 0.5,
            "{g:?}"
        );
    }

    #[test]
    fn png_without_alpha_reuses_its_compressed_rows_and_alpha_becomes_a_mask() {
        let rgb: Vec<u8> = (0..40 * 30 * 3).map(|i| (i % 251) as u8).collect();
        let (pdf, rep) = build(
            vec![png_bytes(40, 30, png::ColorType::Rgb, &rgb)],
            ImageOptions::default(),
        );
        assert_eq!(rep.png_passthrough, 1);
        Doc::open(Source::from_bytes(pdf)).unwrap();
        let rgba: Vec<u8> = (0..40 * 30)
            .flat_map(|i| [10u8, 20, 30, (i % 256) as u8])
            .collect();
        let (pdf, rep) = build(
            vec![png_bytes(40, 30, png::ColorType::Rgba, &rgba)],
            ImageOptions::default(),
        );
        assert_eq!(rep.reencoded, 1);
        let doc = Doc::open(Source::from_bytes(pdf)).unwrap();
        let has_mask = doc.used_objects().iter().any(|n| {
            doc.load(*n)
                .ok()
                .and_then(|l| l.obj.as_dict().map(|d| d.has("SMask")))
                .unwrap_or(false)
        });
        assert!(has_mask);
    }

    #[test]
    fn orientation_matrices_map_the_unit_square_into_the_box() {
        // Every orientation keeps the image inside the box [0,w] x [0,h].
        for o in 1..=8u8 {
            let m = matrix(o, 200.0, 100.0, 10.0, 20.0);
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let x = m[0] * u + m[2] * v + m[4];
                let y = m[1] * u + m[3] * v + m[5];
                assert!((10.0 - 1e-9..=210.0 + 1e-9).contains(&x), "o{o} x {x}");
                assert!((20.0 - 1e-9..=120.0 + 1e-9).contains(&y), "o{o} y {y}");
            }
        }
        // 6 is a quarter turn clockwise: the stored top-left corner (0,1) lands at the top right.
        let m = matrix(6, 100.0, 200.0, 0.0, 0.0);
        let (x, y) = (m[2], m[3] + m[5]);
        assert!(
            (x - 100.0).abs() < 1e-9 && (y - 200.0).abs() < 1e-9,
            "{x} {y}"
        );
    }

    #[test]
    fn unknown_formats_get_a_plain_error() {
        let mut out = Vec::new();
        let e = images_to_pdf(
            &mut out,
            1,
            &mut |_| Ok(("scan.heic".into(), b"ftypheic....".to_vec())),
            &ImageOptions::default(),
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("scan.heic") && e.contains("JPG or PNG"), "{e}");
    }

    #[test]
    fn locale_picks_the_paper() {
        assert_eq!(paper_for_locale("en_US.UTF-8"), PageSize::Letter);
        assert_eq!(paper_for_locale("en_IN"), PageSize::A4);
        assert_eq!(paper_for_locale("C"), PageSize::A4);
    }
}
