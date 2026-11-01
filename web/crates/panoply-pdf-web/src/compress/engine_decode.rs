//! Pixels of JPEG 2000 and JBIG2 pictures, decoded by pdfium.
//!
//! The engine has no decoder for either codec of its own (ADR 0002: nothing but permissive
//! code, and the permissive JPEG 2000 and JBIG2 decoders are C that this build does not carry).
//! pdfium decodes both internally, and it is already the engine that draws pages, so the picture
//! is handed to it alone: a one-page PDF that holds nothing but the stream (filters untouched, JBIG2
//! globals inlined) is opened, the one image object on its page is asked for its bitmap, and the
//! samples are copied out. Nothing about the page being compressed reaches pdfium, so the
//! answer cannot depend on how the page draws the picture, and masks, colour spaces and
//! `/Decode` arrays of the original are not applied: they stay in the dictionary and keep their
//! meaning for whoever reads the recoded stream.
//!
//! pdfium is loaded on first use and, like drawing, may download its library once (see
//! `pdfium.rs`). When it cannot be had the caller leaves the picture alone, as before.

use pdfium_render::prelude::*;

use crate::doc::Doc;
use crate::filters::Filter;
use crate::jpeg::Raster;
use crate::obj::{Dict, Obj};

/// Why a picture could not be decoded here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// pdfium could not be found or loaded.
    NoEngine,
    /// pdfium was loaded and could not decode this picture.
    Undecodable,
}

