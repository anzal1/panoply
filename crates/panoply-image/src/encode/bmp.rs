//! BMP output (24 bit, or 32 bit with alpha) through the `image` crate.

use image::ExtendedColorType;
use image::codecs::bmp::BmpEncoder;

use crate::error::{ImageError, Result};
use crate::types::{Image, Layout, Samples};

/// Encode an 8 bit image as BMP.
pub fn encode(img: &Image) -> Result<Vec<u8>> {
    let rgb_any = img.as_rgb_image();
    let rgb = rgb_any.as_u8_image();
    let Samples::U8(data8) = &rgb.samples else {
        unreachable!()
    };
    let ct = match rgb.layout {
        Layout::Rgba => ExtendedColorType::Rgba8,
        _ => ExtendedColorType::Rgb8,
    };
    let mut out = Vec::new();
    BmpEncoder::new(&mut out)
        .encode(data8, img.width, img.height, ct)
        .map_err(|e| {
            ImageError::Failed(format!(
                "The BMP could not be written ({e}). Try another format."
            ))
        })?;
    Ok(out)
}
