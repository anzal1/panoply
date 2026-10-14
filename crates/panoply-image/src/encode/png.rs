//! PNG output. The `png` crate streams rows (huge pictures), and oxipng squeezes the result
//! when the picture is small enough to hold in memory twice.

use std::borrow::Cow;
use std::io::Write;
use std::sync::Arc;

use png::{BitDepth, ColorType, Compression, Encoder, Info};

use super::Embed;
use crate::error::{ImageError, Result};
use crate::pipeline::RowSink;
use crate::types::{Depth, Image, Layout, Rows};

/// How hard to try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// The `png` crate's fast mode: the right choice for streaming and for quality searches.
    Fast,
    /// Balanced deflate and adaptive filters, no second pass. What an instant conversion uses.
    Balanced,
    /// Balanced deflate and adaptive filters, then oxipng level 2 when the picture is small.
    Normal,
    /// oxipng level 4 (more filter and deflate trials).
    Max,
}

/// Pictures larger than this many pixels skip oxipng (it needs the decoded picture again, plus
/// its trial buffers, which would break the memory cap).
pub const OXIPNG_MAX_PIXELS: u64 = 24_000_000;

fn err(e: impl std::fmt::Display) -> ImageError {
    ImageError::Failed(format!(
        "The PNG could not be written ({e}). Try again, or save as another format."
    ))
}

/// Build the PNG `Info` header for an image shape and metadata.
fn info_for(width: u32, height: u32, layout: Layout, depth: Depth, embed: &Embed) -> Info<'static> {
    let mut info = Info::with_size(width, height);
    info.color_type = match layout {
        Layout::Gray => ColorType::Grayscale,
        Layout::GrayAlpha => ColorType::GrayscaleAlpha,
        Layout::Rgb => ColorType::Rgb,
        Layout::Rgba => ColorType::Rgba,
    };
    info.bit_depth = if depth == Depth::U16 {
        BitDepth::Sixteen
    } else {
        BitDepth::Eight
    };
    if let Some(icc) = &embed.icc {
        info.icc_profile = Some(Cow::Owned(icc.clone()));
    }
    if let Some(exif) = &embed.exif {
        info.exif_metadata = Some(Cow::Owned(exif.clone()));
    }
    if let Some(xmp) = &embed.xmp
        && let Ok(text) = String::from_utf8(xmp.clone())
    {
        info.utf8_text.push(png::text_metadata::ITXtChunk::new(
            "XML:com.adobe.xmp",
            text,
        ));
    }
    info
}

fn compression(effort: Effort) -> Compression {
    match effort {
        Effort::Fast => Compression::Fast,
        Effort::Balanced | Effort::Normal => Compression::Balanced,
        Effort::Max => Compression::High,
    }
}

/// Encode a whole image to PNG bytes.
pub fn encode(img: &Image, embed: &Embed, effort: Effort) -> Result<Vec<u8>> {
    let info = info_for(img.width, img.height, img.layout, img.depth(), embed);
    let mut out = Vec::with_capacity(img.byte_size() / 3 + 1024);
    {
        let mut enc = Encoder::with_info(&mut out, info).map_err(err)?;
        enc.set_compression(compression(effort));
        let mut w = enc.write_header().map_err(err)?;
        match &img.samples {
            crate::types::Samples::U8(v) => w.write_image_data(v).map_err(err)?,
            crate::types::Samples::U16(v) => {
                let mut be = Vec::with_capacity(v.len() * 2);
                for s in v {
                    be.extend_from_slice(&s.to_be_bytes());
                }
                w.write_image_data(&be).map_err(err)?;
            }
        }
        w.finish().map_err(err)?;
    }
    if matches!(effort, Effort::Normal | Effort::Max)
        && (img.width as u64 * img.height as u64) <= OXIPNG_MAX_PIXELS
    {
        out = squeeze(&out, if effort == Effort::Max { 4 } else { 2 });
    }
    Ok(out)
}

/// Run oxipng over PNG bytes. Returns the input when oxipng cannot improve it or fails.
pub fn squeeze(png: &[u8], level: u8) -> Vec<u8> {
    let opts = oxipng::Options::from_preset(level);
    match oxipng::optimize_from_memory(png, &opts) {
        Ok(better) if better.len() < png.len() => better,
        _ => png.to_vec(),
    }
}

