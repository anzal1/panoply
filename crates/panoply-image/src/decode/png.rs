//! PNG through the `png` crate: streams rows, keeps 16 bit, reads the profile, the gamma and
//! the EXIF orientation. Interlaced files decode whole (the format has no row order to stream),
//! so they are checked against the memory budget first.

use std::io::BufReader;

use png::{BitDepth, ColorType, Decoder, Limits, Transformations};

use super::{Input, OpenOpts, RowReader, SrcInfo, check_whole_decode};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::orient::orientation_from_exif;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout};

pub(crate) struct PngReader {
    info: SrcInfo,
    reader: png::Reader<BufReader<Input>>,
    /// Whole decoded image for interlaced files (raw bytes as the decoder wrote them).
    whole: Option<Vec<u8>>,
    rows_done: usize,
    bytes_per_row: usize,
}

fn map_err(e: png::DecodingError) -> ImageError {
    use png::DecodingError as D;
    match e {
        D::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => ImageError::Corrupt(
            "This PNG is incomplete: the file ends before the picture does. Download or export it again from the original.".into(),
        ),
        D::IoError(e) => ImageError::from(e),
        D::LimitsExceeded => ImageError::TooLarge(
            "This PNG asks for more memory than Panoply allows. Try a smaller image.".into(),
        ),
        other => corrupt(other),
    }
}

impl PngReader {
    pub(crate) fn open(file: Input, opts: &OpenOpts) -> Result<Self> {
        let mut decoder = Decoder::new_with_limits(
            BufReader::with_capacity(1 << 16, file),
            Limits {
                bytes: 128 * 1024 * 1024,
            },
        );
        decoder.set_transformations(Transformations::EXPAND);
        let reader = decoder.read_info().map_err(map_err)?;
        let pi = reader.info();
        let (ct, bd) = reader.output_color_type();
        let layout = match ct {
            ColorType::Grayscale => Layout::Gray,
            ColorType::GrayscaleAlpha => Layout::GrayAlpha,
            ColorType::Rgb => Layout::Rgb,
            ColorType::Rgba => Layout::Rgba,
            ColorType::Indexed => return Err(corrupt("unexpanded palette")),
        };
        let depth = match bd {
            BitDepth::Sixteen => Depth::U16,
            _ => Depth::U8,
        };
        let (w, h) = (pi.width, pi.height);
        let mut info = SrcInfo::plain(Format::Png, w, h, layout, depth);
        info.color.icc = pi.icc_profile.as_ref().map(|c| c.to_vec());
        if pi.icc_profile.is_none() && pi.srgb.is_none() {
            info.color.png_gamma = pi.source_gamma.map(|g| g.into_value());
        }
        if let Some(ex) = &pi.exif_metadata {
            info.orientation = orientation_from_exif(ex);
        }
        if let Some(ac) = pi.animation_control {
            info.frames = ac.num_frames as usize;
        }
        let interlaced = pi.interlaced;
        let bytes_per_row = reader
            .output_line_size(w)
            .ok_or_else(|| ImageError::TooLarge("This PNG is too wide to open.".into()))?;
        let mut whole = None;
        let mut reader = reader;
        if interlaced {
            check_whole_decode(
                Format::Png,
                w,
                h,
                layout.channels() * if depth == Depth::U16 { 2 } else { 1 },
                opts.budget(),
            )?;
            let mut buf = vec![
                0u8;
                reader
                    .output_buffer_size()
                    .ok_or_else(|| ImageError::TooLarge(
                        "This PNG is too large to open.".into()
                    ))?
            ];
            reader.next_frame(&mut buf).map_err(map_err)?;
            whole = Some(buf);
        }
        Ok(Self {
            info,
            reader,
            whole,
            rows_done: 0,
            bytes_per_row,
        })
    }
}

impl RowReader for PngReader {
    fn info(&self) -> &SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        let left = self.info.height as usize - self.rows_done;
        let n = max_rows.min(left);
        if n == 0 {
            return Ok(0);
        }
        let samples_per_row = self.info.raw_row_len();
        match out {
            RowBuf::U8(buf) => {
                buf.clear();
                buf.reserve(n * samples_per_row);
                for i in 0..n {
                    if let Some(whole) = &self.whole {
                        let r = self.rows_done + i;
                        buf.extend_from_slice(
                            &whole[r * self.bytes_per_row..(r + 1) * self.bytes_per_row],
                        );
                    } else {
                        let row = self
                            .reader
                            .next_row()
                            .map_err(map_err)?
                            .ok_or_else(|| corrupt("missing rows"))?;
                        buf.extend_from_slice(row.data());
                    }
                }
            }
            RowBuf::U16(buf) => {
                buf.clear();
                buf.reserve(n * samples_per_row);
                for i in 0..n {
                    let bytes: &[u8] = if let Some(whole) = &self.whole {
                        let r = self.rows_done + i;
                        &whole[r * self.bytes_per_row..(r + 1) * self.bytes_per_row]
                    } else {
                        self.reader
                            .next_row()
                            .map_err(map_err)?
                            .ok_or_else(|| corrupt("missing rows"))?
                            .data()
                    };
                    buf.extend(
                        bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|b| u16::from_be_bytes([b[0], b[1]])),
                    );
                }
            }
        }
        self.rows_done += n;
        Ok(n)
    }
}
