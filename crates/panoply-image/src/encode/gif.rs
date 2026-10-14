//! Static GIF output: the picture is reduced to at most 256 colors by our own quantizer and
//! written with the `gif` crate. One entry of the palette carries transparency when needed.

use gif::{Encoder, Frame, Repeat};

use crate::error::{ImageError, Result};
use crate::quant;
use crate::types::Image;

/// Largest side of a GIF.
pub const MAX_SIDE: u32 = 65535;

fn err(e: impl std::fmt::Display) -> ImageError {
    ImageError::Failed(format!(
        "The GIF could not be written ({e}). Try another format."
    ))
}

/// Encode a still GIF. `dither` is the error diffusion strength (0 to 1).
pub fn encode(img: &Image, dither: f32) -> Result<Vec<u8>> {
    if img.width > MAX_SIDE || img.height > MAX_SIDE {
        return Err(ImageError::TooLarge(format!(
            "GIF files cannot be larger than {MAX_SIDE} pixels on a side. Make the picture smaller."
        )));
    }
    let q = quant::quantize(img, 256, dither);
    let mut out = Vec::new();
    {
        let palette: Vec<u8> = q.palette.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
        let mut enc =
            Encoder::new(&mut out, img.width as u16, img.height as u16, &palette).map_err(err)?;
        enc.set_repeat(Repeat::Finite(0)).map_err(err)?;
        // Index of the first fully transparent entry, if any (the quantizer puts them first).
        let transparent = q.palette.iter().position(|c| c[3] < 128).map(|i| i as u8);
        let frame = Frame {
            width: img.width as u16,
            height: img.height as u16,
            buffer: std::borrow::Cow::Borrowed(&q.indices),
            transparent,
            ..Frame::default()
        };
        enc.write_frame(&frame).map_err(err)?;
    }
    Ok(out)
}
