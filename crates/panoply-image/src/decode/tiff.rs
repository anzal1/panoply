//! TIFF through the `tiff` crate, one strip (or one row of tiles) at a time. Gray, gray with
//! alpha, RGB, RGBA, CMYK and palette images at 1 to 16 bits. Subsampled YCbCr, planar and
//! floating point TIFFs are refused here (the OS decoder takes them on macOS).

use std::collections::VecDeque;
use std::io::BufReader;

use tiff::ColorType;
use tiff::decoder::{ChunkType, Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

use super::{Input, OpenOpts, RowReader, SrcInfo};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout};

enum Kind {
    /// Samples are already in raw row form.
    Direct,
    /// Low bit depth gray: values 0..=max, scale to 8 bit.
    LowGray { max: u32, invert: bool },
    /// Palette: expand through the color map.
    Palette { map: Vec<u16>, entries: usize },
}

pub(crate) struct TiffReader {
    info: SrcInfo,
    dec: Decoder<BufReader<Input>>,
    kind: Kind,
    chunk_type: ChunkType,
    next_chunk: u32,
    chunk_count: u32,
    tiles_across: u32,
    tile_w: u32,
    tile_h: u32,
    /// Decoded rows waiting to be handed out, as 8 or 16 bit samples.
    q8: VecDeque<u8>,
    q16: VecDeque<u16>,
    src_samples: usize,
    rows_done: u32,
}

fn map_err(e: tiff::TiffError) -> ImageError {
    use tiff::TiffError as T;
    match e {
        T::UnsupportedError(u) => ImageError::Unsupported(format!(
            "This TIFF uses a feature Panoply cannot read yet ({u}). Re-save it as a plain TIFF or PNG and try again."
        )),
        T::LimitsExceeded => ImageError::TooLarge(
            "This TIFF asks for more memory than Panoply allows. Try a smaller image.".into(),
        ),
        T::IoError(e) => ImageError::from(e),
        other => corrupt(other),
    }
}

impl TiffReader {
    pub(crate) fn open(file: Input, _opts: &OpenOpts) -> Result<Self> {
        let mut dec = Decoder::new(BufReader::with_capacity(1 << 16, file)).map_err(map_err)?;
        let mut limits = Limits::default();
        limits.decoding_buffer_size = 128 * 1024 * 1024;
        dec = dec.with_limits(limits);
        let (w, h) = dec.dimensions().map_err(map_err)?;
        let ct = dec.colortype().map_err(map_err)?;
        let planar = dec
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
            .ok()
            .flatten()
            .unwrap_or(1);
        if planar == 2 {
            return Err(ImageError::Unsupported("This TIFF stores its colors in separate planes, which Panoply cannot read yet. Re-save it as a plain TIFF or PNG.".into()));
        }
        let photometric = dec
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .ok()
            .flatten()
            .unwrap_or(2);
        let (layout, depth, cmyk, kind) = match ct {
            ColorType::Gray(8) => (Layout::Gray, Depth::U8, false, Kind::Direct),
            ColorType::Gray(16) => (Layout::Gray, Depth::U16, false, Kind::Direct),
            ColorType::Gray(b @ 1..=7) => (
                Layout::Gray,
                Depth::U8,
                false,
                Kind::LowGray {
                    max: (1u32 << b) - 1,
                    invert: photometric == 0,
                },
            ),
            ColorType::GrayA(8) => (Layout::GrayAlpha, Depth::U8, false, Kind::Direct),
            ColorType::GrayA(16) => (Layout::GrayAlpha, Depth::U16, false, Kind::Direct),
            ColorType::RGB(8) => (Layout::Rgb, Depth::U8, false, Kind::Direct),
            ColorType::RGB(16) => (Layout::Rgb, Depth::U16, false, Kind::Direct),
            ColorType::RGBA(8) => (Layout::Rgba, Depth::U8, false, Kind::Direct),
            ColorType::RGBA(16) => (Layout::Rgba, Depth::U16, false, Kind::Direct),
            ColorType::CMYK(8) => (Layout::Rgba, Depth::U8, true, Kind::Direct),
            ColorType::CMYK(16) => (Layout::Rgba, Depth::U16, true, Kind::Direct),
            ColorType::Palette(b) if b <= 8 => {
                let map = dec.get_tag_u16_vec(Tag::ColorMap).map_err(map_err)?;
                let entries = 1usize << b;
                if map.len() < entries * 3 {
                    return Err(corrupt("short color map"));
                }
                (
                    Layout::Rgb,
                    Depth::U8,
                    false,
                    Kind::Palette { map, entries },
                )
            }
            other => {
                return Err(ImageError::Unsupported(format!(
                    "This TIFF stores pixels as {other:?}, which Panoply cannot read yet. Re-save it as a plain TIFF or PNG."
                )));
            }
        };
        let src_samples = match &kind {
            Kind::Palette { .. } => 1,
            Kind::LowGray { .. } => 1,
            Kind::Direct => {
                if cmyk {
                    4
                } else {
                    layout.channels()
                }
            }
        };
        let mut info = SrcInfo::plain(Format::Tiff, w, h, layout, depth);
        info.color.cmyk = cmyk;
        if cmyk {
            info.color_label = Some("CMYK".into());
        }
        info.color.icc = dec
            .get_tag_u8_vec(Tag::IccProfile)
            .ok()
            .filter(|v| !v.is_empty());
        if let Some(o) = dec
            .find_tag_unsigned::<u16>(Tag::Orientation)
            .ok()
            .flatten()
            && (1..=8).contains(&o)
        {
            info.orientation = o as u8;
        }
        // A photometric of 0 (WhiteIsZero) is inverted by the crate for 8 and 16 bit gray.
        let chunk_type = dec.get_chunk_type();
        let (chunk_count, tiles_across, tile_w, tile_h) = match chunk_type {
            ChunkType::Strip => (dec.strip_count().map_err(map_err)?, 0, 0, 0),
            ChunkType::Tile => {
                let (tw, th) = dec.chunk_dimensions();
                (dec.tile_count().map_err(map_err)?, w.div_ceil(tw), tw, th)
            }
        };
        Ok(Self {
            info,
            dec,
            kind,
            chunk_type,
            next_chunk: 0,
            chunk_count,
            tiles_across,
            tile_w,
            tile_h,
            q8: VecDeque::new(),
            q16: VecDeque::new(),
            src_samples,
            rows_done: 0,
        })
    }

