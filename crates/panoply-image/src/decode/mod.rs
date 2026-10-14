//! Decoders. Each one turns a file into a [`RowReader`]: rows of raw samples in stored order,
//! plus the facts the pipeline needs (size, layout, depth, color, orientation).
//!
//! Which decoder runs is decided from the file's first bytes. JPEG, PNG and TIFF stream row
//! by row, so a 400 megapixel file never needs its full bitmap in memory. WebP, GIF, BMP and
//! ICO decode whole (their formats cap the size or are small), and every whole decode is checked
//! against the memory cap first. HEIC, HEIF, AVIF, RAW, JPEG 2000 and EXR go through the OS
//! decoder where there is one (see `os`).

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::color::SrcColor;
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout};

mod bmp;
mod gif;
mod jpeg;
mod png;
mod tiff;
mod webp;

pub(crate) mod os;

/// Peak memory the pipeline allows for one job, in bytes (FLOWS.md: 300 MB for images).
pub const MEMORY_CAP: usize = 300 * 1024 * 1024;

/// Where the bytes come from: a file the guard opened, or a buffer already in memory (used to
/// decode our own encoder output when a quality search checks it).
pub enum Input {
    /// An open file.
    File(File),
    /// Bytes in memory.
    Mem(Cursor<Vec<u8>>),
}

impl Input {
    /// Total length in bytes.
    pub fn len(&mut self) -> std::io::Result<u64> {
        match self {
            Self::File(f) => Ok(f.metadata()?.len()),
            Self::Mem(c) => Ok(c.get_ref().len() as u64),
        }
    }

    /// True when there are no bytes.
    pub fn is_empty(&mut self) -> std::io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Read everything from the start into memory.
    pub fn into_vec(mut self) -> std::io::Result<Vec<u8>> {
        match self {
            Self::Mem(c) => Ok(c.into_inner()),
            Self::File(ref mut f) => {
                f.seek(SeekFrom::Start(0))?;
                let mut v = Vec::with_capacity(f.metadata().map(|m| m.len() as usize).unwrap_or(0));
                f.read_to_end(&mut v)?;
                Ok(v)
            }
        }
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(f) => f.read(buf),
            Self::Mem(c) => c.read(buf),
        }
    }
}

impl Seek for Input {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(f) => f.seek(pos),
            Self::Mem(c) => c.seek(pos),
        }
    }
}

/// What a reader knows about its image.
#[derive(Debug, Clone)]
pub struct SrcInfo {
    /// The format of the file.
    pub format: Format,
    /// Width of the rows this reader returns, in pixels.
    pub width: u32,
    /// Height of the rows this reader returns, in pixels.
    pub height: u32,
    /// Width of the file's picture before any decoder side scaling.
    pub full_width: u32,
    /// Height of the file's picture before any decoder side scaling.
    pub full_height: u32,
    /// Layout of the raw rows. CMYK is `Rgba` with `color.cmyk` set (four samples).
    pub layout: Layout,
    /// Sample depth of the raw rows.
    pub depth: Depth,
    /// Color facts for the converter.
    pub color: SrcColor,
    /// EXIF orientation (1 to 8) still to be applied. 1 when `upright`.
    pub orientation: u8,
    /// The reader already returns upright rows (OS decoders).
    pub upright: bool,
    /// The reader already converted color to the working space (OS decoders).
    pub color_managed: bool,
    /// The profile the managed rows are in when it is not sRGB.
    pub managed_icc: Option<Arc<Vec<u8>>>,
    /// Alpha in the rows is premultiplied (OS decoders).
    pub premultiplied: bool,
    /// Pictures in the file: more than 1 means animated or multi-image.
    pub frames: usize,
    /// True when the file may carry transparency (the rows have an alpha channel).
    pub has_alpha: bool,
    /// Things the decode dropped on purpose (depth map, gain map, Live Photo video).
    pub dropped: Vec<String>,
    /// Human description of the color space, for the result card ("Display P3", "CMYK").
    pub color_label: Option<String>,
}

impl SrcInfo {
    /// A reader for a plain, forward-only stored-orientation source.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plain(
        format: Format,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
    ) -> Self {
        SrcInfo {
            format,
            width,
            height,
            full_width: width,
            full_height: height,
            layout,
            depth,
            color: SrcColor::default(),
            orientation: 1,
            upright: false,
            color_managed: false,
            managed_icc: None,
            premultiplied: false,
            frames: 1,
            has_alpha: layout.has_alpha(),
            dropped: Vec::new(),
            color_label: None,
        }
    }

    /// Samples in one raw row.
    pub fn raw_row_len(&self) -> usize {
        self.width as usize * self.layout.channels()
    }
}

/// A forward-only source of raw rows.
pub trait RowReader {
    /// What this reader knows.
    fn info(&self) -> &SrcInfo;

    /// Read up to `max_rows` more rows into `out` (replacing its contents). Returns the number
    /// of rows read, 0 at the end.
    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize>;
}