/// Encode an indexed image: `indices` one byte per pixel, `palette` as RGBA entries.
pub fn encode_indexed(
    width: u32,
    height: u32,
    indices: &[u8],
    palette: &[[u8; 4]],
    embed: &Embed,
    effort: Effort,
) -> Result<Vec<u8>> {
    let n = palette.len();
    let bits = match n {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    };
    let mut info = Info::with_size(width, height);
    info.color_type = ColorType::Indexed;
    info.bit_depth = match bits {
        1 => BitDepth::One,
        2 => BitDepth::Two,
        4 => BitDepth::Four,
        _ => BitDepth::Eight,
    };
    info.palette = Some(Cow::Owned(
        palette.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
    ));
    if palette.iter().any(|p| p[3] != 255) {
        let last = palette.iter().rposition(|p| p[3] != 255).unwrap_or(0);
        info.trns = Some(Cow::Owned(palette[..=last].iter().map(|p| p[3]).collect()));
    }
    if let Some(icc) = &embed.icc {
        info.icc_profile = Some(Cow::Owned(icc.clone()));
    }
    if let Some(exif) = &embed.exif {
        info.exif_metadata = Some(Cow::Owned(exif.clone()));
    }
    let mut out = Vec::new();
    {
        let mut enc = Encoder::with_info(&mut out, info).map_err(err)?;
        enc.set_compression(compression(effort));
        let mut w = enc.write_header().map_err(err)?;
        let packed: Vec<u8> = if bits == 8 {
            indices.to_vec()
        } else {
            let per_byte = 8 / bits;
            let row_bytes = (width as usize).div_ceil(per_byte);
            let mut v = vec![0u8; row_bytes * height as usize];
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let idx = indices[y * width as usize + x];
                    let shift = 8 - bits - (x % per_byte) * bits;
                    v[y * row_bytes + x / per_byte] |= idx << shift;
                }
            }
            v
        };
        w.write_image_data(&packed).map_err(err)?;
        w.finish().map_err(err)?;
    }
    if matches!(effort, Effort::Normal | Effort::Max)
        && (width as u64 * height as u64) <= OXIPNG_MAX_PIXELS
    {
        out = squeeze(&out, if effort == Effort::Max { 4 } else { 2 });
    }
    Ok(out)
}

/// Stream rows into a PNG written to `out`. Returns the writer when done.
pub fn stream<W: Write + 'static>(out: W, embed: &Embed, effort: Effort) -> PngStream<W> {
    PngStream {
        out: Some(out),
        embed: embed.clone(),
        effort,
        writer: None,
        depth: Depth::U8,
    }
}

/// A streaming PNG writer, see [`stream`].
pub struct PngStream<W: Write + 'static> {
    out: Option<W>,
    embed: Embed,
    effort: Effort,
    writer: Option<png::StreamWriter<'static, W>>,
    depth: Depth,
}

impl<W: Write + 'static> PngStream<W> {
    /// Finish the file.
    pub fn finish(mut self) -> Result<()> {
        let w = self
            .writer
            .take()
            .ok_or_else(|| ImageError::Failed("internal: PNG finished before it started".into()))?;
        w.finish().map_err(err)
    }
}

impl<W: Write + 'static> RowSink for PngStream<W> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        let mut embed = self.embed.clone();
        if embed.icc.is_none() {
            embed.icc = icc.map(|a| (*a).clone());
        }
        let info = info_for(width, height, layout, depth, &embed);
        let out = self.out.take().expect("begin is called once");
        let mut enc = Encoder::with_info(out, info).map_err(err)?;
        enc.set_compression(compression(self.effort));
        let w = enc.write_header().map_err(err)?;
        self.writer = Some(w.into_stream_writer().map_err(err)?);
        self.depth = depth;
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let w = self.writer.as_mut().expect("begin was called");
        match rows {
            Rows::U8(s) => w.write_all(s).map_err(err),
            Rows::U16(s) => {
                let mut be = Vec::with_capacity(s.len() * 2);
                for v in s {
                    be.extend_from_slice(&v.to_be_bytes());
                }
                w.write_all(&be).map_err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Samples;

    fn test_img(layout: Layout, depth: Depth) -> Image {
        let mut img = Image::new(37, 23, layout, depth);
        match &mut img.samples {
            Samples::U8(v) => v
                .iter_mut()
                .enumerate()
                .for_each(|(i, s)| *s = (i * 7 % 251) as u8),
            Samples::U16(v) => v
                .iter_mut()
                .enumerate()
                .for_each(|(i, s)| *s = (i * 311 % 65521) as u16),
        }
        img
    }

    #[test]
    fn round_trips_every_layout_and_depth() {
        for layout in [Layout::Gray, Layout::GrayAlpha, Layout::Rgb, Layout::Rgba] {
            for depth in [Depth::U8, Depth::U16] {
                let img = test_img(layout, depth);
                let bytes = encode(&img, &Embed::default(), Effort::Fast).unwrap();
                let dec = png::Decoder::new(std::io::Cursor::new(bytes));
                let mut r = dec.read_info().unwrap();
                let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
                let info = r.next_frame(&mut buf).unwrap();
                assert_eq!((info.width, info.height), (37, 23));
                if depth == Depth::U8 {
                    assert_eq!(&buf[..info.buffer_size()], img.as_u8().unwrap());
                }
            }
        }
    }

    #[test]
    fn oxipng_never_makes_it_bigger_and_stays_lossless() {
        let img = test_img(Layout::Rgb, Depth::U8);
        let fast = encode(&img, &Embed::default(), Effort::Fast).unwrap();
        let normal = encode(&img, &Embed::default(), Effort::Normal).unwrap();
        assert!(normal.len() <= fast.len());
        let dec = png::Decoder::new(std::io::Cursor::new(normal));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..img.byte_size()], img.as_u8().unwrap());
    }

    #[test]
    fn indexed_round_trip_with_alpha() {
        let palette = [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0]];
        let idx: Vec<u8> = (0..10 * 6).map(|i| (i % 3) as u8).collect();
        let bytes = encode_indexed(10, 6, &idx, &palette, &Embed::default(), Effort::Fast).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[0..4], &[255, 0, 0, 255]);
        assert_eq!(&buf[4..8], &[0, 255, 0, 128]);
        assert_eq!(&buf[8..12], &[0, 0, 255, 0]);
    }
}