    /// Decode the next strip or row of tiles and queue its rows.
    fn fill(&mut self) -> Result<()> {
        if self.next_chunk >= self.chunk_count {
            return Err(corrupt("missing strips"));
        }
        let w = self.info.width as usize;
        match self.chunk_type {
            ChunkType::Strip => {
                let i = self.next_chunk;
                let data = self.dec.read_chunk(i).map_err(map_err)?;
                self.next_chunk += 1;
                self.push_chunk(data, w);
            }
            ChunkType::Tile => {
                // One row of tiles: decode each and interleave into full width rows.
                let row = self.next_chunk / self.tiles_across;
                let first = row * self.tiles_across;
                let band_h = (self.info.height - row * self.tile_h).min(self.tile_h) as usize;
                let sp = self.src_samples;
                let wide = self.info.depth == Depth::U16 && matches!(self.kind, Kind::Direct);
                let mut b8 = vec![0u8; if wide { 0 } else { w * band_h * sp }];
                let mut b16 = vec![0u16; if wide { w * band_h * sp } else { 0 }];
                for t in 0..self.tiles_across {
                    let idx = first + t;
                    let (tw, th) = self.dec.chunk_data_dimensions(idx);
                    let data = self.dec.read_chunk(idx).map_err(map_err)?;
                    let x0 = (t * self.tile_w) as usize;
                    match data {
                        DecodingResult::U8(v) if !wide => {
                            for r in 0..th as usize {
                                let s = &v[r * tw as usize * sp..(r + 1) * tw as usize * sp];
                                b8[(r * w + x0) * sp..(r * w + x0 + tw as usize) * sp]
                                    .copy_from_slice(s);
                            }
                        }
                        DecodingResult::U16(v) if wide => {
                            for r in 0..th as usize {
                                let s = &v[r * tw as usize * sp..(r + 1) * tw as usize * sp];
                                b16[(r * w + x0) * sp..(r * w + x0 + tw as usize) * sp]
                                    .copy_from_slice(s);
                            }
                        }
                        _ => {
                            return Err(ImageError::Unsupported(
                                "This TIFF stores its samples in a form Panoply cannot read yet."
                                    .into(),
                            ));
                        }
                    }
                }
                self.next_chunk += self.tiles_across;
                if wide {
                    self.push_chunk(DecodingResult::U16(b16), w);
                } else {
                    self.push_chunk(DecodingResult::U8(b8), w);
                }
            }
        }
        Ok(())
    }

    fn push_chunk(&mut self, data: DecodingResult, _w: usize) {
        match (data, &self.kind) {
            (DecodingResult::U8(v), Kind::Direct) => self.q8.extend(v),
            (DecodingResult::U16(v), Kind::Direct) => self.q16.extend(v),
            (DecodingResult::U8(v), Kind::LowGray { max, invert }) => {
                self.q8.extend(v.iter().map(|&s| {
                    let x = (s as u32).min(*max);
                    let g = (x * 255 / max) as u8;
                    if *invert { 255 - g } else { g }
                }));
            }
            (DecodingResult::U8(v), Kind::Palette { map, entries }) => {
                for &i in &v {
                    let i = (i as usize).min(entries - 1);
                    self.q8.extend([
                        (map[i] >> 8) as u8,
                        (map[entries + i] >> 8) as u8,
                        (map[2 * entries + i] >> 8) as u8,
                    ]);
                }
            }
            _ => {}
        }
    }
}

impl RowReader for TiffReader {
    fn info(&self) -> &SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        let left = (self.info.height - self.rows_done) as usize;
        let n = max_rows.min(left);
        if n == 0 {
            return Ok(0);
        }
        let per_row = self.info.raw_row_len();
        match out {
            RowBuf::U8(buf) => {
                buf.clear();
                while self.q8.len() < n * per_row {
                    self.fill()?;
                }
                buf.extend(self.q8.drain(..n * per_row));
            }
            RowBuf::U16(buf) => {
                buf.clear();
                while self.q16.len() < n * per_row {
                    self.fill()?;
                }
                buf.extend(self.q16.drain(..n * per_row));
            }
        }
        self.rows_done += n as u32;
        Ok(n)
    }
}
