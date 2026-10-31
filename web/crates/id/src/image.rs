//! The slice of panoply_image::types the ID tools use, so they build for wasm32 without the C codecs.
use std::sync::Arc;

use crate::error::{ImageError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout { Gray, GrayAlpha, Rgb, Rgba }
impl Layout {
    pub fn channels(self) -> usize { match self { Layout::Gray => 1, Layout::GrayAlpha => 2, Layout::Rgb => 3, Layout::Rgba => 4 } }
    pub fn has_alpha(self) -> bool { matches!(self, Layout::GrayAlpha | Layout::Rgba) }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth { U8, U16 }
pub enum Rows<'a> { U8(&'a [u8]), U16(&'a [u16]) }
impl Rows<'_> { pub fn len(&self) -> usize { match self { Self::U8(v) => v.len(), Self::U16(v) => v.len() } } }
#[derive(Debug, Clone)]
pub enum Samples { U8(Vec<u8>), U16(Vec<u16>) }
#[derive(Debug, Clone)]
pub struct Image { pub width: u32, pub height: u32, pub layout: Layout, pub samples: Samples, pub icc: Option<Arc<Vec<u8>>> }
impl Image {
    pub fn rgba(width: u32, height: u32, data: Vec<u8>) -> Image { Image { width, height, layout: Layout::Rgba, samples: Samples::U8(data), icc: None } }
    /// A copy of the rectangle `(x, y, w, h)`.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Image {
        let ch = self.layout.channels();
        let Samples::U8(src) = &self.samples else { return self.clone() };
        let mut out = Vec::with_capacity(w as usize * h as usize * ch);
        for row in y..y + h {
            let s = (row as usize * self.width as usize + x as usize) * ch;
            out.extend_from_slice(&src[s..s + w as usize * ch]);
        }
        Image { width: w, height: h, layout: self.layout, samples: Samples::U8(out), icc: None }
    }
    pub fn is_opaque(&self) -> bool {
        if !self.layout.has_alpha() { return true; }
        let ch = self.layout.channels();
        match &self.samples { Samples::U8(v) => v.chunks_exact(ch).all(|p| p[ch - 1] == 255), Samples::U16(v) => v.chunks_exact(ch).all(|p| p[ch - 1] == 65535) }
    }
}
/// Resample like the desktop: in linear light with 16 bit samples, Lanczos 3 when shrinking and Mitchell when enlarging, premultiplied alpha,
/// and no halos (panoply-image resample.rs, copied unchanged).
pub fn resample_image(img: &Image, w: u32, h: u32) -> Result<Image> {
    if (w, h) == (img.width, img.height) { return Ok(img.clone()); }
    let Samples::U8(src) = &img.samples else { return Err(ImageError::Failed("internal: 8 bit only".into())) };
    let filter = crate::resample::Filter::auto(img.width, img.height, w, h);
    let mut r = crate::resample::Resampler::new(img.width, img.height, w, h, img.layout, Depth::U8, filter)?;
    let row = img.width as usize * img.layout.channels();
    let mut out: Vec<u8> = Vec::with_capacity(w as usize * h as usize * img.layout.channels());
    for chunk in src.chunks(row * 64) {
        r.push(Rows::U8(chunk), &mut |rows, _y, _n| { if let Rows::U8(s) = rows { out.extend_from_slice(s); } Ok(()) })?;
    }
    r.finish()?;
    Ok(Image { width: w, height: h, layout: img.layout, samples: Samples::U8(out), icc: None })
}
