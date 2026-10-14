//! The row pipeline: decode, color, resample, flatten, depth, sink.
//!
//! [`run_stream`] pulls raw rows from a [`RowReader`] in batches and pushes finished rows into a
//! [`RowSink`]. Nothing here holds more than a batch of rows (plus the resampler's window), so
//! memory does not grow with the picture. [`decode_to_image`] is the same chain ending in an
//! in-memory [`Image`], with orientation applied.

use std::sync::Arc;

use panoply_core::CancelToken;

use crate::color::{BlueNoise, ColorConv, OutSpace};
use crate::decode::{MEMORY_CAP, RowReader, SrcInfo};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::orient;
use crate::resample::{Filter, Resampler};
use crate::types::{Depth, Image, Layout, Rows, Samples};

/// A growable batch of rows of one depth.
#[derive(Debug)]
pub enum RowBuf {
    /// 8 bit samples.
    U8(Vec<u8>),
    /// 16 bit samples.
    U16(Vec<u16>),
}

impl RowBuf {
    /// An empty buffer of this depth.
    pub fn new(depth: Depth) -> Self {
        match depth {
            Depth::U8 => Self::U8(Vec::new()),
            Depth::U16 => Self::U16(Vec::new()),
        }
    }

    /// Borrow as rows.
    pub fn as_rows(&self) -> Rows<'_> {
        match self {
            Self::U8(v) => Rows::U8(v),
            Self::U16(v) => Rows::U16(v),
        }
    }
}

/// Where finished rows go.
pub trait RowSink {
    /// The final geometry, once, before any rows.
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()>;
    /// The next rows, top to bottom.
    fn push(&mut self, rows: Rows<'_>) -> Result<()>;
}

/// Cancel and progress for a long job.
pub struct Ctl<'a> {
    /// Polled between batches.
    pub cancel: &'a CancelToken,
    /// Called with the fraction done of this stream, 0 to 1.
    pub progress: &'a dyn Fn(f32),
}

