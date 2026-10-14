//! WebP through `image-webp` (pure Rust, lossy and lossless, alpha, animation). The format caps
//! a picture at 16383 pixels per side but not at a memory size, so the decode is checked against
//! the budget first. Animated files report their frame count and decode the first frame.

use std::io::BufReader;

use image_webp::WebPDecoder;

use super::{Input, MemReader, OpenOpts, RowReader, SrcInfo, check_whole_decode};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::orient::orientation_from_exif;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout, Samples};

pub(crate) struct WebpReader(MemReader);

fn map_err(e: image_webp::DecodingError) -> ImageError {
    use image_webp::DecodingError as D;
    match e {
        D::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => ImageError::Corrupt(
            "This WebP is incomplete: the file ends before the picture does. Download or export it again from the original.".into(),
        ),
        D::IoError(e) => ImageError::from(e),
        D::MemoryLimitExceeded => ImageError::TooLarge("This WebP asks for more memory than Panoply allows. Try a smaller image.".into()),
        other => corrupt(other),
    }
}

impl WebpReader {
    pub(crate) fn open(file: Input, opts: &OpenOpts) -> Result<Self> {
        let mut dec = WebPDecoder::new(BufReader::new(file)).map_err(map_err)?;
        let (w, h) = dec.dimensions();
        let has_alpha = dec.has_alpha();
        let animated = dec.is_animated();
        let layout = if has_alpha { Layout::Rgba } else { Layout::Rgb };
        check_whole_decode(Format::WebP, w, h, layout.channels() + 2, opts.budget())?;
        let icc = dec.icc_profile().map_err(map_err)?;
        let exif = dec.exif_metadata().map_err(map_err)?;
        let mut buf = vec![
            0u8;
            dec.output_buffer_size()
                .ok_or_else(|| ImageError::TooLarge(
                    "This WebP is too large to open.".into()
                ))?
        ];
        if animated {
            dec.read_frame(&mut buf).map_err(map_err)?;
        } else {
            dec.read_image(&mut buf).map_err(map_err)?;
        }
        let mut info = SrcInfo::plain(Format::WebP, w, h, layout, Depth::U8);
        info.color.icc = icc;
        if let Some(e) = exif {
            info.orientation = orientation_from_exif(&e);
        }
        info.frames = if animated {
            dec.num_frames() as usize
        } else {
            1
        };
        Ok(Self(MemReader::new(info, Samples::U8(buf))))
    }
}

impl RowReader for WebpReader {
    fn info(&self) -> &SrcInfo {
        self.0.info()
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        self.0.read_rows(max_rows, out)
    }
}
