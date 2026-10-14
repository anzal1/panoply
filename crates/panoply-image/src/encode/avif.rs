//! AVIF output with `ravif` (rav1e, BSD-2), the fallback where the OS has no AVIF encoder.
//! ravif cannot tag a color profile, so callers convert to sRGB first.

use imgref::Img;
use ravif::{BitDepth, ColorModel, Encoder};
use rgb::{RGB8, RGBA8};

use super::Embed;
use crate::error::{ImageError, Result};
use crate::types::{Image, Layout, Samples};

/// AVIF settings.
#[derive(Debug, Clone)]
pub struct AvifOpts {
    /// 1 to 100.
    pub quality: f32,
    /// rav1e speed 1 (slowest, smallest) to 10 (fastest).
    pub speed: u8,
    /// Threads to use (the CPU budget).
    pub threads: usize,
    /// Metadata to embed (only EXIF is carried).
    pub embed: Embed,
}

impl Default for AvifOpts {
    fn default() -> Self {
        Self {
            quality: 65.0,
            speed: 6,
            threads: 2,
            embed: Embed::default(),
        }
    }
}

/// Encode an image as AVIF (8 bit).
pub fn encode(img: &Image, opts: &AvifOpts) -> Result<Vec<u8>> {
    let rgb_any = img.as_rgb_image();
    let rgb = rgb_any.as_u8_image();
    let Samples::U8(data) = &rgb.samples else {
        unreachable!()
    };
    let mut enc = Encoder::new()
        .with_quality(opts.quality)
        .with_alpha_quality(opts.quality.max(70.0))
        .with_speed(opts.speed)
        .with_num_threads(Some(opts.threads.max(1)))
        .with_bit_depth(BitDepth::Eight)
        .with_internal_color_model(ColorModel::YCbCr);
    if let Some(exif) = &opts.embed.exif {
        enc = enc.with_exif(exif.as_slice());
    }
    let (w, h) = (rgb.width as usize, rgb.height as usize);
    let out = if rgb.layout == Layout::Rgba {
        let px: &[RGBA8] = bytemuck::cast_slice(data);
        enc.encode_rgba(Img::new(px, w, h))
    } else {
        let px: &[RGB8] = bytemuck::cast_slice(data);
        enc.encode_rgb(Img::new(px, w, h))
    };
    out.map(|e| e.avif_file).map_err(|e| {
        ImageError::Failed(format!(
            "The AVIF could not be written ({e}). Try another format."
        ))
    })
}