impl Ctl<'_> {
    pub(crate) fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            Err(ImageError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A [`Ctl`] that never cancels and reports nothing, for tests and small jobs.
pub fn quiet() -> (CancelToken, impl Fn(f32)) {
    (CancelToken::new(), |_| {})
}

/// What the stream should do.
#[derive(Debug, Clone)]
pub struct StreamPlan {
    /// Convert to sRGB, or keep a wide gamut source.
    pub out_space: OutSpace,
    /// Resample to this size, in the reader's stored orientation.
    pub scale: Option<(u32, u32)>,
    /// Kernel; `None` picks one from the direction of the resize.
    pub filter: Option<Filter>,
    /// Composite alpha onto this color and drop the channel.
    pub flatten: Option<[u8; 3]>,
    /// Depth of the rows that reach the sink; `None` keeps the source depth.
    pub out_depth: Option<Depth>,
}

impl Default for StreamPlan {
    fn default() -> Self {
        Self {
            out_space: OutSpace::Srgb,
            scale: None,
            filter: None,
            flatten: None,
            out_depth: None,
        }
    }
}

/// What the stream produced.
#[derive(Debug, Clone)]
pub struct StreamOut {
    /// Width of the rows that reached the sink.
    pub width: u32,
    /// Height of the rows that reached the sink.
    pub height: u32,
    /// Layout of those rows.
    pub layout: Layout,
    /// Depth of those rows.
    pub depth: Depth,
    /// Profile of the pixels when not sRGB.
    pub icc: Option<Arc<Vec<u8>>>,
    /// Lines for the result card ("converted CMYK to sRGB").
    pub notes: Vec<String>,
}

/// Unpremultiply rows in place (OS decoders return premultiplied alpha).
fn unpremultiply(buf: &mut RowBuf, ch: usize) {
    match buf {
        RowBuf::U8(v) => {
            for p in v.chunks_exact_mut(ch) {
                let a = p[ch - 1] as u32;
                if a != 0 && a != 255 {
                    for c in &mut p[..ch - 1] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
        }
        RowBuf::U16(v) => {
            for p in v.chunks_exact_mut(ch) {
                let a = p[ch - 1] as u32;
                if a != 0 && a != 65535 {
                    for c in &mut p[..ch - 1] {
                        *c = ((*c as u32 * 65535 + a / 2) / a).min(65535) as u16;
                    }
                }
            }
        }
    }
}

/// Last stage: flatten, then match the sink's depth.
struct Finisher {
    width: usize,
    in_layout: Layout,
    out_layout: Layout,
    out_depth: Depth,
    flatten: Option<[u8; 3]>,
    flat8: Vec<u8>,
    flat16: Vec<u16>,
    depth8: Vec<u8>,
    depth16: Vec<u16>,
}

impl Finisher {
    fn new(width: usize, in_layout: Layout, flatten: Option<[u8; 3]>, out_depth: Depth) -> Self {
        let mut out_layout = in_layout;
        if let (Some(bg), true) = (flatten, in_layout.has_alpha()) {
            let neutral = bg[0] == bg[1] && bg[1] == bg[2];
            out_layout = if in_layout.is_gray() && neutral {
                Layout::Gray
            } else {
                Layout::Rgb
            };
        }
        Self {
            width,
            in_layout,
            out_layout,
            out_depth,
            flatten: flatten.filter(|_| in_layout.has_alpha()),
            flat8: Vec::new(),
            flat16: Vec::new(),
            depth8: Vec::new(),
            depth16: Vec::new(),
        }
    }

    fn process(&mut self, rows: Rows<'_>, y0: u32, sink: &mut dyn RowSink) -> Result<()> {
        // 1. Flatten.
        let flattened: Rows<'_> = if let Some(bg) = self.flatten {
            let ch = self.in_layout.channels();
            let out_ch = self.out_layout.channels();
            let gray_in = self.in_layout.is_gray();
            match rows {
                Rows::U8(s) => {
                    self.flat8.clear();
                    self.flat8.reserve(s.len() / ch * out_ch);
                    let gbg = ((bg[0] as u32 * 299 + bg[1] as u32 * 587 + bg[2] as u32 * 114 + 500)
                        / 1000) as u8;
                    for p in s.chunks_exact(ch) {
                        let a = p[ch - 1] as u32;
                        let mix = |c: u8, b: u8| {
                            ((c as u32 * a + b as u32 * (255 - a) + 127) / 255) as u8
                        };
                        if gray_in {
                            if out_ch == 1 {
                                self.flat8.push(mix(p[0], gbg));
                            } else {
                                self.flat8.extend_from_slice(&[
                                    mix(p[0], bg[0]),
                                    mix(p[0], bg[1]),
                                    mix(p[0], bg[2]),
                                ]);
                            }
                        } else {
                            self.flat8.extend_from_slice(&[
                                mix(p[0], bg[0]),
                                mix(p[1], bg[1]),
                                mix(p[2], bg[2]),
                            ]);
                        }
                    }
                    Rows::U8(&self.flat8)
                }
                Rows::U16(s) => {
                    self.flat16.clear();
                    self.flat16.reserve(s.len() / ch * out_ch);
                    let b16 = |b: u8| b as u32 * 257;
                    let gbg = (b16(bg[0]) * 299 + b16(bg[1]) * 587 + b16(bg[2]) * 114 + 500) / 1000;
                    for p in s.chunks_exact(ch) {
                        let a = p[ch - 1] as u64;
                        let mix = |c: u16, b: u32| {
                            ((c as u64 * a + b as u64 * (65535 - a) + 32767) / 65535) as u16
                        };
                        if gray_in {
                            if out_ch == 1 {
                                self.flat16.push(mix(p[0], gbg));
                            } else {
                                self.flat16.extend_from_slice(&[
                                    mix(p[0], b16(bg[0])),
                                    mix(p[0], b16(bg[1])),
                                    mix(p[0], b16(bg[2])),
                                ]);
                            }
                        } else {
                            self.flat16.extend_from_slice(&[
                                mix(p[0], b16(bg[0])),
                                mix(p[1], b16(bg[1])),
                                mix(p[2], b16(bg[2])),
                            ]);
                        }
                    }
                    Rows::U16(&self.flat16)
                }
            }
        } else {
            rows
        };
        // 2. Depth.
        let out_ch = self.out_layout.channels();
        let final_rows: Rows<'_> = match (flattened, self.out_depth) {
            (Rows::U8(s), Depth::U8) => Rows::U8(s),
            (Rows::U16(s), Depth::U16) => Rows::U16(s),
            (Rows::U16(s), Depth::U8) => {
                // 16 to 8 bit: blue noise dither, never truncation (IPC-5).
                let bn = BlueNoise::shared();
                let alpha_at = if self.out_layout.has_alpha() {
                    Some(out_ch - 1)
                } else {
                    None
                };
                let width = self.width.max(1);
                self.depth8.clear();
                self.depth8.reserve(s.len());
                for (i, p) in s.chunks_exact(out_ch).enumerate() {
                    let (x, y) = (i % width, y0 as usize + i / width);
                    for (c, &v) in p.iter().enumerate() {
                        if Some(c) == alpha_at {
                            self.depth8.push(((v as u32 + 128) / 257) as u8);
                        } else {
                            let t = bn.at(x + c * 13, y + c * 29);
                            let f = v as f32 / 257.0 + t;
                            self.depth8.push(f.floor().clamp(0.0, 255.0) as u8);
                        }
                    }
                }
                Rows::U8(&self.depth8)
            }
            (Rows::U8(s), Depth::U16) => {
                self.depth16.clear();
                self.depth16.extend(s.iter().map(|&v| v as u16 * 257));
                Rows::U16(&self.depth16)
            }
        };
        sink.push(final_rows)
    }
}

/// Run the whole chain.
pub fn run_stream(
    reader: &mut dyn RowReader,
    plan: &StreamPlan,
    ctl: &Ctl<'_>,
    sink: &mut dyn RowSink,
) -> Result<StreamOut> {
    let info: SrcInfo = reader.info().clone();
    let managed = info.color_managed;
    let conv = if managed {
        None
    } else {
        Some(ColorConv::new(
            info.layout,
            info.depth,
            &info.color,
            plan.out_space,
        )?)
    };
    let work_layout = conv.as_ref().map(|c| c.out_layout).unwrap_or(info.layout);
    let icc = if managed {
        info.managed_icc.clone()
    } else {
        conv.as_ref().and_then(|c| c.out_icc.clone())
    };
    let mut notes = Vec::new();
    if let Some(n) = conv.as_ref().and_then(|c| c.note.clone()) {
        notes.push(n);
    }
    let (out_w, out_h) = plan.scale.unwrap_or((info.width, info.height));
    let work_depth = info.depth;
    let out_depth = plan.out_depth.unwrap_or(work_depth);
    let mut fin = Finisher::new(out_w as usize, work_layout, plan.flatten, out_depth);
    sink.begin(out_w, out_h, fin.out_layout, out_depth, icc.clone())?;

    let mut resampler = match plan.scale {
        Some((w, h)) if (w, h) != (info.width, info.height) => {
            let f = plan
                .filter
                .unwrap_or_else(|| Filter::auto(info.width, info.height, w, h));
            Some(Resampler::new(
                info.width,
                info.height,
                w,
                h,
                work_layout,
                work_depth,
                f,
            )?)
        }
        _ => None,
    };

    let raw_row_bytes = info.raw_row_len() * if info.depth == Depth::U16 { 2 } else { 1 };
    let batch = (4 * 1024 * 1024 / raw_row_bytes.max(1)).clamp(8, 256);
    let mut raw = RowBuf::new(info.depth);
    let mut work = RowBuf::new(work_depth);
    let mut done = 0usize;
    let mut out_y = 0u32;
    let total = info.height as usize;
    while done < total {
        ctl.check()?;
        let n = reader.read_rows(batch.min(total - done), &mut raw)?;
        if n == 0 {
            return Err(crate::error::corrupt("the picture ends early"));
        }
        if info.premultiplied && work_layout.has_alpha() && conv.is_none() {
            unpremultiply(&mut raw, work_layout.channels());
        }
        let rows: Rows<'_> = match &conv {
            Some(c) if !c.is_identity() => {
                c.convert(raw.as_rows(), &mut work)?;
                work.as_rows()
            }
            _ => raw.as_rows(),
        };
        match &mut resampler {
            Some(rs) => {
                rs.push(rows, &mut |out_rows, y0, _n| {
                    fin.process(out_rows, y0, sink)
                })?;
            }
            None => {
                fin.process(rows, out_y, sink)?;
                out_y += n as u32;
            }
        }
        done += n;
        (ctl.progress)(done as f32 / total as f32);
    }
    if let Some(rs) = &resampler {
        rs.finish()?;
    }
    Ok(StreamOut {
        width: out_w,
        height: out_h,
        layout: fin.out_layout,
        depth: out_depth,
        icc,
        notes,
    })
}