/// How to open a file.
#[derive(Debug, Clone, Default)]
pub struct OpenOpts {
    /// Smallest size, in stored orientation, the caller needs. JPEG uses it to decode at a
    /// reduced size (DCT scaling, the same shrink-on-load libvips does) while staying at
    /// least twice the target so the final filter still has real work to do.
    pub min_size_hint: Option<(u32, u32)>,
    /// For animated files: use the first frame instead of refusing.
    pub first_frame: bool,
    /// Memory the caller can spend on a whole decode, in bytes (defaults to [`MEMORY_CAP`] / 2).
    pub buffer_budget: Option<usize>,
    /// Whether wide gamut sources keep their profile. OS decoders convert while decoding, so
    /// they need to know up front.
    pub keep_color: bool,
}

impl OpenOpts {
    pub(crate) fn budget(&self) -> usize {
        self.buffer_budget.unwrap_or(MEMORY_CAP / 2)
    }
}

/// Everything about a file that can be known without decoding pixels.
#[derive(Debug, Clone)]
pub struct Probe {
    /// The format.
    pub format: Format,
    /// Stored width.
    pub width: u32,
    /// Stored height.
    pub height: u32,
    /// Orientation to apply (1 to 8).
    pub orientation: u8,
    /// Pictures in the file.
    pub frames: usize,
    /// Rows carry alpha.
    pub has_alpha: bool,
    /// Sample depth.
    pub depth: Depth,
    /// An embedded profile exists.
    pub has_icc: bool,
    /// The samples are CMYK.
    pub cmyk: bool,
}

/// Open a file for decoding.
pub fn open(
    mut file: Input,
    ext_hint: Option<&str>,
    opts: &OpenOpts,
) -> Result<Box<dyn RowReader>> {
    let mut head = [0u8; 64];
    let n = {
        let n = file.read(&mut head)?;
        file.seek(SeekFrom::Start(0))?;
        n
    };
    let format = crate::format::detect(&head[..n], ext_hint).ok_or_else(|| {
        ImageError::Unsupported(
            "This file is not an image Panoply can read. Check that it is a JPEG, PNG, WebP, GIF, TIFF, BMP, HEIC or AVIF file.".into(),
        )
    })?;
    let reader: Box<dyn RowReader> = match format {
        Format::Jpeg => Box::new(jpeg::JpegReader::open(file, opts)?),
        Format::Png => Box::new(png::PngReader::open(file, opts)?),
        Format::Tiff => Box::new(tiff::TiffReader::open(file, opts)?),
        Format::WebP => Box::new(webp::WebpReader::open(file, opts)?),
        Format::Gif => Box::new(gif::GifReader::open(file, opts)?),
        Format::Bmp | Format::Ico => Box::new(bmp::BmpReader::open(file, format, opts)?),
        Format::Heic | Format::Avif | Format::Jxl | Format::Jp2 | Format::Raw | Format::Exr => {
            os::open(file, format, opts)?
        }
    };
    Ok(reader)
}

/// Refuse a whole decode that would not fit in the memory budget.
pub(crate) fn check_whole_decode(
    format: Format,
    width: u32,
    height: u32,
    bytes_per_px: usize,
    budget: usize,
) -> Result<()> {
    let need = width as usize * height as usize * bytes_per_px;
    if need > budget {
        return Err(ImageError::TooLarge(format!(
            "This {} image is {width} x {height} pixels ({:.0} megapixels). Opening it would need about {} MB, over the {} MB limit, and the format cannot be read in pieces. Resize it with another tool first, or export a smaller version.",
            format.label(),
            width as f64 * height as f64 / 1e6,
            need / (1024 * 1024),
            budget / (1024 * 1024),
        )));
    }
    Ok(())
}

/// Wrap a reader around an in-memory image (WebP, GIF, BMP: whole decodes).
pub(crate) struct MemReader {
    info: SrcInfo,
    data: crate::types::Samples,
    next_row: usize,
}

impl MemReader {
    pub(crate) fn new(info: SrcInfo, data: crate::types::Samples) -> Self {
        Self {
            info,
            data,
            next_row: 0,
        }
    }
}

impl RowReader for MemReader {
    fn info(&self) -> &SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        let n = max_rows.min(self.info.height as usize - self.next_row);
        let rl = self.info.raw_row_len();
        let (a, b) = (self.next_row * rl, (self.next_row + n) * rl);
        match (&self.data, out) {
            (crate::types::Samples::U8(s), RowBuf::U8(d)) => {
                d.clear();
                d.extend_from_slice(&s[a..b]);
            }
            (crate::types::Samples::U16(s), RowBuf::U16(d)) => {
                d.clear();
                d.extend_from_slice(&s[a..b]);
            }
            _ => {
                return Err(ImageError::Failed(
                    "internal: row buffer depth mismatch".into(),
                ));
            }
        }
        self.next_row += n;
        Ok(n)
    }
}
