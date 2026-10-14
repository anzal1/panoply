//! OS encoders (ADR 0004): HEIC and AVIF through ImageIO on macOS. Nothing elsewhere.

use super::Embed;
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::types::Image;

/// No operating system encoder is wired outside macOS yet.
#[cfg(not(target_os = "macos"))]
pub fn can_encode(_format: Format) -> bool {
    false
}

/// No operating system encoder is wired outside macOS yet.
#[cfg(not(target_os = "macos"))]
pub fn encode(_img: &Image, _format: Format, _quality: f32, _embed: &Embed) -> Result<Vec<u8>> {
    Err(ImageError::Unsupported(
        "This computer has no operating system encoder for that format.".into(),
    ))
}

/// Can the OS write this format here? Asked of the system at run time.
#[cfg(target_os = "macos")]
pub fn can_encode(format: Format) -> bool {
    use panoply_platform_macos as pm;
    match format {
        Format::Heic => pm::can_encode(pm::Format::Heic),
        Format::Avif => pm::can_encode(pm::Format::Avif),
        _ => false,
    }
}

/// Encode through ImageIO. `quality` is 1 to 100.
#[cfg(target_os = "macos")]
pub fn encode(img: &Image, format: Format, quality: f32, embed: &Embed) -> Result<Vec<u8>> {
    use panoply_platform_macos as pm;

    use crate::types::{Layout, Samples};
    let fmt = match format {
        Format::Heic => pm::Format::Heic,
        Format::Avif => pm::Format::Avif,
        _ => {
            return Err(ImageError::Unsupported(
                "Only HEIC and AVIF go through the operating system encoder.".into(),
            ));
        }
    };
    let rgb_cow = img.as_rgb_image();
    let rgb: &Image = &rgb_cow;
    let alpha = rgb.layout == Layout::Rgba;
    let n = rgb.width as usize * rgb.height as usize;
    let ch = rgb.layout.channels();
    // ImageIO wants RGBA with straight alpha.
    enum Buf {
        U8(Vec<u8>),
        U16(Vec<u16>),
    }
    let buf = match &rgb.samples {
        Samples::U8(v) => {
            let mut o = Vec::with_capacity(n * 4);
            for p in v.chunks_exact(ch) {
                o.extend_from_slice(&[p[0], p[1], p[2], if alpha { p[3] } else { 255 }]);
            }
            Buf::U8(o)
        }
        Samples::U16(v) => {
            let mut o = Vec::with_capacity(n * 4);
            for p in v.chunks_exact(ch) {
                o.extend_from_slice(&[p[0], p[1], p[2], if alpha { p[3] } else { 65535 }]);
            }
            Buf::U16(o)
        }
    };
    let pixels = match &buf {
        Buf::U8(v) => pm::Pixels::U8(v),
        Buf::U16(v) => pm::Pixels::U16(v),
    };
    let input = pm::EncodeInput {
        width: rgb.width,
        height: rgb.height,
        pixels,
        space: pm::Space::Srgb,
    };
    let icc_bytes = img.icc.as_ref().map(|a| a.as_slice());
    let opts = pm::EncodeOptions {
        quality: (quality / 100.0).clamp(0.0, 1.0),
        icc: icc_bytes,
        keep_properties: embed.os_props.as_ref(),
        drop_gps: embed.drop_gps,
        opaque: !alpha,
    };
    pm::encode(&input, fmt, &opts)
        .map_err(|e| ImageError::Failed(format!("{} Choose JPG or WebP instead.", e.message())))
}
