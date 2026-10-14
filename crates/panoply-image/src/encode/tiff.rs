//! TIFF output: lossless, deflate compressed with the horizontal predictor, 8 or 16 bit,
//! with the ICC profile when the picture has one.

use std::io::Cursor;

use tiff::encoder::colortype::{Gray8, Gray16, RGB8, RGB16, RGBA8, RGBA16};
use tiff::encoder::{Compression, DeflateLevel, Predictor, TiffEncoder};
use tiff::tags::Tag;

use super::Embed;
use crate::error::{ImageError, Result};
use crate::types::{Image, Layout, Samples};

fn err(e: impl std::fmt::Display) -> ImageError {
    ImageError::Failed(format!(
        "The TIFF could not be written ({e}). Try another format."
    ))
}

/// Encode an image as TIFF.
pub fn encode(img: &Image, embed: &Embed) -> Result<Vec<u8>> {
    // The TIFF writer has no gray plus alpha type, so that case goes out as RGBA.
    let expanded;
    let img = if img.layout == Layout::GrayAlpha {
        expanded = img.to_rgb_layout();
        &expanded
    } else {
        img
    };
    let mut buf = Cursor::new(Vec::with_capacity(img.byte_size() / 2));
    {
        let mut enc = TiffEncoder::new(&mut buf)
            .map_err(err)?
            .with_compression(Compression::Deflate(DeflateLevel::Balanced))
            .with_predictor(Predictor::Horizontal);
        macro_rules! write {
            ($ct:ty, $data:expr) => {{
                let mut image = enc.new_image::<$ct>(img.width, img.height).map_err(err)?;
                if let Some(icc) = &embed.icc {
                    image
                        .encoder()
                        .write_tag(Tag::IccProfile, &icc[..])
                        .map_err(err)?;
                }
                image.write_data($data).map_err(err)?;
            }};
        }
        match (&img.samples, img.layout) {
            (Samples::U8(v), Layout::Gray) => write!(Gray8, v),
            (Samples::U8(v), Layout::Rgb) => write!(RGB8, v),
            (Samples::U8(v), Layout::Rgba) => write!(RGBA8, v),
            (Samples::U16(v), Layout::Gray) => write!(Gray16, v),
            (Samples::U16(v), Layout::Rgb) => write!(RGB16, v),
            (Samples::U16(v), Layout::Rgba) => write!(RGBA16, v),
            (_, Layout::GrayAlpha) => unreachable!("expanded above"),
        }
    }
    Ok(buf.into_inner())
}
