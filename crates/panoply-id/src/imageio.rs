//! Reading and writing pictures for the ID modules: one upright 8 bit RGB (or RGBA) picture in
//! memory, and files that carry their print resolution.

use std::path::Path;

use panoply_image::color::OutSpace;
use panoply_image::error::{ImageError, Result};
use panoply_image::format::Format;
use panoply_image::ops::Io;
use panoply_image::ops::common::{
    Animation, EncodeSpec, check_cancel, display_name, encode_bytes, load,
};
use panoply_image::orient;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image};
use panoply_image::resample::Filter;
use panoply_image::types::{Depth, Image};

/// What `load_upright` found out on the way.
pub struct Upright {
    /// The picture: orientation applied, sRGB, 8 bit.
    pub image: Image,
    /// Width in pixels of the file before any reduction.
    pub full_width: u32,
    /// Height in pixels of the file before any reduction.
    pub full_height: u32,
    /// Lines for the card ("applied the orientation tag", ...).
    pub notes: Vec<String>,
}

/// Open `path` as an upright sRGB picture, brought down so its long side is at most `cap_long`
/// (never enlarged). With `flatten` set, transparency is composited on that colour and the
/// result is RGB; without it an alpha channel is kept.
pub fn load_upright(
    io: &dyn Io,
    path: &Path,
    cap_long: u32,
    flatten: Option<[u8; 3]>,
) -> Result<Upright> {
    let mut notes = Vec::new();
    let mut loaded = load(io, path, None, Animation::First, OutSpace::Srgb)?;
    let info = loaded.info.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = orient::oriented_size(o, info.full_width, info.full_height);
    let long = uw.max(uh);
    let (sw, sh) = if long > cap_long {
        let s = f64::from(cap_long) / f64::from(long);
        (
            ((f64::from(uw) * s).round() as u32).max(1),
            ((f64::from(uh) * s).round() as u32).max(1),
        )
    } else {
        (uw, uh)
    };
    if info.format.decodes_reduced() && (sw < uw || sh < uh) {
        loaded = load(io, path, Some((sw, sh)), Animation::First, OutSpace::Srgb)?;
    }
    let scale = ((sw, sh) != (uw, uh)).then_some((sw, sh));
    let (cancel, progress) = (io.cancel(), |f: f32| io.progress(f * 0.2, "Reading"));
    let ctl = Ctl {
        cancel,
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        scale,
        filter: scale.map(|_| Filter::auto(uw, uh, sw, sh)),
        flatten,
        out_depth: Some(Depth::U8),
    };
    let (image, r) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
    notes.extend(r.notes);
    if o > 1 {
        notes.push(format!(
            "applied the orientation tag ({})",
            panoply_image::ops::convert::orientation_words(o)
        ));
    }
    if info.frames > 1 {
        notes.push("used the first frame of an animation".into());
    }
    check_cancel(io)?;
    let _ = display_name(path);
    Ok(Upright {
        image,
        full_width: uw,
        full_height: uh,
        notes,
    })
}

/// Encode an 8 bit RGB picture as a baseline JPEG that carries `dpi` in its JFIF header.
pub fn jpeg_bytes(img: &Image, quality: f32, subsample: bool, dpi: u32) -> Result<Vec<u8>> {
    let mut s = EncodeSpec::new(Format::Jpeg);
    s.quality = Some(quality);
    s.subsample = subsample;
    s.progressive = false;
    let mut b = encode_bytes(img, &s)?;
    set_jfif_density(&mut b, dpi);
    Ok(b)
}

