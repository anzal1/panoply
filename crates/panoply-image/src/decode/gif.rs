//! GIF through the `gif` crate. The first frame is composed onto a transparent canvas the size
//! of the logical screen. More than one frame is reported through `SrcInfo::frames`, and the
//! caller decides whether to refuse or to use the first frame.

use std::io::BufReader;

use gif::{ColorOutput, DecodeOptions};

use super::{Input, MemReader, OpenOpts, RowReader, SrcInfo, check_whole_decode};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout, Samples};

pub(crate) struct GifReader(MemReader);

fn map_err(e: gif::DecodingError) -> ImageError {
    match e {
        gif::DecodingError::Io(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => ImageError::Corrupt(
            "This GIF is incomplete: the file ends before the picture does. Download or export it again from the original.".into(),
        ),
        gif::DecodingError::Io(e) => ImageError::from(e),
        other => corrupt(other),
    }
}

impl GifReader {
    pub(crate) fn open(file: Input, opts: &OpenOpts) -> Result<Self> {
        let mut o = DecodeOptions::new();
        o.set_color_output(ColorOutput::RGBA);
        let mut dec = o.read_info(BufReader::new(file)).map_err(map_err)?;
        let (w, h) = (dec.width() as u32, dec.height() as u32);
        check_whole_decode(Format::Gif, w, h, 4, opts.budget())?;
        let icc = dec.icc_profile().map(|p| p.to_vec());
        let mut canvas = vec![0u8; w as usize * h as usize * 4];
        let frame = dec
            .read_next_frame()
            .map_err(map_err)?
            .ok_or_else(|| corrupt("no frames"))?;
        let (fl, ft, fw, fh) = (
            frame.left as usize,
            frame.top as usize,
            frame.width as usize,
            frame.height as usize,
        );
        for y in 0..fh.min((h as usize).saturating_sub(ft)) {
            let cw = fw.min((w as usize).saturating_sub(fl));
            let src = &frame.buffer[y * fw * 4..y * fw * 4 + cw * 4];
            let d = ((ft + y) * w as usize + fl) * 4;
            canvas[d..d + cw * 4].copy_from_slice(src);
        }
        let more = dec.next_frame_info().map_err(map_err)?.is_some();
        let mut info = SrcInfo::plain(Format::Gif, w, h, Layout::Rgba, Depth::U8);
        info.color.icc = icc;
        info.frames = if more { 2 } else { 1 };
        Ok(Self(MemReader::new(info, Samples::U8(canvas))))
    }
}

impl RowReader for GifReader {
    fn info(&self) -> &SrcInfo {
        self.0.info()
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        self.0.read_rows(max_rows, out)
    }
}
