//! Encoders. JPEG and PNG can stream rows into a writer, so huge pictures never sit in memory.
//! The rest (WebP, AVIF, HEIC, TIFF, BMP, GIF) take a whole [`crate::types::Image`].

pub mod avif;
pub mod bmp;
pub mod gif;
pub mod jpeg;
pub mod jpegopt;
pub mod jpegstrips;
pub mod os;
pub mod png;
pub mod tiff;
pub mod webp;

/// Metadata blobs to carry into an output file.
#[derive(Debug, Clone, Default)]
pub struct Embed {
    /// ICC profile (not for plain sRGB).
    pub icc: Option<Vec<u8>>,
    /// EXIF as a TIFF header block (`II*\0...`), already prepared for the new pixels.
    pub exif: Option<Vec<u8>>,
    /// XMP packet.
    pub xmp: Option<Vec<u8>>,
    /// macOS only: the source's ImageIO property dictionaries, carried into HEIC and AVIF.
    #[cfg(target_os = "macos")]
    pub os_props: Option<panoply_platform_macos::Properties>,
    /// Leave GPS out of `os_props` (macOS HEIC and AVIF).
    pub drop_gps: bool,
}

use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::types::Image;

/// True when the OS can encode this format on this computer (HEIC and AVIF through ImageIO on
/// macOS). The check asks the system at run time.
pub fn os_can_encode(format: Format) -> bool {
    os::can_encode(format)
}

/// AVIF: the OS encoder first (ADR 0004), then `ravif`.
pub fn avif_or_os(img: &Image, quality: f32, embed: &Embed) -> Result<Vec<u8>> {
    if os::can_encode(Format::Avif) {
        return os::encode(img, Format::Avif, quality, embed);
    }
    // ravif cannot tag a color profile, so a wide gamut picture is converted to sRGB first.
    let owned;
    let img = if img.icc.is_some() {
        owned = srgb_copy(img)?;
        &owned
    } else {
        img
    };
    let plain = Embed {
        icc: None,
        exif: embed.exif.clone(),
        xmp: None,
        ..Default::default()
    };
    avif::encode(
        img,
        &avif::AvifOpts {
            quality,
            embed: plain,
            ..Default::default()
        },
    )
}

/// HEIC: only through the OS encoder (x265 is GPL and HEVC needs a patent review, decision 64).
pub fn heic(img: &Image, quality: f32, embed: &Embed) -> Result<Vec<u8>> {
    if os::can_encode(Format::Heic) {
        return os::encode(img, Format::Heic, quality, embed);
    }
    Err(ImageError::Unsupported(
        "Panoply writes HEIC only through the operating system's encoder, and this computer does not offer one. Choose JPG, WebP or AVIF instead.".into(),
    ))
}

/// A copy of `img` converted from its own profile to sRGB.
pub fn srgb_copy(img: &Image) -> Result<Image> {
    use crate::color::{ColorConv, OutSpace, SrcColor};
    let Some(icc) = &img.icc else {
        return Ok(img.clone());
    };
    let color = SrcColor {
        icc: Some((**icc).clone()),
        ..Default::default()
    };
    let conv = ColorConv::new(img.layout, img.depth(), &color, OutSpace::Srgb)?;
    let mut out = img.clone();
    out.icc = None;
    match (&img.samples, &mut out.samples) {
        (crate::types::Samples::U8(s), crate::types::Samples::U8(d)) => conv.convert_u8(s, d)?,
        (crate::types::Samples::U16(s), crate::types::Samples::U16(d)) => conv.convert_u16(s, d)?,
        _ => {}
    }
    Ok(out)
}