/// Write `dpi` into the JFIF header of `jpeg` (dots per inch), in place. The length does not
/// change. Files without a JFIF segment are left alone.
pub fn set_jfif_density(jpeg: &mut [u8], dpi: u32) {
    // SOI, then APP0 marker (FF E0), length (2), "JFIF\0", version (2), units, xd (2), yd (2).
    if jpeg.len() < 20
        || jpeg[0..2] != [0xFF, 0xD8]
        || jpeg[2..4] != [0xFF, 0xE0]
        || &jpeg[6..11] != b"JFIF\0"
    {
        return;
    }
    let d = (dpi.min(65535) as u16).to_be_bytes();
    jpeg[13] = 1;
    jpeg[14..16].copy_from_slice(&d);
    jpeg[16..18].copy_from_slice(&d);
}

/// The print resolution a JPEG says it has (JFIF dots per inch or per cm), when it says one.
pub fn jfif_dpi(jpeg: &[u8]) -> Option<f64> {
    if jpeg.len() < 20
        || jpeg[0..2] != [0xFF, 0xD8]
        || jpeg[2..4] != [0xFF, 0xE0]
        || &jpeg[6..11] != b"JFIF\0"
    {
        return None;
    }
    let units = jpeg[13];
    let (x, y) = (
        f64::from(u16::from_be_bytes([jpeg[14], jpeg[15]])),
        f64::from(u16::from_be_bytes([jpeg[16], jpeg[17]])),
    );
    if x < 1.0 || (x - y).abs() > 0.5 {
        return None;
    }
    match units {
        1 => Some(x),
        2 => Some(x * 2.54),
        _ => None,
    }
}

/// Add a `pHYs` chunk (pixels per metre) to a PNG so it carries its print resolution. A file
/// that already has one, or that is not a PNG, comes back unchanged.
pub fn png_with_density(png: &[u8], dpi: u32) -> Vec<u8> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.len() < 33 || &png[..8] != SIG || &png[12..16] != b"IHDR" {
        return png.to_vec();
    }
    // Walk the chunks: stop before the first IDAT, bail out if pHYs is already there.
    let mut i = 8;
    let mut at = None;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        let kind = &png[i + 4..i + 8];
        if kind == b"pHYs" {
            return png.to_vec();
        }
        if kind == b"IDAT" {
            at = Some(i);
            break;
        }
        i += 12 + len;
    }
    let Some(at) = at else { return png.to_vec() };
    let ppm = (f64::from(dpi) / 0.0254).round() as u32;
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&ppm.to_be_bytes());
    chunk.extend_from_slice(&ppm.to_be_bytes());
    chunk.push(1);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..at]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[at..]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A plain error for a picture the face tools cannot use.
pub fn too_small(what: &str) -> ImageError {
    ImageError::Failed(format!(
        "{what} Use a larger photo, ideally the original from the camera."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use panoply_image::types::{Layout, Samples};

    fn gradient(w: u32, h: u32) -> Image {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[(x * 5) as u8, (y * 5) as u8, 128]);
            }
        }
        Image {
            width: w,
            height: h,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        }
    }

    #[test]
    fn a_jpeg_carries_its_dpi() {
        let b = jpeg_bytes(&gradient(40, 50), 90.0, false, 300).unwrap();
        assert_eq!(jfif_dpi(&b), Some(300.0));
        let mut c = b.clone();
        set_jfif_density(&mut c, 600);
        assert_eq!(c.len(), b.len());
        assert_eq!(jfif_dpi(&c), Some(600.0));
    }

    #[test]
    fn a_png_gets_a_valid_phys_chunk() {
        let img = gradient(20, 20);
        let png = encode_bytes(&img, &EncodeSpec::new(Format::Png)).unwrap();
        let with = png_with_density(&png, 300);
        assert_eq!(with.len(), png.len() + 21);
        // The png crate reads it back as 300 dpi (11811 pixels per metre).
        let dec = png::Decoder::new(std::io::Cursor::new(&with));
        let reader = dec.read_info().unwrap();
        let d = reader.info().pixel_dims.unwrap();
        assert_eq!((d.xppu, d.yppu), (11811, 11811));
        assert_eq!(png_with_density(&with, 600).len(), with.len());
    }

    #[test]
    fn crc_matches_the_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
