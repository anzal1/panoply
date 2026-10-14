//! WebP output through libwebp (BSD-3): lossy with sharp YUV, or lossless, with alpha.
//! WebP caps a side at 16383 pixels.

use webp::{Encoder, PixelLayout, WebPConfig};

use super::Embed;
use crate::error::{ImageError, Result};
use crate::types::{Image, Layout, Samples};

/// Largest side a WebP can have.
pub const MAX_SIDE: u32 = 16383;

/// WebP settings.
#[derive(Debug, Clone)]
pub struct WebpOpts {
    /// 0 to 100 for lossy. For lossless it is the effort (higher is slower and smaller).
    pub quality: f32,
    /// Lossless mode.
    pub lossless: bool,
    /// Encoder method 0 (fast) to 6 (slow, smaller).
    pub method: i32,
    /// Metadata to embed.
    pub embed: Embed,
}

impl Default for WebpOpts {
    fn default() -> Self {
        Self {
            quality: 80.0,
            lossless: false,
            method: 4,
            embed: Embed::default(),
        }
    }
}

/// Encode an 8 bit image as WebP.
pub fn encode(img: &Image, opts: &WebpOpts) -> Result<Vec<u8>> {
    if img.width > MAX_SIDE || img.height > MAX_SIDE {
        return Err(ImageError::TooLarge(format!(
            "WebP files cannot be larger than {MAX_SIDE} pixels on a side, and this picture is {} x {}. Use PNG or AVIF, or make the picture smaller.",
            img.width, img.height
        )));
    }
    let rgb_any = img.as_rgb_image();
    let rgb = rgb_any.as_u8_image();
    let Samples::U8(data) = &rgb.samples else {
        unreachable!()
    };
    let layout = if rgb.layout == Layout::Rgba {
        PixelLayout::Rgba
    } else {
        PixelLayout::Rgb
    };
    let mut cfg = WebPConfig::new()
        .map_err(|_| ImageError::Failed("The WebP encoder could not start.".into()))?;
    cfg.lossless = opts.lossless as i32;
    cfg.quality = opts.quality.clamp(0.0, 100.0);
    cfg.method = opts.method.clamp(0, 6);
    cfg.alpha_compression = if opts.lossless { 0 } else { 1 };
    cfg.use_sharp_yuv = 1;
    let mem = Encoder::new(data, layout, rgb.width, rgb.height)
        .encode_advanced(&cfg)
        .map_err(|e| {
            ImageError::Failed(format!(
                "The WebP could not be written ({e:?}). Try another format."
            ))
        })?;
    let bytes = mem.to_vec();
    embed(bytes, &opts.embed)
}

/// Add the ICC profile, EXIF and XMP to a bare WebP file.
pub fn embed(bytes: Vec<u8>, e: &Embed) -> Result<Vec<u8>> {
    if e.icc.is_none() && e.exif.is_none() && e.xmp.is_none() {
        return Ok(bytes);
    }
    crate::meta::webp_embed(
        &bytes,
        e.icc.as_deref(),
        e.exif.as_deref(),
        e.xmp.as_deref(),
    )
    .map_err(|e| ImageError::Failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Depth;

    #[test]
    fn lossy_and_lossless_round_trip() {
        let mut img = Image::new(33, 17, Layout::Rgba, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            for (i, s) in v.iter_mut().enumerate() {
                *s = (i * 5 % 256) as u8;
            }
        }
        let lossless = encode(
            &img,
            &WebpOpts {
                lossless: true,
                ..Default::default()
            },
        )
        .unwrap();
        let dec = webp::Decoder::new(&lossless).decode().unwrap();
        assert_eq!((dec.width(), dec.height()), (33, 17));
        // Lossless keeps RGBA (colors under transparent pixels may change, so compare opaque ones).
        let want = img.as_u8().unwrap();
        for (a, b) in want.chunks_exact(4).zip(dec.chunks_exact(4)) {
            if a[3] == 255 {
                assert_eq!(a, b);
            }
        }
        let lossy = encode(
            &img,
            &WebpOpts {
                quality: 60.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(webp::Decoder::new(&lossy).decode().is_some());
    }

    #[test]
    fn too_wide_is_refused() {
        let img = Image::new(16384, 1, Layout::Rgb, Depth::U8);
        assert!(encode(&img, &WebpOpts::default()).is_err());
    }
}
