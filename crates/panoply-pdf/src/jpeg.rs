//! JPEG: reading the facts a PDF needs from the header, decoding to pixels, and encoding.
//!
//! Decoding is zune-jpeg and encoding is jpeg-encoder, both pure Rust. The header parser is our
//! own so that images can be inspected without decoding them: size, components, whether it is
//! the Adobe CMYK flavour, density, EXIF orientation.

use jpeg_encoder::{ColorType, Encoder, QuantizationTableType, SamplingFactor};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// Facts from a JPEG header.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JpegInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Colour components: 1 gray, 3 colour, 4 CMYK.
    pub components: u8,
    /// Bits per sample.
    pub precision: u8,
    /// True for progressive.
    pub progressive: bool,
    /// The Adobe marker's transform flag, when present.
    pub adobe_transform: Option<u8>,
    /// Pixel density in dots per inch (x, y), from JFIF or EXIF.
    pub dpi: Option<(f64, f64)>,
    /// EXIF orientation 1 to 8.
    pub orientation: Option<u8>,
    /// Has an embedded ICC profile.
    pub icc: bool,
}

/// Read the header of a JPEG. `None` when the bytes are not a JPEG with a frame header.
pub fn parse_info(b: &[u8]) -> Option<JpegInfo> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut info = JpegInfo::default();
    let mut got_frame = false;
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let mut m = b[i + 1];
        let mut j = i + 1;
        while m == 0xFF && j + 1 < b.len() {
            j += 1;
            m = b[j];
        }
        i = j + 1;
        match m {
            0x00 | 0x01 | 0xD0..=0xD7 | 0xD8 => continue,
            0xD9 | 0xDA => break,
            _ => {}
        }
        if i + 2 > b.len() {
            break;
        }
        let len = usize::from(u16::from_be_bytes([b[i], b[i + 1]]));
        if len < 2 || i + len > b.len() {
            break;
        }
        let seg = &b[i + 2..i + len];
        match m {
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) && seg.len() >= 6 => {
                info.precision = seg[0];
                info.height = u32::from(u16::from_be_bytes([seg[1], seg[2]]));
                info.width = u32::from(u16::from_be_bytes([seg[3], seg[4]]));
                info.components = seg[5];
                info.progressive = matches!(m, 0xC2 | 0xC6 | 0xCA | 0xCE);
                got_frame = true;
            }
            0xE0 if seg.starts_with(b"JFIF\0") && seg.len() >= 12 => {
                let (xd, yd) = (
                    f64::from(u16::from_be_bytes([seg[8], seg[9]])),
                    f64::from(u16::from_be_bytes([seg[10], seg[11]])),
                );
                match seg[7] {
                    1 if xd > 1.0 && yd > 1.0 => info.dpi = info.dpi.or(Some((xd, yd))),
                    2 if xd > 0.0 && yd > 0.0 => {
                        info.dpi = info.dpi.or(Some((xd * 2.54, yd * 2.54)))
                    }
                    _ => {}
                }
            }
            0xE1 if seg.starts_with(b"Exif\0\0") => exif(&seg[6..], &mut info),
            0xE2 if seg.starts_with(b"ICC_PROFILE\0") => info.icc = true,
            0xEE if seg.starts_with(b"Adobe") && seg.len() >= 12 => {
                info.adobe_transform = Some(seg[11])
            }
            _ => {}
        }
        i += len;
    }
    got_frame.then_some(info)
}

fn exif(t: &[u8], info: &mut JpegInfo) {
    if t.len() < 8 {
        return;
    }
    let le = match &t[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let s = t.get(o..o + 2)?;
        Some(if le {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let s = t.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };
    let Some(ifd) = u32_at(4) else { return };
    let ifd = ifd as usize;
    let Some(n) = u16_at(ifd) else { return };
    let (mut xr, mut yr, mut unit) = (None, None, 2u16);
    let rational = |o: usize| -> Option<f64> {
        let (a, b) = (u32_at(o)?, u32_at(o + 4)?);
        (b != 0).then(|| f64::from(a) / f64::from(b))
    };
    for k in 0..usize::from(n).min(64) {
        let e = ifd + 2 + k * 12;
        let (Some(tag), Some(_ty)) = (u16_at(e), u16_at(e + 2)) else {
            break;
        };
        match tag {
            0x0112 => {
                if let Some(v) = u16_at(e + 8)
                    && (1..=8).contains(&v)
                {
                    info.orientation = Some(v as u8);
                }
            }
            0x011A => xr = u32_at(e + 8).and_then(|o| rational(o as usize)),
            0x011B => yr = u32_at(e + 8).and_then(|o| rational(o as usize)),
            0x0128 => unit = u16_at(e + 8).unwrap_or(2),
            _ => {}
        }
    }
    if let (Some(x), Some(y)) = (xr, yr)
        && x > 1.0
        && y > 1.0
    {
        let f = if unit == 3 { 2.54 } else { 1.0 };
        info.dpi = Some((x * f, y * f));
    }
}

/// Decoded pixels: interleaved 8-bit gray (1 channel) or RGB (3).
pub struct Raster {
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
    /// Channels per pixel, 1 or 3.
    pub ch: usize,
    /// `w * h * ch` bytes.
    pub data: Vec<u8>,
}

/// Decode a gray or RGB JPEG. `None` for anything else (CMYK, 12-bit, damaged).
pub fn decode(bytes: &[u8], info: &JpegInfo) -> Option<Raster> {
    let (cs, ch) = match (info.components, info.adobe_transform) {
        (1, _) => (ColorSpace::Luma, 1usize),
        (3, Some(0)) => return None, // RGB stored without the colour transform: leave alone
        (3, _) => (ColorSpace::RGB, 3usize),
        _ => return None,
    };
    if info.precision != 8 {
        return None;
    }
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(cs);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(bytes), opts);
    let data = dec.decode().ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    if data.len() != w * h * ch {
        return None;
    }
    Some(Raster { w, h, ch, data })
}

/// Which quantisation tables to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tables {
    /// The tables of Annex K of the standard.
    Standard,
    /// ImageMagick's tables (N. Robidoux).
    ImageMagick,
    /// Tables tuned for MS-SSIM.
    MsSsim,
    /// Tables tuned for PSNR-HVS.
    PsnrHvs,
}