// ---------------------------------------------------------------- sinks

/// Collects rows into an [`Image`].
#[derive(Default)]
pub struct ImageSink {
    image: Option<Image>,
    filled: usize,
}

impl ImageSink {
    /// A new empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the finished image.
    pub fn into_image(self) -> Result<Image> {
        match self.image {
            Some(img) if self.filled == img.samples.len() => Ok(img),
            _ => Err(ImageError::Failed(
                "internal: the picture was not completely written".into(),
            )),
        }
    }
}

impl RowSink for ImageSink {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        let need = width as usize
            * height as usize
            * layout.channels()
            * if depth == Depth::U16 { 2 } else { 1 };
        if need > MEMORY_CAP {
            return Err(ImageError::TooLarge(format!(
                "The result would be {width} x {height} pixels, about {} MB, which is over the {} MB limit. Choose a smaller size.",
                need / (1024 * 1024),
                MEMORY_CAP / (1024 * 1024)
            )));
        }
        let mut img = Image::new(width, height, layout, depth);
        img.icc = icc;
        self.image = Some(img);
        self.filled = 0;
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let img = self.image.as_mut().expect("begin was called");
        match (&mut img.samples, rows) {
            (Samples::U8(d), Rows::U8(s)) => {
                d[self.filled..self.filled + s.len()].copy_from_slice(s)
            }
            (Samples::U16(d), Rows::U16(s)) => {
                d[self.filled..self.filled + s.len()].copy_from_slice(s)
            }
            _ => return Err(ImageError::Failed("internal: sink depth mismatch".into())),
        }
        self.filled += rows.len();
        Ok(())
    }
}

