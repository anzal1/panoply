//! BMP and ICO through the `image` crate (both are small and decode whole). An ICO with several
//! sizes yields its largest picture.

use std::io::Read;

use image::{DynamicImage, ImageFormat};

use super::{Input, MemReader, OpenOpts, RowReader, SrcInfo, check_whole_decode};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout, Samples};

pub(crate) struct BmpReader(MemReader);

impl BmpReader {
    pub(crate) fn open(mut file: Input, format: Format, opts: &OpenOpts) -> Result<Self> {
        let len = file.len().unwrap_or(0) as usize;
        if len > opts.budget() {
            return Err(ImageError::TooLarge(
                "This file is too large to open as a BMP or ICO.".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(len);
        file.read_to_end(&mut bytes)?;
        let fmt = if format == Format::Bmp {
            ImageFormat::Bmp
        } else {
            ImageFormat::Ico
        };
        let img = image::load_from_memory_with_format(&bytes, fmt).map_err(corrupt)?;
        let (w, h) = (img.width(), img.height());
        check_whole_decode(format, w, h, 4, opts.budget())?;
        let (layout, samples) = match img {
            DynamicImage::ImageRgb8(i) => (Layout::Rgb, i.into_raw()),
            DynamicImage::ImageLuma8(i) => (Layout::Gray, i.into_raw()),
            other => (Layout::Rgba, other.to_rgba8().into_raw()),
        };
        let info = SrcInfo::plain(format, w, h, layout, Depth::U8);
        Ok(Self(MemReader::new(info, Samples::U8(samples))))
    }
}

impl RowReader for BmpReader {
    fn info(&self) -> &SrcInfo {
        self.0.info()
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        self.0.read_rows(max_rows, out)
    }
}
