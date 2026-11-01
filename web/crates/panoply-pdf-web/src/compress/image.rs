//! Recoding one image stream: shrink it to the resolution the page shows, store it as a JPEG at
//! the lowest quality that still scores an SSIM of at least the floor against the original, or
//! store it again losslessly with better predictors. Whatever is not smaller is left alone.
//!
//! Pictures are recoded by what they hold, whatever codec stored them:
//!
//! - gray and colour pictures (raw, Flate, JPEG, and JPEG 2000 decoded by pdfium) become a JPEG;
//! - 1-bit pictures and stencil masks (raw, Flate and JBIG2 decoded by pdfium) become CCITT
//!   Group 4 when that is at least 5% smaller than what is stored, and are otherwise left as they
//!   are. They are never shrunk: a stencil has no grays to average.
//!
//! The SSIM check compares luma of the result with luma of the original brought to the same
//! pixel size by area averaging, so it measures what the encoder took away and not the shrink
//! the page does not need. How the finished file looks overall is checked separately by drawing
//! both PDFs and comparing the pages (see `render::compare`).

use super::engine_decode::{self, Fail};
use super::g4;
use crate::doc::{Doc, RawStream};
use crate::error::Result;
use crate::filters::{self, Filter};
use crate::jpeg::{self, EncodeOptions, Raster};
use crate::obj::{Dict, Obj};
use crate::resample;
use crate::ssim::{luma, ssim_gray, ssim_gray_masked};

/// An image must save at least this share of its stored size to be replaced.
const MIN_GAIN: f64 = 0.05;
/// Stored size below which an image is not worth the work.
const MIN_BYTES: u64 = 2048;
/// Largest raster held in memory for one image, in bytes.
const MAX_RASTER: usize = 96 * 1024 * 1024;
/// Most pixels pdfium may be asked to decode for one JPEG 2000 picture (see
/// `engine_decode::decoded_pixels`). Measured at about 25 to 35 bytes a pixel while it works (it
/// holds the wavelet planes and the bitmap, and we hold a copy), which keeps a job under 300 MB.
const MAX_ENGINE_PIXELS: usize = 5_000_000;
/// Most pixels pdfium may be asked to decode for one JBIG2 picture (a 600 dpi A4 page is 35
/// million).
const MAX_ENGINE_BITMAP: usize = 48_000_000;
/// The lowest and highest JPEG quality tried.
const Q_MIN: u8 = 30;

/// The qualities tried for such a layer at a scale, the first to decide whether the scale holds
/// and the second to see whether it holds with less.
const Q_LAYER_PROBE: u8 = 40;
const Q_LAYER_LOW: u8 = 20;
/// A JPEG 2000 picture stored in fewer bits per pixel than this (counted at the size it would
/// be recoded to) cannot become a JPEG that is 5% smaller: wavelets code smooth pictures in a
/// fraction of what the discrete cosine transform needs. Measured: the smooth backgrounds of
/// scans cost a JPEG 0.25 bits per pixel or more at the lowest quality.
const JPX_MIN_BPP: f64 = 0.12;
/// How many times smaller than the page resolution a partly shown layer may be stored, tried
/// from the smallest.
const LAYER_DIVISORS: [usize; 5] = [8, 6, 4, 3, 2];
const Q_MAX: u8 = 92;

/// How hard to compress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quality {
    /// The lowest quality whose SSIM is at least the floor.
    Adaptive {
        /// The SSIM floor, for example 0.97.
        floor: f64,
    },
    /// A fixed JPEG quality.
    Fixed(u8),
}

/// One setting of the dial.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    /// Highest resolution kept, in pixels per inch.
    pub dpi_cap: f64,
    /// Quality choice.
    pub quality: Quality,
}

/// Why an image was left as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Skip {
    /// A kind of image the engine does not recode (palette, CMYK, CCITT already, colour keys, ...).
    Kind,
    /// A JPEG 2000 or JBIG2 picture, and pdfium (which decodes them) could not be loaded.
    NoEngine,
    /// Under 2 KB.
    Tiny,
    /// Too many pixels to hold in memory with the budget.
    TooLarge,
    /// The pixels could not be decoded.
    Undecodable,
    /// Looks like a drawing, and lossless recoding saves nothing.
    Graphic,
    /// No setting of the quality dial passes the SSIM floor, or the result is not smaller.
    NoGain,
}

/// What to do with the image.
#[derive(Clone, Copy)]
pub struct Params<'a> {
    /// Dial setting.
    pub level: Level,
    /// Highest resolution the page shows the image at, when known.
    pub eff_dpi: Option<f64>,
    /// The image is the soft mask of another image: lossless only.
    pub is_smask: bool,
    /// Convert colour images to gray.
    pub grayscale: bool,
    /// The document, to resolve references in the image dictionary.
    pub doc: &'a Doc,
}