/// Mirrors each row left to right (EXIF orientation 2) on its way to another sink.
pub struct FlipSink<'a> {
    /// The sink that receives the mirrored rows.
    pub inner: &'a mut dyn RowSink,
    layout: Layout,
    width: usize,
    b8: Vec<u8>,
    b16: Vec<u16>,
}

impl<'a> FlipSink<'a> {
    /// Wrap a sink.
    pub fn new(inner: &'a mut dyn RowSink) -> Self {
        Self {
            inner,
            layout: Layout::Rgb,
            width: 0,
            b8: Vec::new(),
            b16: Vec::new(),
        }
    }
}

impl RowSink for FlipSink<'_> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        self.layout = layout;
        self.width = width as usize;
        self.inner.begin(width, height, layout, depth, icc)
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let ch = self.layout.channels();
        let rl = self.width * ch;
        match rows {
            Rows::U8(s) => {
                self.b8.clear();
                for row in s.chunks_exact(rl) {
                    for px in row.chunks_exact(ch).rev() {
                        self.b8.extend_from_slice(px);
                    }
                }
                self.inner.push(Rows::U8(&self.b8))
            }
            Rows::U16(s) => {
                self.b16.clear();
                for row in s.chunks_exact(rl) {
                    for px in row.chunks_exact(ch).rev() {
                        self.b16.extend_from_slice(px);
                    }
                }
                self.inner.push(Rows::U16(&self.b16))
            }
        }
    }
}

// ---------------------------------------------------------------- whole image

/// What to build when decoding into memory.
#[derive(Debug, Clone)]
pub struct ImagePlan {
    /// Convert to sRGB, or keep a wide gamut source.
    pub out_space: OutSpace,
    /// Resample to this size, in UPRIGHT orientation.
    pub scale: Option<(u32, u32)>,
    /// Kernel; `None` picks one from the direction of the resize.
    pub filter: Option<Filter>,
    /// Composite alpha onto this color and drop the channel.
    pub flatten: Option<[u8; 3]>,
    /// Depth of the result; `None` keeps the source depth.
    pub out_depth: Option<Depth>,
}

impl Default for ImagePlan {
    fn default() -> Self {
        Self {
            out_space: OutSpace::Srgb,
            scale: None,
            filter: None,
            flatten: None,
            out_depth: None,
        }
    }
}

/// Decode a whole picture into memory: color managed, resampled, orientation applied.
pub fn decode_to_image(
    reader: &mut dyn RowReader,
    plan: &ImagePlan,
    ctl: &Ctl<'_>,
) -> Result<(Image, StreamOut)> {
    let info = reader.info().clone();
    let o = if info.upright { 1 } else { info.orientation };
    let stored_scale = plan.scale.map(|(w, h)| {
        if orient::swaps_dims(o) {
            (h, w)
        } else {
            (w, h)
        }
    });
    let sp = StreamPlan {
        out_space: plan.out_space,
        scale: stored_scale,
        filter: plan.filter,
        flatten: plan.flatten,
        out_depth: plan.out_depth,
    };
    let mut sink = ImageSink::new();
    let mut out = run_stream(reader, &sp, ctl, &mut sink)?;
    let img = orient::apply(sink.into_image()?, o);
    out.width = img.width;
    out.height = img.height;
    Ok((img, out))
}

/// A reader over an image already in memory, so the resampler and the color stages can be
/// reused on it. Rows are color managed and keep the image's profile.
pub struct ImageRows<'a> {
    info: SrcInfo,
    img: &'a Image,
    next: u32,
}

impl<'a> ImageRows<'a> {
    /// Read rows of `img` from the top.
    pub fn new(img: &'a Image) -> Self {
        let mut info = SrcInfo::plain(Format::Png, img.width, img.height, img.layout, img.depth());
        info.color_managed = true;
        info.managed_icc = img.icc.clone();
        info.upright = true;
        Self { info, img, next: 0 }
    }
}

impl RowReader for ImageRows<'_> {
    fn info(&self) -> &SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        let n = (max_rows as u32).min(self.img.height - self.next);
        match (self.img.rows(self.next, n), out) {
            (Rows::U8(s), RowBuf::U8(d)) => {
                d.clear();
                d.extend_from_slice(s);
            }
            (Rows::U16(s), RowBuf::U16(d)) => {
                d.clear();
                d.extend_from_slice(s);
            }
            _ => {
                return Err(ImageError::Failed(
                    "internal: row buffer depth mismatch".into(),
                ));
            }
        }
        self.next += n;
        Ok(n as usize)
    }
}