/// A one-page PDF around a single image stream. `image_dict` is the dictionary text without
/// `<<`, `>>` and `/Length`. `globals` becomes object 6 and must be referenced by the caller.
fn single_image_pdf(
    image_dict: &str,
    data: &[u8],
    globals: Option<&[u8]>,
    page: (usize, usize),
) -> Vec<u8> {
    let content = format!("q {} 0 0 {} 0 0 cm /Im Do Q", page.0, page.1);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs: Vec<usize> = Vec::new();
    let mut add = |out: &mut Vec<u8>, num: usize, body: &[u8]| {
        while offs.len() < num {
            offs.push(0);
        }
        offs[num - 1] = out.len();
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    add(&mut out, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    add(&mut out, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    add(
        &mut out,
        3,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> >> >>",
            page.0, page.1
        )
        .as_bytes(),
    );
    add(
        &mut out,
        4,
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .as_bytes(),
    );
    let mut img = format!("<< {image_dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    img.extend_from_slice(data);
    img.extend_from_slice(b"\nendstream");
    add(&mut out, 5, &img);
    if let Some(g) = globals {
        let mut o = format!("<< /Length {} >>\nstream\n", g.len()).into_bytes();
        o.extend_from_slice(g);
        o.extend_from_slice(b"\nendstream");
        add(&mut out, 6, &o);
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offs.len() + 1).as_bytes());
    for o in &offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offs.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Bitmap of the only image on the first page of `pdf`: width, height, format, rows with stride.
fn first_image_bitmap(
    pdf: Vec<u8>,
) -> Result<(usize, usize, PdfBitmapFormat, usize, Vec<u8>), Fail> {
    let engine = crate::pdfium::engine().map_err(|_| Fail::NoEngine)?;
    let _guard = crate::pdfium::lock();
    let doc = engine
        .load_pdf_from_byte_vec(pdf, None)
        .map_err(|_| Fail::Undecodable)?;
    let page = doc.pages().get(0).map_err(|_| Fail::Undecodable)?;
    for obj in page.objects().iter() {
        if let Some(img) = obj.as_image_object() {
            let bmp = img.get_raw_bitmap().map_err(|_| Fail::Undecodable)?;
            let (w, h) = (bmp.width().max(0) as usize, bmp.height().max(0) as usize);
            let fmt = bmp.format().map_err(|_| Fail::Undecodable)?;
            let bytes = bmp.as_raw_bytes();
            if w == 0 || h == 0 || bytes.len() % h != 0 {
                return Err(Fail::Undecodable);
            }
            let stride = bytes.len() / h;
            return Ok((w, h, fmt, stride, bytes));
        }
    }
    Err(Fail::Undecodable)
}

/// How many pixels pdfium decodes for a `w x h` JPEG 2000 picture asked for at `target`. It skips
/// as many resolution levels (each halving both sides) as keep the result at least as large as
/// the target (measured: decoding 2728 x 3564 takes 99 ms whole, 28 ms for any target between a
/// half and a third, 8 ms from a quarter).
pub fn decoded_pixels(w: usize, h: usize, target: Option<(usize, usize)>) -> usize {
    let Some((tw, th)) = target else {
        return w * h;
    };
    let ratio = (w / tw.max(1)).min(h / th.max(1)).max(1);
    let skip = usize::BITS - 1 - ratio.leading_zeros();
    (w >> skip).max(1) * (h >> skip).max(1)
}

/// 8-bit gray or RGB pixels of a JPEG 2000 stream. `w` and `h` are what the dictionary says; a
/// picture that decodes to another size is refused. `cs` is the `/ColorSpace` name when the
/// dictionary has a device space, `None` to let the codestream say.
///
/// `target` asks for the picture at a smaller size. The page is then drawn at that size, which
/// lets pdfium decode the codestream at a reduced resolution (JPEG 2000 stores a pyramid of
/// them) instead of the full one: a scan of 7.7 million pixels shown at 150 dpi needs a quarter
/// of the memory and time, and memory is what a 300 MB budget is spent on. The size returned
/// is then the target, not `w` by `h`.
pub fn decode_jpx(
    raw: &[u8],
    w: usize,
    h: usize,
    cs: Option<&str>,
    target: Option<(usize, usize)>,
) -> Result<Raster, Fail> {
    let mut d = format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /Filter /JPXDecode");
    if let Some(cs) = cs {
        d.push_str(&format!(" /ColorSpace /{cs} /BitsPerComponent 8"));
    }
    let pdf = single_image_pdf(&d, raw, None, target.unwrap_or((100, 100)));
    let (rw, rh, ch, data) = match target {
        None => {
            let (bw, bh, fmt, stride, bytes) = first_image_bitmap(pdf)?;
            if (bw, bh) != (w, h) {
                return Err(Fail::Undecodable);
            }
            let (ch, bpp) = match fmt {
                PdfBitmapFormat::Gray => (1, 1),
                PdfBitmapFormat::BGR => (3, 3),
                PdfBitmapFormat::BGRx => (3, 4),
                // An alpha channel in the codestream: not a picture this path may flatten.
                PdfBitmapFormat::BGRA => return Err(Fail::Undecodable),
            };
            (w, h, ch, unpack(&bytes, w, h, stride, ch, bpp))
        }
        Some((tw, th)) => {
            let (bytes, stride) = draw_page(pdf, tw, th)?;
            let mut data = unpack(&bytes, tw, th, stride, 3, 3);
            // Drawing always gives colour. A gray picture has equal channels everywhere.
            let gray = cs == Some("DeviceGray")
                || (cs.is_none()
                    && data
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .all(|p| p[0] == p[1] && p[1] == p[2]));
            if gray {
                data = data.as_chunks::<3>().0.iter().map(|p| p[0]).collect();
            }
            (tw, th, if gray { 1 } else { 3 }, data)
        }
    };
    Ok(Raster {
        w: rw,
        h: rh,
        ch,
        data,
    })
}

/// Copy `w x h` pixels out of rows of `stride` bytes, BGR order to RGB.
fn unpack(bytes: &[u8], w: usize, h: usize, stride: usize, ch: usize, bpp: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(w * h * ch);
    for y in 0..h {
        let row = &bytes[y * stride..y * stride + w * bpp];
        if ch == 1 {
            data.extend_from_slice(row);
        } else {
            for px in row.chunks_exact(bpp) {
                data.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
    }
    data
}

/// The first page of `pdf` drawn into `w x h` pixels, as BGR rows with their stride.
fn draw_page(pdf: Vec<u8>, w: usize, h: usize) -> Result<(Vec<u8>, usize), Fail> {
    let engine = crate::pdfium::engine().map_err(|_| Fail::NoEngine)?;
    let _guard = crate::pdfium::lock();
    let doc = engine
        .load_pdf_from_byte_vec(pdf, None)
        .map_err(|_| Fail::Undecodable)?;
    let page = doc.pages().get(0).map_err(|_| Fail::Undecodable)?;
    let mut bitmap = PdfBitmap::empty(w as Pixels, h as Pixels, PdfBitmapFormat::BGR)
        .map_err(|_| Fail::Undecodable)?;
    // pdfium-render asks for reversed byte order by default, which would hand back RGB in a bitmap
    // called BGR. Turned off, the rows are BGR as `unpack` expects.
    let cfg = PdfRenderConfig::new()
        .set_fixed_size(w as Pixels, h as Pixels)
        .set_format(PdfBitmapFormat::BGR)
        .set_reverse_byte_order(false)
        .clear_before_rendering(true)
        .set_clear_color(PdfColor::WHITE)
        .render_form_data(false);
    page.render_into_bitmap_with_config(&mut bitmap, &cfg)
        .map_err(|_| Fail::Undecodable)?;
    let bytes = bitmap.as_raw_bytes();
    if bytes.len() % h != 0 || bytes.len() / h < w * 3 {
        return Err(Fail::Undecodable);
    }
    let stride = bytes.len() / h;
    Ok((bytes, stride))
}

/// Samples of a JBIG2 stream as the picture stores them: packed 8 to a byte, most significant bit
/// first, rows padded to a byte, a sample of 1 for a white pixel. `dict` is the image dictionary,
/// for the filter parameters (the shared globals stream).
pub fn decode_jbig2(
    doc: &Doc,
    dict: &Dict,
    raw: &[u8],
    w: usize,
    h: usize,
) -> Result<Vec<u8>, Fail> {
    // The shared symbol and pattern data, inlined decoded, so the one-page file stands alone.
    let globals = doc
        .filters(dict)
        .into_iter()
        .find(|(f, _)| *f == Filter::Jbig2)
        .and_then(|(_, parms)| match parms.get("JBIG2Globals") {
            Some(Obj::Ref(n, _)) => Some(*n),
            _ => None,
        })
        .map(|n| {
            let l = doc.load(n).map_err(|_| Fail::Undecodable)?;
            let (Obj::Dict(gd), Some(gs)) = (l.obj, l.stream) else {
                return Err(Fail::Undecodable);
            };
            doc.decode(&gd, gs, crate::doc::DECODE_LIMIT, false)
                .map_err(|_| Fail::Undecodable)
        })
        .transpose()?;
    // Drawn as plain 1-bit gray so that no `/ImageMask` or `/Decode` of the original turns the
    // samples around: what comes back is what the filter produced.
    let mut d = format!(
        "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /JBIG2Decode"
    );
    if globals.is_some() {
        d.push_str(" /DecodeParms << /JBIG2Globals 6 0 R >>");
    }
    let (bw, bh, fmt, stride, bytes) =
        first_image_bitmap(single_image_pdf(&d, raw, globals.as_deref(), (100, 100)))?;
    if (bw, bh) != (w, h) || fmt != PdfBitmapFormat::Gray {
        return Err(Fail::Undecodable);
    }
    let rb = w.div_ceil(8);
    let mut out = vec![0u8; rb * h];
    for y in 0..h {
        let row = &bytes[y * stride..y * stride + w];
        let dst = &mut out[y * rb..(y + 1) * rb];
        for (x, &v) in row.iter().enumerate() {
            if v >= 128 {
                dst[x >> 3] |= 0x80 >> (x & 7);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_RGB: &str = "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8";

    /// The page and image bitmaps agree on colour order and cover the size asked for.
    #[test]
    fn both_ways_of_asking_pdfium_give_rgb_in_the_right_order() {
        let px: Vec<u8> = [200u8, 100, 50].repeat(64);
        // The picture itself.
        let pdf = single_image_pdf(RAW_RGB, &px, None, (100, 100));
        let (w, h, fmt, stride, bytes) = match first_image_bitmap(pdf) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skipped: {e:?}");
                return; // no pdfium here
            }
        };
        assert_eq!((w, h), (8, 8));
        let bpp = if fmt == PdfBitmapFormat::BGR { 3 } else { 4 };
        let img = unpack(&bytes, w, h, stride, 3, bpp);
        assert!(
            img.as_chunks::<3>().0.iter().all(|p| *p == [200, 100, 50]),
            "{:?}",
            &img[..3]
        );
        // The page drawn into 64 x 48 pixels.
        let pdf = single_image_pdf(RAW_RGB, &px, None, (64, 48));
        let (bytes, stride) = draw_page(pdf, 64, 48).expect("draw");
        let img = unpack(&bytes, 64, 48, stride, 3, 3);
        let same = img
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|p| **p == [200, 100, 50])
            .count();
        assert!(same > 64 * 48 * 95 / 100, "{same} of {}", 64 * 48);
    }

    /// A JPEG 2000 picture from the corpus decodes to the same colours at full size and when
    /// pdfium is asked for a reduced one.
    #[test]
    fn a_jpx_picture_decodes_alike_full_and_reduced() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/cache/pdf/commons/ia-scan-mining-claims.pdf");
        let Ok(file) = std::fs::File::open(path) else {
            return; // corpus not fetched
        };
        let opened = crate::open(&file).expect("open");
        let doc = &opened.doc;
        let mut tried = 0;
        for n in doc.used_objects() {
            let Ok(l) = doc.load(n) else { continue };
            let (Obj::Dict(d), Some(raw)) = (l.obj, l.stream) else {
                continue;
            };
            if d.name("Subtype") != Some(b"Image")
                || !matches!(doc.filters(&d).as_slice(), [(Filter::Jpx, _)])
                || raw.len < 20_000
            {
                continue;
            }
            let (w, h) = (
                d.int("Width").unwrap() as usize,
                d.int("Height").unwrap() as usize,
            );
            let bytes = doc.raw(raw).unwrap();
            let full = match decode_jpx(&bytes, w, h, Some("DeviceRGB"), None) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("skipped: {e:?}");
                    return;
                }
            };
            let small = decode_jpx(&bytes, w, h, Some("DeviceRGB"), Some((w / 4, h / 4))).unwrap();
            assert_eq!((small.w, small.h, small.ch), (w / 4, h / 4, 3));
            let mean = |r: &Raster| -> [f64; 3] {
                let mut s = [0f64; 3];
                for p in r.data.as_chunks::<3>().0 {
                    for c in 0..3 {
                        s[c] += f64::from(p[c]);
                    }
                }
                s.map(|v| v / (r.w * r.h) as f64)
            };
            let (a, b) = (mean(&full), mean(&small));
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 3.0, "channel {c}: {a:?} vs {b:?}");
            }
            tried += 1;
            if tried == 2 {
                break;
            }
        }
        assert!(tried > 0, "no JPX picture found");
    }

    #[test]
    fn the_decoded_size_follows_the_level_skipping() {
        assert_eq!(decoded_pixels(2728, 3564, None), 2728 * 3564);
        assert_eq!(decoded_pixels(2728, 3564, Some((1169, 1527))), 1364 * 1782);
        assert_eq!(decoded_pixels(2728, 3564, Some((682, 891))), 682 * 891);
        assert_eq!(decoded_pixels(4960, 7016, Some((1240, 1754))), 1240 * 1754);
        assert_eq!(decoded_pixels(100, 100, Some((100, 100))), 100 * 100);
    }
}