/// The result for one image.
pub enum Verdict {
    /// Replace the stream with this dictionary and these bytes.
    Recoded {
        /// New stream dictionary (source references, rewritten by the copier).
        dict: Dict,
        /// New stream bytes.
        data: Vec<u8>,
        /// SSIM of the result against the original at the new size, for lossy results.
        ssim: Option<f64>,
        /// True when pixels were lost (JPEG), false when the change is lossless.
        lossy: bool,
    },
    /// Leave the stream as it is.
    Kept(Skip),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cs {
    Gray,
    Rgb,
    Cmyk,
    Other,
}

/// Number of components and kind of a colour space, when known.
fn colour_space(doc: &Doc, cs: &Obj, depth: usize) -> Option<(usize, Cs)> {
    if depth > 4 {
        return None;
    }
    match doc.resolve(cs) {
        Obj::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"G" | b"CalGray" => Some((1, Cs::Gray)),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some((3, Cs::Rgb)),
            b"DeviceCMYK" | b"CMYK" => Some((4, Cs::Cmyk)),
            _ => None,
        },
        Obj::Array(a) => {
            let first = a.first().map(|o| doc.resolve(o))?;
            match first.as_name()? {
                b"CalGray" => Some((1, Cs::Gray)),
                b"CalRGB" => Some((3, Cs::Rgb)),
                b"ICCBased" => {
                    let n = doc.resolve_dict(a.get(1)?)?.int("N")?;
                    match n {
                        1 => Some((1, Cs::Gray)),
                        3 => Some((3, Cs::Rgb)),
                        4 => Some((4, Cs::Cmyk)),
                        _ => None,
                    }
                }
                b"Lab" => Some((3, Cs::Other)),
                b"Indexed" | b"I" | b"Separation" => Some((1, Cs::Other)),
                b"DeviceN" => {
                    let n = doc.resolve_array(a.get(1)?)?.len();
                    (1..=32).contains(&n).then_some((n, Cs::Other))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Recode an image stream.
pub fn recode(p: &Params<'_>, dict: &Dict, raw: RawStream) -> Result<Verdict> {
    match load(p, dict, raw)? {
        Ok(l) => encode(p, dict, raw, &l),
        Err(skip) => Ok(Verdict::Kept(skip)),
    }
}

/// The decoded pixels of an image and what they came from.
pub struct Loaded {
    source: Source,
    kind: Cs,
    pixels: Pixels,
    /// Width in pixels as the dictionary has it. A JPEG 2000 picture shown much smaller than it
    /// is stored is decoded at a reduced size, so this can differ from the raster.
    stored_w: usize,
    /// Where the picture shows, when it sits under a 1-bit stencil or soft mask.
    visible: Option<Visible>,
}

/// Where a picture shows: the stencil mask or 1-bit soft mask it sits under, one bit per mask
/// pixel, 1 where the picture is painted. This is how a scan split into layers stores its colour
/// layer: a picture of ink colour, painted only through a stencil of the glyphs.
struct Visible {
    mw: usize,
    mh: usize,
    /// Rows padded to a byte, most significant bit first.
    bits: Vec<u8>,
}

impl Visible {
    /// The map at `w x h`, one byte per pixel: how much of the pixel is painted, 0 to 255, from
    /// the mask pixels under it.
    fn coverage(&self, w: usize, h: usize) -> Vec<u8> {
        let rb = self.mw.div_ceil(8);
        let mut out = vec![0u8; w * h];
        for y in 0..h {
            let (y0, y1) = (
                y * self.mh / h,
                ((y + 1) * self.mh).div_ceil(h).min(self.mh),
            );
            let y1 = y1.max(y0 + 1).min(self.mh);
            for x in 0..w {
                let (x0, x1) = (
                    x * self.mw / w,
                    ((x + 1) * self.mw).div_ceil(w).min(self.mw),
                );
                let x1 = x1.max(x0 + 1).min(self.mw);
                let mut on = 0usize;
                for my in y0..y1 {
                    let row = &self.bits[my * rb..(my + 1) * rb];
                    for mx in x0..x1 {
                        on += usize::from(row[mx >> 3] & (0x80 >> (mx & 7)) != 0);
                    }
                }
                out[y * w + x] = (on * 255 / ((y1 - y0) * (x1 - x0))) as u8;
            }
        }
        out
    }
}

enum Pixels {
    /// 8-bit gray or RGB.
    Raster(Raster),
    /// 1-bit samples, rows padded to a byte, 1 for white (see `g4`).
    Bits { w: usize, h: usize, data: Vec<u8> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Raw,
    Flate,
    Dct,
    Jpx,
    Jbig2,
}

fn engine_skip(f: Fail) -> Skip {
    match f {
        Fail::NoEngine => Skip::NoEngine,
        Fail::Undecodable => Skip::Undecodable,
    }
}

/// Decode the pixels of an image, once. [`encode`] can then try as many settings as it likes
/// without decoding again (the size estimate tries ten).
pub fn load(
    p: &Params<'_>,
    dict: &Dict,
    raw: RawStream,
) -> Result<std::result::Result<Loaded, Skip>> {
    let doc = p.doc;
    if raw.len < MIN_BYTES {
        return Ok(Err(Skip::Tiny));
    }
    let int = |k: &str| dict.get(k).and_then(|o| doc.resolve_int(o));
    let (Some(w), Some(h)) = (int("Width"), int("Height")) else {
        return Ok(Err(Skip::Kind));
    };
    if w <= 0 || h <= 0 || w > 1 << 20 || h > 1 << 20 {
        return Ok(Err(Skip::Kind));
    }
    let (w, h) = (w as usize, h as usize);
    let image_mask = matches!(
        dict.get("ImageMask").map(|o| doc.resolve(o)),
        Some(Obj::Bool(true))
    );
    let bpc = int("BitsPerComponent").unwrap_or(if image_mask { 1 } else { 8 });
    let chain = doc.filters(dict);
    let source = match chain.as_slice() {
        [] => Source::Raw,
        [(Filter::Flate, _)] => Source::Flate,
        [(Filter::Dct, _)] => Source::Dct,
        [(Filter::Jpx, _)] => Source::Jpx,
        [(Filter::Jbig2, _)] => Source::Jbig2,
        _ => return Ok(Err(Skip::Kind)),
    };
    let cs = dict.get("ColorSpace").and_then(|c| colour_space(doc, c, 0));

    // 1-bit pictures and stencil masks.
    if image_mask || (bpc == 1 && source != Source::Jpx) {
        let plain_gray = matches!(cs, Some((1, Cs::Gray)));
        if !(image_mask || plain_gray) || matches!(source, Source::Dct | Source::Jpx) {
            return Ok(Err(Skip::Kind));
        }
        // A JBIG2 picture is left as it is without decoding it. Measured on the 139 JBIG2
        // pictures of three scanned books, Group 4 was 1.2 to 6 times larger every time (JBIG2
        // codes repeated glyphs once), so there is nothing to win. pdfium still decodes JBIG2 when
        // it is the mask of a colour layer (see `load_visible`).
        if source == Source::Jbig2 {
            return Ok(Err(Skip::NoGain));
        }
        let data = match bits_of(doc, dict, raw, source, w, h)? {
            Ok(d) => d,
            Err(skip) => return Ok(Err(skip)),
        };
        return Ok(Ok(Loaded {
            source,
            kind: Cs::Gray,
            pixels: Pixels::Bits { w, h, data },
            stored_w: w,
            visible: None,
        }));
    }

    if source == Source::Jbig2 {
        return Ok(Err(Skip::Kind));
    }

    // JPEG 2000: decoded by pdfium. Only pictures that may become a JPEG are worth the decode.
    if source == Source::Jpx {
        let cs_name = match dict.get("ColorSpace").map(|c| doc.resolve(c)) {
            None => None,
            Some(Obj::Name(n)) => match n.as_slice() {
                b"DeviceGray" | b"G" => Some("DeviceGray"),
                b"DeviceRGB" | b"RGB" => Some("DeviceRGB"),
                _ => return Ok(Err(Skip::Kind)),
            },
            Some(_) => return Ok(Err(Skip::Kind)),
        };
        if p.is_smask
            || dict.has("Decode")
            || dict.has("SMaskInData")
            || matches!(dict.get("Mask"), Some(Obj::Array(_)))
            || has_matte(doc, dict)
        {
            return Ok(Err(Skip::Kind));
        }
        // Shown much smaller than stored: decode small. The same target the JPEG step would pick.
        let target = shrink_target(p, w, h);
        if engine_decode::decoded_pixels(w, h, target) > MAX_ENGINE_PIXELS {
            return Ok(Err(Skip::TooLarge));
        }
        let visible = load_visible(doc, dict)?;
        let px = target.map_or(w * h, |(a, b)| a * b);
        if visible.is_none()
            && matches!(p.level.quality, Quality::Adaptive { .. })
            && (raw.len as f64 * 8.0) < px as f64 * JPX_MIN_BPP
        {
            return Ok(Err(Skip::NoGain));
        }
        let bytes = doc.raw(raw)?;
        let r = match engine_decode::decode_jpx(&bytes, w, h, cs_name, target) {
            Ok(r) => r,
            Err(f) => return Ok(Err(engine_skip(f))),
        };
        let expect = match cs_name {
            Some("DeviceGray") => Some(1),
            Some(_) => Some(3),
            None => None,
        };
        if expect.is_some_and(|c| c != r.ch) {
            return Ok(Err(Skip::Kind));
        }
        let kind = if r.ch == 1 { Cs::Gray } else { Cs::Rgb };
        return Ok(Ok(Loaded {
            source,
            kind,
            pixels: Pixels::Raster(r),
            stored_w: w,
            visible,
        }));
    }

    if bpc != 8 {
        return Ok(Err(Skip::Kind));
    }
    let Some((ncomp, kind)) = cs else {
        return Ok(Err(Skip::Kind));
    };
    if w * h * ncomp > MAX_RASTER {
        return Ok(Err(Skip::TooLarge));
    }

    // Pixels of the original.
    let raster = match source {
        Source::Dct => {
            let bytes = doc.raw(raw)?;
            let Some(info) = jpeg::parse_info(&bytes) else {
                return Ok(Err(Skip::Undecodable));
            };
            if usize::from(info.components) != ncomp
                || info.width as usize != w
                || info.height as usize != h
            {
                return Ok(Err(Skip::Kind));
            }
            match jpeg::decode(&bytes, &info) {
                Some(r) => r,
                None => return Ok(Err(Skip::Kind)),
            }
        }
        _ => {
            let data = match doc.decode(dict, raw, w * h * ncomp + 4096, false) {
                Ok(d) => d,
                Err(_) => return Ok(Err(Skip::Undecodable)),
            };
            if data.len() != w * h * ncomp {
                return Ok(Err(Skip::Undecodable));
            }
            Raster {
                w,
                h,
                ch: ncomp,
                data,
            }
        }
    };
    Ok(Ok(Loaded {
        source,
        kind,
        pixels: Pixels::Raster(raster),
        stored_w: w,
        visible: load_visible(doc, dict)?,
    }))
}

/// The samples of a 1-bit picture, packed, 1 for white (see `g4`).
fn bits_of(
    doc: &Doc,
    dict: &Dict,
    raw: RawStream,
    source: Source,
    w: usize,
    h: usize,
) -> Result<std::result::Result<Vec<u8>, Skip>> {
    let rb = w.div_ceil(8);
    if rb * h > MAX_RASTER {
        return Ok(Err(Skip::TooLarge));
    }
    Ok(match source {
        Source::Jbig2 => {
            // pdfium holds one byte a pixel, and we hold a copy of it while we pack.
            if w * h > MAX_ENGINE_BITMAP {
                return Ok(Err(Skip::TooLarge));
            }
            let bytes = doc.raw(raw)?;
            engine_decode::decode_jbig2(doc, dict, &bytes, w, h).map_err(engine_skip)
        }
        Source::Raw | Source::Flate => match doc.decode(dict, raw, rb * h + 4096, false) {
            Ok(d) if d.len() == rb * h => Ok(d),
            _ => Err(Skip::Undecodable),
        },
        _ => Err(Skip::Kind),
    })
}

/// Where the picture of `dict` shows, from the 1-bit stencil (`/Mask`) or soft mask (`/SMask`) it
/// names. `None` for anything else: no mask, a colour key, a soft mask with grays.
fn load_visible(doc: &Doc, dict: &Dict) -> Result<Option<Visible>> {
    let (key, stencil) = if matches!(dict.get("Mask"), Some(Obj::Ref(..))) {
        ("Mask", true)
    } else if matches!(dict.get("SMask"), Some(Obj::Ref(..))) {
        ("SMask", false)
    } else {
        return Ok(None);
    };
    let Some(Obj::Ref(n, _)) = dict.get(key) else {
        return Ok(None);
    };
    let Ok(l) = doc.load(*n) else {
        return Ok(None);
    };
    let (Obj::Dict(md), Some(mraw)) = (l.obj, l.stream) else {
        return Ok(None);
    };
    let int = |k: &str| md.get(k).and_then(|o| doc.resolve_int(o));
    let (Some(mw), Some(mh)) = (int("Width"), int("Height")) else {
        return Ok(None);
    };
    if mw <= 0 || mh <= 0 || mw > 1 << 20 || mh > 1 << 20 {
        return Ok(None);
    }
    let (mw, mh) = (mw as usize, mh as usize);
    let is_stencil = matches!(
        md.get("ImageMask").map(|o| doc.resolve(o)),
        Some(Obj::Bool(true))
    );
    let one_bit_gray = int("BitsPerComponent") == Some(1)
        && matches!(
            md.get("ColorSpace").and_then(|c| colour_space(doc, c, 0)),
            Some((1, Cs::Gray))
        );
    if (stencil && !is_stencil) || (!stencil && !one_bit_gray) || md.has("Mask") || md.has("SMask")
    {
        return Ok(None);
    }
    let source = match doc.filters(&md).as_slice() {
        [] => Source::Raw,
        [(Filter::Flate, _)] => Source::Flate,
        [(Filter::Jbig2, _)] => Source::Jbig2,
        _ => return Ok(None),
    };
    let Ok(mut bits) = bits_of(doc, &md, mraw, source, mw, mh)? else {
        return Ok(None);
    };
    // A stencil paints where the sample is 0 (with the default `/Decode [0 1]`); a soft mask is
    // opaque where it is 1. `/Decode [1 0]` turns either around.
    let swapped = md
        .get("Decode")
        .and_then(|d| doc.resolve_array(d))
        .and_then(|a| a.first().and_then(Obj::as_f64))
        .is_some_and(|v| v > 0.5);
    if stencil != swapped {
        bits.iter_mut().for_each(|b| *b = !*b);
    }
    Ok(Some(Visible { mw, mh, bits }))
}

/// The size a picture of `w x h` is shrunk to at this setting, `None` when it keeps its size.
fn shrink_target(p: &Params<'_>, w: usize, h: usize) -> Option<(usize, usize)> {
    let eff = p.eff_dpi?;
    if eff <= p.level.dpi_cap * 1.25 {
        return None;
    }
    let (nw, nh) = resample::scaled_size(w, h, p.level.dpi_cap / eff);
    (nw >= 32 && nh >= 32 && (nw < w || nh < h)).then_some((nw, nh))
}

/// Try to store a decoded image smaller, at the setting in `p`.
pub fn encode(p: &Params<'_>, dict: &Dict, raw: RawStream, loaded: &Loaded) -> Result<Verdict> {
    let doc = p.doc;
    let (source, kind) = (loaded.source, loaded.kind);
    let raster = match &loaded.pixels {
        Pixels::Bits { w, h, data } => return Ok(bilevel(dict, raw, *w, *h, data)),
        Pixels::Raster(r) => r,
    };

    // A picture decoded at a reduced size is shown at proportionally fewer pixels per inch.
    let p = &Params {
        eff_dpi: p
            .eff_dpi
            .map(|d| d * raster.w as f64 / loaded.stored_w as f64),
        ..*p
    };
    let lossy_ok = !p.is_smask
        && matches!(kind, Cs::Gray | Cs::Rgb)
        && !dict.has("Decode")
        // A colour key mask names exact values; a stencil mask is a picture of its own.
        && !matches!(dict.get("Mask"), Some(Obj::Array(_)))
        && !has_matte(doc, dict);
    let coded = matches!(source, Source::Dct | Source::Jpx);
    if lossy_ok {
        let photographic = coded || looks_photographic(raster);
        if photographic && let Some(v) = lossy(p, dict, raw, raster, loaded.visible.as_ref())? {
            return Ok(v);
        }
        if !photographic && coded {
            return Ok(Verdict::Kept(Skip::NoGain));
        }
    }
    // Lossless: only for images stored without a lossy codec.
    if !coded {
        if let Some(v) = lossless(dict, raw, raster) {
            return Ok(v);
        }
        return Ok(Verdict::Kept(if lossy_ok {
            Skip::NoGain
        } else {
            Skip::Graphic
        }));
    }
    Ok(Verdict::Kept(Skip::NoGain))
}

/// A 1-bit picture or stencil mask as CCITT Group 4, when that is clearly smaller than what is
/// stored. Same samples, same dictionary (`/ImageMask`, `/Decode`, colour space), other filter.
fn bilevel(dict: &Dict, raw: RawStream, w: usize, h: usize, bits: &[u8]) -> Verdict {
    let Some(g4) = g4::encode(bits, w, h) else {
        return Verdict::Kept(Skip::NoGain);
    };
    if g4.len() as f64 >= raw.len as f64 * (1.0 - MIN_GAIN) {
        return Verdict::Kept(Skip::NoGain);
    }
    let mut nd = dict.clone();
    nd.set("Filter", Obj::name("CCITTFaxDecode"));
    let mut parms = Dict::new();
    parms.set("K", Obj::Int(-1));
    parms.set("Columns", Obj::Int(w as i64));
    parms.set("Rows", Obj::Int(h as i64));
    nd.set("DecodeParms", Obj::Dict(parms));
    nd.remove("DP");
    Verdict::Recoded {
        dict: nd,
        data: g4,
        ssim: None,
        lossy: false,
    }
}

fn has_matte(doc: &Doc, dict: &Dict) -> bool {
    dict.get("SMask")
        .and_then(|m| doc.resolve_dict(m))
        .is_some_and(|m| m.has("Matte"))
}

/// A quick test of whether an image looks like a photograph (many distinct colours, smooth
/// gradients) rather than a drawing, which JPEG does badly.
fn looks_photographic(r: &Raster) -> bool {
    let n = r.w * r.h;
    let step = (n / 4096).max(1);
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while i < n && seen.len() < 256 {
        let o = i * r.ch;
        let key = if r.ch == 3 {
            // Quantise a little so noise in a flat colour does not count as variety.
            (u32::from(r.data[o] >> 2) << 12)
                | (u32::from(r.data[o + 1] >> 2) << 6)
                | u32::from(r.data[o + 2] >> 2)
        } else {
            u32::from(r.data[o] >> 1)
        };
        seen.insert(key);
        i += step;
    }
    seen.len() >= 48
}

/// Gray conversion first, then the shrink the page allows.
fn prepare<'a>(
    p: &Params<'_>,
    original: &'a Raster,
) -> (std::borrow::Cow<'a, [u8]>, usize, usize, usize) {
    let (mut px, mut ch) = (std::borrow::Cow::Borrowed(&original.data[..]), original.ch);
    if p.grayscale && ch == 3 {
        px = std::borrow::Cow::Owned(luma(&px, 3));
        ch = 1;
    }
    let (mut w, mut h) = (original.w, original.h);
    if let Some((nw, nh)) = shrink_target(p, w, h) {
        let shrunk = resample::shrink(&px, w, h, ch, nw, nh);
        px = std::borrow::Cow::Owned(shrunk);
        (w, h) = (nw, nh);
    }
    (px, w, h, ch)
}

/// The dictionary of a recoded JPEG.
fn jpeg_dict(
    p: &Params<'_>,
    dict: &Dict,
    original_ch: usize,
    w: usize,
    h: usize,
    ch: usize,
) -> Dict {
    let mut nd = dict.clone();
    nd.set("Width", Obj::Int(w as i64));
    nd.set("Height", Obj::Int(h as i64));
    nd.set("BitsPerComponent", Obj::Int(8));
    nd.set("Filter", Obj::name("DCTDecode"));
    nd.remove("DecodeParms");
    nd.remove("DP");
    if p.grayscale && original_ch == 3 {
        nd.set("ColorSpace", Obj::name("DeviceGray"));
    } else if !nd.has("ColorSpace") {
        // A JPEG 2000 stream may leave the space to the codestream; a JPEG may not.
        nd.set(
            "ColorSpace",
            Obj::name(if ch == 1 { "DeviceGray" } else { "DeviceRGB" }),
        );
    }
    nd
}

/// A picture that shows only through a mask, stored smaller than the page resolution.
///
/// The colour layer of a split scan is painted only where the stencil of the glyphs has ink, and
/// its colours are smeared to nothing in between, so judging every pixel of it (and spending
/// bytes on the smear) is wrong twice. The layer is judged where it shows: the picture is
/// stored at 1/8 to 1/2 of the page resolution, the smallest scale whose JPEG, stretched back
/// to the page resolution the way a viewer does, still scores at least `floor` (SSIM, on each
/// colour channel) in every window the mask touches. Then the quality goes as low as that
/// allows. `None` when no scale holds the floor: the ordinary path takes over.
fn lossy_masked(
    p: &Params<'_>,
    dict: &Dict,
    raw: RawStream,
    original: &Raster,
    vis: &Visible,
    floor: f64,
) -> Option<Verdict> {
    let (px, w, h, ch) = prepare(p, original);
    let cover = vis.coverage(w, h);
    let shows: Vec<u8> = cover.iter().map(|&c| u8::from(c > 0)).collect();
    // The layer as it looks on a plain sheet through its mask: each colour channel mixed with the
    // sheet by how much of the pixel the mask paints. Comparing these (rather than the bare
    // layers) is what makes the score mean something, because only the painted share of a layer
    // pixel ever reaches the page. The sheet is white for dark ink and black for light ink, so
    // that the ink always stands out from it.
    let weight: f64 = cover.iter().map(|&c| f64::from(c)).sum::<f64>().max(1.0);
    let ink: f64 = luma(&px, ch)
        .iter()
        .zip(&cover)
        .map(|(&v, &c)| f64::from(v) * f64::from(c))
        .sum::<f64>()
        / weight;
    let sheet: u32 = if ink < 128.0 { 255 } else { 0 };
    let on_paper = |data: &[u8]| -> Vec<Vec<u8>> {
        (0..ch)
            .map(|c| {
                data.iter()
                    .skip(c)
                    .step_by(ch)
                    .zip(&cover)
                    .map(|(&v, &a)| {
                        ((u32::from(v) * u32::from(a) + sheet * (255 - u32::from(a)) + 127) / 255)
                            as u8
                    })
                    .collect()
            })
            .collect()
    };
    let reference = on_paper(&px);
    let opts = |q: u8, n: usize| EncodeOptions {
        tables: jpeg::Tables::Standard,
        quality: q,
        progressive: n >= 65_536,
        subsample: ch == 3 && q < 90,
    };
    // Encode the layer at (sw, sh) and score it, stretched back, where it shows.
    let try_at = |small: &[u8], sw: usize, sh: usize, q: u8| -> Option<(Vec<u8>, f64)> {
        let jpg = jpeg::encode(small, sw, sh, ch, opts(q, sw * sh))?;
        let info = jpeg::parse_info(&jpg)?;
        let back = jpeg::decode(&jpg, &info)?;
        if back.ch != ch {
            return None;
        }
        let up = if (sw, sh) == (w, h) {
            back.data
        } else {
            resample::enlarge(&back.data, sw, sh, ch, w, h)
        };
        let s = on_paper(&up)
            .iter()
            .zip(&reference)
            .map(|(a, b)| ssim_gray_masked(b, a, w, h, &shows))
            .fold(1.0f64, f64::min);
        Some((jpg, s))
    };
    let holds = |s: f64| s >= floor;
    let stored = raw.len as f64 * (1.0 - MIN_GAIN);
    for div in LAYER_DIVISORS {
        let (sw, sh) = (w.div_ceil(div), h.div_ceil(div));
        if sw < 16 || sh < 16 {
            continue;
        }
        let small = resample::shrink(&px, w, h, ch, sw, sh);
        let Some((jpg, s)) = try_at(&small, sw, sh, Q_LAYER_PROBE) else {
            continue;
        };
        if !holds(s) {
            continue;
        }
        // This scale holds. Try for less.
        let (mut best, mut best_s) = (jpg, s);
        if let Some((j, s)) = try_at(&small, sw, sh, Q_LAYER_LOW)
            && holds(s)
        {
            (best, best_s) = (j, s);
        }
        if best.len() as f64 >= stored {
            return Some(Verdict::Kept(Skip::NoGain));
        }
        return Some(Verdict::Recoded {
            dict: jpeg_dict(p, dict, original.ch, sw, sh, ch),
            data: best,
            ssim: Some(best_s),
            lossy: true,
        });
    }
    None
}

fn lossy(
    p: &Params<'_>,
    dict: &Dict,
    raw: RawStream,
    original: &Raster,
    visible: Option<&Visible>,
) -> Result<Option<Verdict>> {
    if let (Some(vis), Quality::Adaptive { floor }) = (visible, p.level.quality)
        && let Some(v) = lossy_masked(p, dict, raw, original, vis, floor)
    {
        return Ok(Some(v));
    }
    let (px, w, h, ch) = prepare(p, original);
    let reference = luma(&px, ch);
    let opts = |q: u8| EncodeOptions {
        tables: jpeg::Tables::Standard,
        quality: q,
        progressive: w * h >= 65_536,
        subsample: ch == 3 && q < 90,
    };
    let q = match p.level.quality {
        Quality::Fixed(q) => q.clamp(1, 100),
        Quality::Adaptive { floor } => {
            let (tile, tw, th) = sample_tiles(&px, w, h, ch);
            let Some(q) = search(&tile, tw, th, ch, floor, &opts) else {
                return Ok(Some(Verdict::Kept(Skip::NoGain)));
            };
            q
        }
    };
    // The whole image at the chosen quality, checked once against the floor.
    let mut q = q;
    let mut tries = 0;
    loop {
        let Some(jpg) = jpeg::encode(&px, w, h, ch, opts(q)) else {
            return Ok(None);
        };
        if jpg.len() as f64 >= raw.len as f64 * (1.0 - MIN_GAIN) {
            return Ok(Some(Verdict::Kept(Skip::NoGain)));
        }
        let Some(info) = jpeg::parse_info(&jpg) else {
            return Ok(None);
        };
        let Some(back) = jpeg::decode(&jpg, &info) else {
            return Ok(None);
        };
        let s = ssim_gray(&reference, &luma(&back.data, ch), w, h);
        let ok = match p.level.quality {
            Quality::Adaptive { floor } => s >= floor,
            Quality::Fixed(_) => true,
        };
        if ok {
            let nd = jpeg_dict(p, dict, original.ch, w, h, ch);
            return Ok(Some(Verdict::Recoded {
                dict: nd,
                data: jpg,
                ssim: Some(s),
                lossy: true,
            }));
        }
        tries += 1;
        if tries > 2 || q >= Q_MAX {
            return Ok(Some(Verdict::Kept(Skip::NoGain)));
        }
        q = (q + 6).min(Q_MAX);
    }
}

/// The lowest quality in [`Q_MIN`, `Q_MAX`] that scores at least `floor`, or `None` when even the
/// top of the range does not.
fn search(
    px: &[u8],
    w: usize,
    h: usize,
    ch: usize,
    floor: f64,
    opts: &dyn Fn(u8) -> EncodeOptions,
) -> Option<u8> {
    let reference = luma(px, ch);
    let ok = |q: u8| -> bool {
        let Some(jpg) = jpeg::encode(px, w, h, ch, opts(q)) else {
            return false;
        };
        let Some(info) = jpeg::parse_info(&jpg) else {
            return false;
        };
        let Some(back) = jpeg::decode(&jpg, &info) else {
            return false;
        };
        ssim_gray(&reference, &luma(&back.data, ch), w, h) >= floor
    };
    if !ok(Q_MAX) {
        return None;
    }
    let (mut lo, mut hi) = (Q_MIN, Q_MAX);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(hi)
}

/// For a big image, the pieces that decide the quality: the most detailed 128 x 128 tiles
/// stacked into one small image. Small images are used whole.
fn sample_tiles(px: &[u8], w: usize, h: usize, ch: usize) -> (Vec<u8>, usize, usize) {
    const T: usize = 128;
    if w * h <= 250_000 || w < 2 * T || h < 2 * T {
        return (px.to_vec(), w, h);
    }
    let (gx, gy) = (w / T, h / T);
    let mut scored: Vec<(f64, usize, usize)> = Vec::new();
    for ty in 0..gy {
        for tx in 0..gx {
            let (x0, y0) = (tx * T, ty * T);
            // Variance of luma on every other pixel.
            let (mut s, mut s2, mut n) = (0f64, 0f64, 0f64);
            for y in (y0..y0 + T).step_by(2) {
                for x in (x0..x0 + T).step_by(2) {
                    let o = (y * w + x) * ch;
                    let v = if ch == 3 {
                        f64::from(px[o]) * 0.3
                            + f64::from(px[o + 1]) * 0.59
                            + f64::from(px[o + 2]) * 0.11
                    } else {
                        f64::from(px[o])
                    };
                    s += v;
                    s2 += v * v;
                    n += 1.0;
                }
            }
            scored.push((s2 / n - (s / n) * (s / n), tx, ty));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(5);
    let mut out = Vec::with_capacity(T * T * ch * scored.len());
    for &(_, tx, ty) in &scored {
        for y in 0..T {
            let o = ((ty * T + y) * w + tx * T) * ch;
            out.extend_from_slice(&px[o..o + T * ch]);
        }
    }
    (out, T, T * scored.len())
}

/// Store the samples again with PNG row filters and the best deflate: identical pixels, usually
/// smaller. Only when it saves at least [`MIN_GAIN`].
fn lossless(dict: &Dict, raw: RawStream, r: &Raster) -> Option<Verdict> {
    // Incompressible data (already noise-like) is not worth a long deflate: try a sample first.
    let sample = &r.data[..r.data.len().min(96 * 1024)];
    if (filters::deflate(sample, 1).len() as f64) > sample.len() as f64 * 0.97 {
        return None;
    }
    let filtered = png_filter(&r.data, r.w * r.ch, r.ch);
    let z = filters::deflate(&filtered, if filtered.len() > 8 << 20 { 6 } else { 9 });
    if z.len() as f64 >= raw.len as f64 * (1.0 - MIN_GAIN) {
        return None;
    }
    let mut nd = dict.clone();
    nd.set("Filter", Obj::name("FlateDecode"));
    let mut parms = Dict::new();
    parms.set("Predictor", Obj::Int(15));
    parms.set("Colors", Obj::Int(r.ch as i64));
    parms.set("BitsPerComponent", Obj::Int(8));
    parms.set("Columns", Obj::Int(r.w as i64));
    nd.set("DecodeParms", Obj::Dict(parms));
    nd.remove("DP");
    Some(Verdict::Recoded {
        dict: nd,
        data: z,
        ssim: None,
        lossy: false,
    })
}

/// PNG-filter every row, choosing per row the filter with the smallest sum of absolute values.
pub fn png_filter(data: &[u8], row: usize, bpp: usize) -> Vec<u8> {
    let rows = data.len() / row;
    let mut out = Vec::with_capacity(rows * (row + 1));
    let zero = vec![0u8; row];
    let mut cand: [Vec<u8>; 5] = std::array::from_fn(|_| vec![0u8; row]);
    for y in 0..rows {
        let cur = &data[y * row..(y + 1) * row];
        let prev = if y == 0 {
            &zero[..]
        } else {
            &data[(y - 1) * row..y * row]
        };
        for i in 0..row {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            cand[0][i] = cur[i];
            cand[1][i] = cur[i].wrapping_sub(a);
            cand[2][i] = cur[i].wrapping_sub(b);
            cand[3][i] = cur[i].wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8);
            cand[4][i] = cur[i].wrapping_sub(filters::paeth(a, b, c));
        }
        let best = (0..5)
            .min_by_key(|&f| {
                cand[f]
                    .iter()
                    .map(|&v| u32::from((v as i8).unsigned_abs()))
                    .sum::<u32>()
            })
            .unwrap_or(0);
        out.push(best as u8);
        out.extend_from_slice(&cand[best]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source as Src;
    use crate::testutil::Builder;

    fn photo(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 31 + y * 17) % 3) as i32 - 1;
                let fx = (x as f64 / w as f64 * 6.0).sin() * 60.0;
                let fy = (y as f64 / h as f64 * 4.0).cos() * 60.0;
                v.push((128.0 + fx + n as f64).clamp(0.0, 255.0) as u8);
                v.push((128.0 + fy + n as f64).clamp(0.0, 255.0) as u8);
                v.push((128.0 + fx * 0.5 + fy * 0.5).clamp(0.0, 255.0) as u8);
            }
        }
        v
    }

    fn doc_with_image(dict: &str, data: &[u8]) -> (Doc, u32) {
        let mut b = Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
        b.obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>");
        b.stream(4, dict, data);
        (Doc::open(Src::from_bytes(b.build(1, ""))).unwrap(), 4)
    }

    fn run(dict: &str, data: &[u8], level: Level, eff: Option<f64>, smask: bool) -> Verdict {
        let (doc, n) = doc_with_image(dict, data);
        let l = doc.load(n).unwrap();
        let p = Params {
            level,
            eff_dpi: eff,
            is_smask: smask,
            grayscale: false,
            doc: &doc,
        };
        recode(&p, l.obj.as_dict().unwrap(), l.stream.unwrap()).unwrap()
    }

    #[test]
    fn a_raw_photo_becomes_a_jpeg_that_keeps_the_floor_and_shrinks_to_the_page() {
        let (w, h) = (640, 480);
        let px = photo(w, h);
        let dict = "/Type /XObject /Subtype /Image /Width 640 /Height 480 /ColorSpace /DeviceRGB /BitsPerComponent 8";
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        match run(dict, &px, level, Some(300.0), false) {
            Verdict::Recoded {
                dict,
                data,
                ssim,
                lossy,
            } => {
                assert!(lossy);
                assert!(ssim.unwrap() >= 0.97, "{ssim:?}");
                assert!(data.len() < px.len() / 20, "{} vs {}", data.len(), px.len());
                // 300 dpi down to the 150 dpi cap: half the pixels each way.
                assert_eq!(dict.int("Width"), Some(320));
                assert_eq!(dict.int("Height"), Some(240));
                assert!(dict.get("Filter").unwrap().is_name("DCTDecode"));
            }
            Verdict::Kept(s) => panic!("kept: {s:?}"),
        }
    }

    #[test]
    fn a_page_resolution_under_the_cap_keeps_every_pixel() {
        let px = photo(320, 240);
        let dict = "/Type /XObject /Subtype /Image /Width 320 /Height 240 /ColorSpace /DeviceRGB /BitsPerComponent 8";
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        match run(dict, &px, level, Some(110.0), false) {
            Verdict::Recoded { dict, .. } => assert_eq!(dict.int("Width"), Some(320)),
            Verdict::Kept(s) => panic!("kept: {s:?}"),
        }
    }

    #[test]
    fn noise_is_recoded_only_when_the_floor_holds() {
        let mut x = 12345u32;
        let noise: Vec<u8> = (0..300 * 300)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect();
        let dict = "/Type /XObject /Subtype /Image /Width 300 /Height 300 /ColorSpace /DeviceGray /BitsPerComponent 8";
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        match run(dict, &noise, level, None, false) {
            Verdict::Recoded { ssim, data, .. } => {
                assert!(ssim.unwrap() >= 0.97);
                assert!(data.len() < noise.len());
            }
            Verdict::Kept(_) => {}
        }
    }

    #[test]
    fn a_soft_mask_is_only_ever_recoded_without_loss() {
        // A smooth ramp stored raw compresses well with predictors.
        let ramp: Vec<u8> = (0..200 * 200)
            .map(|i| ((i % 200) * 255 / 200) as u8)
            .collect();
        let dict = "/Type /XObject /Subtype /Image /Width 200 /Height 200 /ColorSpace /DeviceGray /BitsPerComponent 8";
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        match run(dict, &ramp, level, None, true) {
            Verdict::Recoded {
                dict, data, lossy, ..
            } => {
                assert!(!lossy);
                assert!(dict.get("Filter").unwrap().is_name("FlateDecode"));
                assert!(data.len() < 2000, "{}", data.len());
            }
            Verdict::Kept(s) => panic!("kept: {s:?}"),
        }
    }

    #[test]
    fn unsupported_kinds_are_left_alone() {
        let data = vec![0u8; 5000];
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Fixed(70),
        };
        for dict in [
            "/Subtype /Image /Width 100 /Height 50 /ImageMask true /BitsPerComponent 1",
            "/Subtype /Image /Width 100 /Height 50 /ColorSpace [/Indexed /DeviceRGB 1 <000000ffffff>] /BitsPerComponent 8",
            "/Subtype /Image /Width 100 /Height 50 /ColorSpace /DeviceCMYK /BitsPerComponent 8 /Filter /JPXDecode",
        ] {
            // Palette images may only ever be recoded without loss.
            match run(dict, &data, level, None, false) {
                Verdict::Recoded { lossy, .. } => assert!(!lossy, "{dict}"),
                Verdict::Kept(_) => {}
            }
        }
    }

    /// A stencil of text-like blobs, packed, 0 where it paints (the default `/Decode`).
    fn stencil(w: usize, h: usize, ink: impl Fn(usize, usize) -> bool) -> Vec<u8> {
        let rb = w.div_ceil(8);
        let mut v = vec![0xFFu8; rb * h];
        for y in 0..h {
            for x in 0..w {
                if ink(x, y) {
                    v[y * rb + (x >> 3)] &= !(0x80 >> (x & 7));
                }
            }
        }
        v
    }

    fn doc_with_layers(fg: &str, fg_data: &[u8], mask: &str, mask_data: &[u8]) -> Doc {
        let mut b = Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
        b.obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>");
        b.stream(4, fg, fg_data);
        b.stream(5, mask, mask_data);
        Doc::open(Src::from_bytes(b.build(1, ""))).unwrap()
    }

    fn recode_obj(doc: &Doc, n: u32, level: Level, eff: Option<f64>) -> Verdict {
        let l = doc.load(n).unwrap();
        let p = Params {
            level,
            eff_dpi: eff,
            is_smask: false,
            grayscale: false,
            doc,
        };
        recode(&p, l.obj.as_dict().unwrap(), l.stream.unwrap()).unwrap()
    }

    #[test]
    fn a_colour_layer_under_a_stencil_is_stored_far_below_the_page_resolution() {
        let (w, h) = (800usize, 1000usize);
        // A dark layer whose colour drifts slowly, and a stencil of glyph-sized blobs.
        let fg: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [
                    (60 + x / 40) as u8,
                    (50 + y / 50) as u8,
                    (70 + (x + y) / 80) as u8,
                ]
            })
            .collect();
        let st = stencil(w, h, |x, y| {
            (x / 7 + y / 9) % 3 == 0 && x % 7 < 4 && y % 9 < 6
        });
        let fg_dict = "/Type /XObject /Subtype /Image /Width 800 /Height 1000 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Mask 5 0 R";
        let mask_dict = "/Type /XObject /Subtype /Image /Width 800 /Height 1000 /ImageMask true /BitsPerComponent 1";
        let doc = doc_with_layers(fg_dict, &fg, mask_dict, &st);
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        match recode_obj(&doc, 4, level, Some(300.0)) {
            Verdict::Recoded {
                dict, data, ssim, ..
            } => {
                // The ordinary shrink to the cap would give 400 pixels across.
                assert!(dict.int("Width").unwrap() <= 134, "{:?}", dict.int("Width"));
                assert!(ssim.unwrap() >= 0.97);
                assert!(data.len() < 4000, "{}", data.len());
                assert!(dict.get("Mask").is_some());
            }
            Verdict::Kept(s) => panic!("kept: {s:?}"),
        }
        // The same picture with no mask is judged everywhere and keeps the cap.
        let doc = doc_with_layers(&fg_dict.replace(" /Mask 5 0 R", ""), &fg, mask_dict, &st);
        if let Verdict::Recoded { dict, .. } = recode_obj(&doc, 4, level, Some(300.0)) {
            assert_eq!(dict.int("Width"), Some(400));
        }
    }

    #[test]
    fn a_layer_whose_ink_colour_would_change_keeps_its_resolution() {
        // Left half dark, right half light, stencil covering the boundary: shrinking 8x smears
        // the boundary into the ink, which the score must notice.
        let (w, h) = (400usize, 400usize);
        let fg: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if (i % w) / 5 % 2 == 0 {
                    [20u8, 20, 20]
                } else {
                    [235, 235, 235]
                }
            })
            .collect();
        let st = stencil(w, h, |x, _| (x / 5) % 2 == 0);
        let doc = doc_with_layers(
            "/Type /XObject /Subtype /Image /Width 400 /Height 400 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Mask 5 0 R",
            &fg,
            "/Type /XObject /Subtype /Image /Width 400 /Height 400 /ImageMask true /BitsPerComponent 1",
            &st,
        );
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        if let Verdict::Recoded { dict, .. } = recode_obj(&doc, 4, level, None) {
            // Not stored at an eighth: the stripes are the ink.
            assert!(dict.int("Width").unwrap() > 50, "{:?}", dict.int("Width"));
        }
    }

    #[test]
    fn a_layer_of_light_ink_keeps_its_resolution_too() {
        // The mirror of the last test: the ink layer is white, the smear between is dark, and the
        // stencil paints the white. The score is taken against a black sheet for light ink, so
        // the smeared ink is as plainly wrong as dark ink would be.
        let (w, h) = (400usize, 400usize);
        let fg: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if (i % w) / 5 % 2 == 0 {
                    [250u8, 250, 250]
                } else {
                    [60, 60, 60]
                }
            })
            .collect();
        let st = stencil(w, h, |x, _| (x / 5) % 2 == 0);
        let doc = doc_with_layers(
            "/Type /XObject /Subtype /Image /Width 400 /Height 400 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Mask 5 0 R",
            &fg,
            "/Type /XObject /Subtype /Image /Width 400 /Height 400 /ImageMask true /BitsPerComponent 1",
            &st,
        );
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        if let Verdict::Recoded { dict, .. } = recode_obj(&doc, 4, level, None) {
            assert!(dict.int("Width").unwrap() > 50, "{:?}", dict.int("Width"));
        }
    }

    #[test]
    fn visibility_follows_the_stencil_the_soft_mask_and_decode() {
        let (w, h) = (16usize, 8usize);
        let left_ink = stencil(w, h, |x, _| x < 8);
        let fg = "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8";
        let left_cover = |v: &Visible| -> (u8, u8) {
            let c = v.coverage(16, 8);
            (c[0], c[15])
        };
        // A stencil paints where the sample is 0.
        let doc = doc_with_layers(
            &format!("{fg} /Mask 5 0 R"),
            &[0u8; 128],
            "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ImageMask true /BitsPerComponent 1",
            &left_ink,
        );
        let d = doc.load(4).unwrap().obj.as_dict().unwrap().clone();
        assert_eq!(
            left_cover(&load_visible(&doc, &d).unwrap().unwrap()),
            (255, 0)
        );
        // `/Decode [1 0]` turns it around.
        let doc = doc_with_layers(
            &format!("{fg} /Mask 5 0 R"),
            &[0u8; 128],
            "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ImageMask true /BitsPerComponent 1 /Decode [1 0]",
            &left_ink,
        );
        let d = doc.load(4).unwrap().obj.as_dict().unwrap().clone();
        assert_eq!(
            left_cover(&load_visible(&doc, &d).unwrap().unwrap()),
            (0, 255)
        );
        // A 1-bit soft mask is opaque where the sample is 1: the bits are the inverse of the stencil.
        let opaque_left: Vec<u8> = left_ink.iter().map(|b| !b).collect();
        let doc = doc_with_layers(
            &format!("{fg} /SMask 5 0 R"),
            &[0u8; 128],
            "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 1",
            &opaque_left,
        );
        let d = doc.load(4).unwrap().obj.as_dict().unwrap().clone();
        assert_eq!(
            left_cover(&load_visible(&doc, &d).unwrap().unwrap()),
            (255, 0)
        );
        // A soft mask with grays says nothing about where the picture is shown.
        let doc = doc_with_layers(
            &format!("{fg} /SMask 5 0 R"),
            &[0u8; 128],
            "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[255u8; 128],
        );
        let d = doc.load(4).unwrap().obj.as_dict().unwrap().clone();
        assert!(load_visible(&doc, &d).unwrap().is_none());
    }

    /// Rows of text-like pixels, packed with 1 for white, as a PDF stores a 1-bit picture.
    fn text_bits(w: usize, h: usize) -> Vec<u8> {
        let rb = w.div_ceil(8);
        let mut v = vec![0xFFu8; rb * h];
        for y in 0..h {
            for x in 0..w {
                let glyph = (y / 12) % 2 == 0 && (x / 9) % 3 != 2 && (x % 9) < 6 && (y % 12) < 8;
                if glyph {
                    v[y * rb + (x >> 3)] &= !(0x80 >> (x & 7));
                }
            }
        }
        v
    }

    #[test]
    fn a_one_bit_picture_stored_flat_becomes_group_4_with_the_same_samples() {
        let (w, h) = (600usize, 400usize);
        let bits = text_bits(w, h);
        for extra in ["/ImageMask true", "/ColorSpace /DeviceGray /Decode [1 0]"] {
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width 600 /Height 400 /BitsPerComponent 1 {extra}"
            );
            let level = Level {
                dpi_cap: 150.0,
                quality: Quality::Adaptive { floor: 0.97 },
            };
            let (dict2, data) = match run(&dict, &bits, level, Some(300.0), false) {
                Verdict::Recoded {
                    dict, data, lossy, ..
                } => {
                    assert!(!lossy);
                    (dict, data)
                }
                Verdict::Kept(s) => panic!("kept: {s:?}"),
            };
            assert!(
                dict2.get("Filter").unwrap().is_name("CCITTFaxDecode"),
                "{extra}"
            );
            let parms = dict2.get("DecodeParms").and_then(Obj::as_dict).unwrap();
            assert_eq!(parms.int("K"), Some(-1));
            assert_eq!(parms.int("Columns"), Some(600));
            assert!(
                data.len() < bits.len() / 5,
                "{} vs {}",
                data.len(),
                bits.len()
            );
            // Same samples, and the dictionary still says how to read them.
            assert_eq!(dict2.has("ImageMask"), extra.starts_with("/ImageMask"));
            assert_eq!(dict2.has("Decode"), extra.contains("Decode"));
            // Decoding the new stream gives the original rows back.
            let rb = w.div_ceil(8);
            let mut y = 0;
            fax::decoder::decode_g4(data.iter().copied(), w as u16, Some(h as u16), |t| {
                let mut row = vec![0xFFu8; rb];
                let mut black = false;
                let mut x = 0usize;
                for &edge in t.iter().chain(std::iter::once(&(w as u16))) {
                    if black {
                        for px in x..usize::from(edge) {
                            row[px >> 3] &= !(0x80 >> (px & 7));
                        }
                    }
                    x = usize::from(edge);
                    black = !black;
                }
                assert_eq!(row, bits[y * rb..(y + 1) * rb], "row {y}");
                y += 1;
            })
            .unwrap();
            assert_eq!(y, h);
        }
    }

    #[test]
    fn a_one_bit_picture_that_group_4_does_not_shrink_is_left_alone() {
        // Pure noise: Group 4 is larger than the packed bits.
        let (w, h) = (256usize, 128usize);
        let mut x = 99u32;
        let bits: Vec<u8> = (0..w / 8 * h)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect();
        let dict = "/Type /XObject /Subtype /Image /Width 256 /Height 128 /BitsPerComponent 1 /ImageMask true";
        let level = Level {
            dpi_cap: 150.0,
            quality: Quality::Adaptive { floor: 0.97 },
        };
        assert!(matches!(
            run(dict, &bits, level, None, false),
            Verdict::Kept(Skip::NoGain)
        ));
    }

    #[test]
    fn png_filter_round_trips_through_the_predictor_decoder() {
        let (w, h) = (37, 11);
        let data: Vec<u8> = (0..w * h * 3)
            .map(|i| ((i * 7 + i / 5) % 251) as u8)
            .collect();
        let f = png_filter(&data, w * 3, 3);
        let mut parms = Dict::new();
        parms.set("Predictor", Obj::Int(15));
        parms.set("Colors", Obj::Int(3));
        parms.set("Columns", Obj::Int(w as i64));
        assert_eq!(filters::unpredict(f, &parms).unwrap(), data);
    }
}