impl Tables {
    fn get(self) -> QuantizationTableType {
        match self {
            Tables::Standard => QuantizationTableType::Default,
            Tables::ImageMagick => QuantizationTableType::ImageMagick,
            Tables::MsSsim => QuantizationTableType::CustomMsSsim,
            Tables::PsnrHvs => QuantizationTableType::CustomPsnrHvs,
        }
    }
}

/// How to encode.
#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions {
    /// Quantisation tables.
    pub tables: Tables,
    /// 1 to 100.
    pub quality: u8,
    /// Progressive scans (smaller for big images).
    pub progressive: bool,
    /// Chroma subsampling 4:2:0 for colour (4:4:4 when false).
    pub subsample: bool,
}

/// Encode gray (1 channel) or RGB (3) pixels to a JPEG.
pub fn encode(data: &[u8], w: usize, h: usize, ch: usize, o: EncodeOptions) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || w > 65535 || h > 65535 || data.len() != w * h * ch {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() / 8 + 1024);
    let mut enc = Encoder::new(&mut out, o.quality.clamp(1, 100));
    // Progressive output is always an interleaved first scan, and baseline is written
    // interleaved only without Huffman optimisation. Both decode in every viewer, which a
    // multi-scan baseline file (what optimised tables produce) does not.
    enc.set_optimized_huffman_tables(false);
    enc.set_progressive(o.progressive);
    enc.set_sampling_factor(if o.subsample && ch == 3 {
        SamplingFactor::F_2_2
    } else {
        SamplingFactor::F_1_1
    });
    enc.set_quantization_tables(o.tables.get(), o.tables.get());
    let ct = if ch == 1 {
        ColorType::Luma
    } else {
        ColorType::Rgb
    };
    enc.encode(data, w as u16, h as u16, ct).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7 + y * 13) % 5) as i32 - 2;
                v.push(((x * 255 / w) as i32 + n).clamp(0, 255) as u8);
                v.push(((y * 255 / h) as i32 + n).clamp(0, 255) as u8);
                v.push((((x + y) * 255 / (w + h)) as i32 - n).clamp(0, 255) as u8);
            }
        }
        v
    }

    #[test]
    fn encode_decode_round_trip_and_header() {
        let (w, h) = (96, 64);
        let px = photo(w, h);
        let jpg = encode(
            &px,
            w,
            h,
            3,
            EncodeOptions {
                tables: Tables::Standard,
                quality: 85,
                progressive: false,
                subsample: true,
            },
        )
        .unwrap();
        let info = parse_info(&jpg).unwrap();
        assert_eq!((info.width, info.height, info.components), (96, 64, 3));
        assert!(!info.progressive);
        let r = decode(&jpg, &info).unwrap();
        assert_eq!((r.w, r.h, r.ch), (96, 64, 3));
        let a = crate::ssim::luma(&px, 3);
        let b = crate::ssim::luma(&r.data, 3);
        assert!(crate::ssim::ssim_gray(&a, &b, w, h) > 0.95);
        // Chroma subsampling must not scramble the picture (a multi-scan baseline file did).
        let worst = px
            .iter()
            .zip(&r.data)
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap();
        assert!(worst < 40, "worst pixel error {worst}");
    }

    #[test]
    fn gray_and_progressive() {
        let (w, h) = (80, 80);
        let px: Vec<u8> = (0..w * h).map(|i| ((i % w) * 3) as u8).collect();
        let jpg = encode(
            &px,
            w,
            h,
            1,
            EncodeOptions {
                tables: Tables::Standard,
                quality: 70,
                progressive: true,
                subsample: false,
            },
        )
        .unwrap();
        let info = parse_info(&jpg).unwrap();
        assert_eq!(info.components, 1);
        assert!(info.progressive);
        assert!(decode(&jpg, &info).is_some());
    }

    #[test]
    fn non_jpeg_has_no_info() {
        assert!(parse_info(b"\x89PNG....").is_none());
        assert!(parse_info(&[0xFF, 0xD8]).is_none());
    }

    #[test]
    fn exif_orientation_and_jfif_density() {
        // SOI, JFIF at 300 dpi, EXIF orientation 6, then a minimal SOF0.
        let mut b = vec![0xFF, 0xD8];
        let jfif = b"JFIF\0\x01\x01\x01\x01\x2C\x01\x2C\0\0";
        b.extend_from_slice(&[0xFF, 0xE0]);
        b.extend_from_slice(&((jfif.len() + 2) as u16).to_be_bytes());
        b.extend_from_slice(jfif);
        let mut ex = b"Exif\0\0MM\0\x2A\0\0\0\x08".to_vec();
        ex.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&[0xFF, 0xE1]);
        b.extend_from_slice(&((ex.len() + 2) as u16).to_be_bytes());
        b.extend_from_slice(&ex);
        b.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8, 0, 10, 0, 20, 1, 1, 0x11, 0]);
        let info = parse_info(&b).unwrap();
        assert_eq!((info.width, info.height), (20, 10));
        assert_eq!(info.orientation, Some(6));
        assert_eq!(info.dpi, Some((300.0, 300.0)));
    }
}
