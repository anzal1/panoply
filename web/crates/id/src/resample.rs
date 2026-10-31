//! Streaming resampling in linear light (IPC-2 and IPC-8).
//!
//! Rows go in as they decode. Each batch is brought to linear light (16 bit), alpha is
//! premultiplied, the width is resampled with `fast_image_resize` (SIMD convolution), and the
//! height is resampled here with a sliding window of rows, so a 20000 by 20000 image needs
//! memory for a few hundred rows, not for the picture. Finished rows come back as sRGB
//! encoded samples with alpha restored, ready for the next stage.
//!
//! Filters: Lanczos3 when shrinking (sharp, the libvips default), Mitchell when enlarging
//! (no ringing on the stretched edges).
//!
//! Ringing: a Lanczos kernel overshoots next to a hard edge (about 4 percent of the step), which
//! shows as a halo. Every output sample is therefore limited to the range of the source samples
//! under the kernel's main lobe, in both passes. That removes the halo and leaves everything else
//! the kernel does, sharpening included, untouched.

use std::collections::VecDeque;

use fast_image_resize as fir;
use fir::images::{TypedImage, TypedImageRef};
use fir::pixels::{U16, U16x2, U16x3, U16x4};
use fir::{FilterType, ResizeAlg, ResizeOptions, Resizer};

use crate::color::luts;
use crate::error::{ImageError, Result};
use crate::image::{Depth, Layout, Rows};

/// Which kernel to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// Sharp, for shrinking.
    Lanczos3,
    /// Soft, for enlarging.
    Mitchell,
}

impl Filter {
    /// Lanczos3 when shrinking in either direction, Mitchell when only enlarging.
    pub fn auto(in_w: u32, in_h: u32, out_w: u32, out_h: u32) -> Filter {
        if out_w <= in_w || out_h <= in_h {
            Filter::Lanczos3
        } else {
            Filter::Mitchell
        }
    }

    fn support(self) -> f64 {
        match self {
            Filter::Lanczos3 => 3.0,
            Filter::Mitchell => 2.0,
        }
    }

    fn eval(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Filter::Lanczos3 => {
                if x >= 3.0 {
                    0.0
                } else if x < 1e-9 {
                    1.0
                } else {
                    let px = std::f64::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                }
            }
            Filter::Mitchell => {
                // B = C = 1/3
                const B: f64 = 1.0 / 3.0;
                const C: f64 = 1.0 / 3.0;
                if x < 1.0 {
                    ((12.0 - 9.0 * B - 6.0 * C) * x * x * x
                        + (-18.0 + 12.0 * B + 6.0 * C) * x * x
                        + (6.0 - 2.0 * B))
                        / 6.0
                } else if x < 2.0 {
                    ((-B - 6.0 * C) * x * x * x
                        + (6.0 * B + 30.0 * C) * x * x
                        + (-12.0 * B - 48.0 * C) * x
                        + (8.0 * B + 24.0 * C))
                        / 6.0
                } else {
                    0.0
                }
            }
        }
    }

    fn fir(self) -> FilterType {
        match self {
            Filter::Lanczos3 => FilterType::Lanczos3,
            Filter::Mitchell => FilterType::Mitchell,
        }
    }
}

/// Weights for one output row: source rows `first .. first + weights.len()`.
struct VRow {
    first: usize,
    weights: Vec<f32>,
    /// Weights `core_lo..core_hi` are the rows under the kernel's main lobe.
    core_lo: usize,
    core_hi: usize,
}

/// Source samples `lo..hi` under the main lobe of the kernel for the output sample centered
/// at `center` (in source units), with `fs` source samples per kernel unit.
fn core_range(center: f64, fs: f64, len: usize) -> (usize, usize) {
    let lo = ((center - fs - 0.5).floor().max(0.0)) as usize;
    let hi = (((center + fs + 0.5).ceil()) as usize).min(len).max(lo + 1);
    (lo.min(len - 1), hi)
}

fn vertical_weights(in_h: u32, out_h: u32, filter: Filter) -> Vec<VRow> {
    let scale = in_h as f64 / out_h as f64;
    let fs = scale.max(1.0);
    let support = filter.support() * fs;
    (0..out_h as usize)
        .map(|j| {
            let center = (j as f64 + 0.5) * scale;
            let lo = ((center - support).floor().max(0.0)) as usize;
            let hi = (((center + support).ceil()) as usize)
                .min(in_h as usize)
                .max(lo + 1);
            let mut w: Vec<f64> = (lo..hi)
                .map(|i| filter.eval(((i as f64 + 0.5) - center) / fs))
                .collect();
            let sum: f64 = w.iter().sum();
            if sum.abs() > 1e-12 {
                w.iter_mut().for_each(|v| *v /= sum);
            } else {
                // Degenerate: fall back to the nearest source row.
                let n = (center.floor() as usize).clamp(lo, hi - 1);
                w.iter_mut().for_each(|v| *v = 0.0);
                w[n - lo] = 1.0;
            }
            let (cl, ch) = core_range(center, fs, in_h as usize);
            let (cl, ch) = (cl.clamp(lo, hi - 1), ch.clamp(lo + 1, hi));
            VRow {
                first: lo,
                weights: w.into_iter().map(|v| v as f32).collect(),
                core_lo: cl - lo,
                core_hi: ch - lo,
            }
        })
        .collect()
}

fn horizontal_core(in_w: u32, out_w: u32) -> Vec<(u32, u32)> {
    let scale = in_w as f64 / out_w as f64;
    let fs = scale.max(1.0);
    (0..out_w as usize)
        .map(|j| {
            let (lo, hi) = core_range((j as f64 + 0.5) * scale, fs, in_w as usize);
            (lo as u32, hi as u32)
        })
        .collect()
}

/// The streaming resampler. Create it for one image, push rows, collect rows.
pub struct Resampler {
    in_w: u32,
    in_h: u32,
    out_w: u32,
    out_h: u32,
    layout: Layout,
    depth: Depth,
    ch: usize,
    filter: Filter,
    resizer: Resizer,
    vweights: Vec<VRow>,
    /// For each output column, the source columns under the kernel's main lobe.
    hcore: Vec<(u32, u32)>,
    /// Horizontally reduced linear rows, starting at source row `base`.
    window: VecDeque<Vec<u16>>,
    spare: Vec<Vec<u16>>,
    base: usize,
    received: usize,
    next_out: usize,
    /// Scratch for converting a batch.
    lin: Vec<u16>,
    reduced: Vec<u16>,
    acc: Vec<f32>,
    lo_buf: Vec<u16>,
    hi_buf: Vec<u16>,
    out8: Vec<u8>,
    out16: Vec<u16>,
}

/// Rows emitted at once.
const EMIT_ROWS: usize = 64;

impl Resampler {
    /// A resampler from `in_w x in_h` to `out_w x out_h` for rows of this layout and depth.
    pub fn new(
        in_w: u32,
        in_h: u32,
        out_w: u32,
        out_h: u32,
        layout: Layout,
        depth: Depth,
        filter: Filter,
    ) -> Result<Self> {
        if in_w == 0 || in_h == 0 || out_w == 0 || out_h == 0 {
            return Err(ImageError::BadOption(
                "The picture or the new size is empty.".into(),
            ));
        }
        Ok(Self {
            in_w,
            in_h,
            out_w,
            out_h,
            layout,
            depth,
            ch: layout.channels(),
            filter,
            resizer: Resizer::new(),
            vweights: vertical_weights(in_h, out_h, filter),
            hcore: horizontal_core(in_w, out_w),
            window: VecDeque::new(),
            spare: Vec::new(),
            base: 0,
            received: 0,
            next_out: 0,
            lin: Vec::new(),
            reduced: Vec::new(),
            acc: vec![0.0; out_w as usize * layout.channels()],
            lo_buf: Vec::new(),
            hi_buf: Vec::new(),
            out8: Vec::new(),
            out16: Vec::new(),
        })
    }

    /// Bring a batch of rows to premultiplied linear 16 bit.
    fn load_linear(&mut self, rows: Rows<'_>) {
        let l = luts();
        let ch = self.ch;
        let alpha = self.layout.has_alpha();
        self.lin.clear();
        match rows {
            Rows::U8(s) => {
                self.lin.reserve(s.len());
                if alpha {
                    for p in s.chunks_exact(ch) {
                        let a = p[ch - 1] as u32 * 257;
                        for &c in &p[..ch - 1] {
                            let v = l.s8_to_lin16[c as usize] as u32;
                            self.lin.push(((v * a + 32767) / 65535) as u16);
                        }
                        self.lin.push(a as u16);
                    }
                } else {
                    self.lin
                        .extend(s.iter().map(|&c| l.s8_to_lin16[c as usize]));
                }
            }
            Rows::U16(s) => {
                self.lin.reserve(s.len());
                if alpha {
                    for p in s.chunks_exact(ch) {
                        let a = p[ch - 1] as u32;
                        for &c in &p[..ch - 1] {
                            let v = l.s16_to_lin16[c as usize] as u32;
                            self.lin.push(((v * a + 32767) / 65535) as u16);
                        }
                        self.lin.push(a as u16);
                    }
                } else {
                    self.lin
                        .extend(s.iter().map(|&c| l.s16_to_lin16[c as usize]));
                }
            }
        }
    }

    /// Resample the width of `self.lin` (n rows) into `self.reduced`.
    fn horizontal(&mut self, n: usize) -> Result<()> {
        let (iw, ow) = (self.in_w as usize, self.out_w as usize);
        if iw == ow {
            std::mem::swap(&mut self.lin, &mut self.reduced);
            return Ok(());
        }
        self.reduced.clear();
        self.reduced.resize(ow * n * self.ch, 0);
        let opts = ResizeOptions::new()
            .resize_alg(ResizeAlg::Convolution(self.filter.fir()))
            .use_alpha(false);
        macro_rules! run {
            ($px:ty) => {{
                let src: &[$px] = bytemuck::cast_slice(&self.lin[..]);
                let dst: &mut [$px] = bytemuck::cast_slice_mut(&mut self.reduced[..]);
                let src_img = TypedImageRef::<$px>::new(iw as u32, n as u32, src)
                    .map_err(|_| internal("source size"))?;
                let mut dst_img = TypedImage::<$px>::from_pixels_slice(ow as u32, n as u32, dst)
                    .map_err(|_| internal("target size"))?;
                self.resizer
                    .resize_typed(&src_img, &mut dst_img, &opts)
                    .map_err(|e| internal(&format!("{e}")))?;
            }};
        }
        match self.ch {
            1 => run!(U16),
            2 => run!(U16x2),
            3 => run!(U16x3),
            _ => run!(U16x4),
        }
        // Limit every output sample to the range under the kernel's main lobe (no halos).
        let ch = self.ch;
        for r in 0..n {
            let src = &self.lin[r * iw * ch..(r + 1) * iw * ch];
            let dst = &mut self.reduced[r * ow * ch..(r + 1) * ow * ch];
            for (j, &(lo, hi)) in self.hcore.iter().enumerate() {
                for c in 0..ch {
                    let (mut mn, mut mx) = (u16::MAX, 0u16);
                    for i in lo as usize..hi as usize {
                        let v = src[i * ch + c];
                        mn = mn.min(v);
                        mx = mx.max(v);
                    }
                    let o = &mut dst[j * ch + c];
                    *o = (*o).clamp(mn, mx);
                }
            }
        }
        Ok(())
    }

    /// Feed rows (straight alpha, sRGB encoded). Finished output rows go to `emit` with the
    /// index of the first one and how many there are.
    pub fn push(
        &mut self,
        rows: Rows<'_>,
        emit: &mut dyn FnMut(Rows<'_>, u32, usize) -> Result<()>,
    ) -> Result<()> {
        let row_len = self.in_w as usize * self.ch;
        let n = rows.len() / row_len;
        if n == 0 {
            return Ok(());
        }
        self.load_linear(rows);
        self.horizontal(n)?;
        let orow = self.out_w as usize * self.ch;
        for r in 0..n {
            let mut v = self.spare.pop().unwrap_or_default();
            v.clear();
            v.extend_from_slice(&self.reduced[r * orow..(r + 1) * orow]);
            self.window.push_back(v);
        }
        self.received += n;
        self.drain(emit)
    }

    /// Emit every output row whose source rows have all arrived.
    fn drain(&mut self, emit: &mut dyn FnMut(Rows<'_>, u32, usize) -> Result<()>) -> Result<()> {
        let mut batch_start = self.next_out;
        let mut batch = 0usize;
        self.out8.clear();
        self.out16.clear();
        while self.next_out < self.out_h as usize {
            let (first, count) = {
                let vr = &self.vweights[self.next_out];
                (vr.first, vr.weights.len())
            };
            if first + count > self.received {
                break;
            }
            // Drop rows no later output row will use. The first needed row never decreases.
            while self.base < first {
                if let Some(v) = self.window.pop_front() {
                    self.spare.push(v);
                }
                self.base += 1;
            }
            self.acc.iter_mut().for_each(|a| *a = 0.0);
            {
                let vr = &self.vweights[self.next_out];
                for (k, &w) in vr.weights.iter().enumerate() {
                    let row = &self.window[first + k - self.base];
                    for (a, &s) in self.acc.iter_mut().zip(row.iter()) {
                        *a += w * s as f32;
                    }
                }
            }
            {
                // Limit to the range of the rows under the main lobe (no halos).
                let (cl, chh) = {
                    let vr = &self.vweights[self.next_out];
                    (first + vr.core_lo, first + vr.core_hi)
                };
                let len = self.acc.len();
                self.lo_buf.clear();
                self.lo_buf.resize(len, u16::MAX);
                self.hi_buf.clear();
                self.hi_buf.resize(len, 0);
                for i in cl..chh {
                    let row = &self.window[i - self.base];
                    for ((lo, hi), &v) in self
                        .lo_buf
                        .iter_mut()
                        .zip(self.hi_buf.iter_mut())
                        .zip(row.iter())
                    {
                        *lo = (*lo).min(v);
                        *hi = (*hi).max(v);
                    }
                }
                for ((a, &lo), &hi) in self
                    .acc
                    .iter_mut()
                    .zip(self.lo_buf.iter())
                    .zip(self.hi_buf.iter())
                {
                    *a = a.clamp(lo as f32, hi as f32);
                }
            }
            self.finish_row();
            self.next_out += 1;
            batch += 1;
            if batch == EMIT_ROWS {
                self.flush(batch_start, batch, emit)?;
                batch_start = self.next_out;
                batch = 0;
            }
        }
        if batch > 0 {
            self.flush(batch_start, batch, emit)?;
        }
        Ok(())
    }

    fn flush(
        &mut self,
        y0: usize,
        n: usize,
        emit: &mut dyn FnMut(Rows<'_>, u32, usize) -> Result<()>,
    ) -> Result<()> {
        let rows = match self.depth {
            Depth::U8 => Rows::U8(&self.out8),
            Depth::U16 => Rows::U16(&self.out16),
        };
        emit(rows, y0 as u32, n)?;
        self.out8.clear();
        self.out16.clear();
        Ok(())
    }

    /// Turn the accumulated linear row into sRGB samples with alpha restored.
    fn finish_row(&mut self) {
        let l = luts();
        let ch = self.ch;
        let alpha = self.layout.has_alpha();
        let color_ch = if alpha { ch - 1 } else { ch };
        for p in self.acc.chunks_exact(ch) {
            let a = if alpha {
                p[ch - 1].round().clamp(0.0, 65535.0)
            } else {
                65535.0
            };
            for &c in &p[..color_ch] {
                let mut lin = c;
                if alpha {
                    lin = if a > 0.0 { lin * 65535.0 / a } else { 0.0 };
                }
                let idx = lin.round().clamp(0.0, 65535.0) as usize;
                match self.depth {
                    Depth::U8 => self.out8.push(l.lin16_to_s8[idx]),
                    Depth::U16 => self.out16.push(l.lin16_to_s16[idx]),
                }
            }
            if alpha {
                match self.depth {
                    Depth::U8 => self.out8.push(((a as u32 + 128) / 257) as u8),
                    Depth::U16 => self.out16.push(a as u16),
                }
            }
        }
    }

    /// Check that every row arrived and every output row left.
    pub fn finish(&self) -> Result<()> {
        if self.received != self.in_h as usize || self.next_out != self.out_h as usize {
            return Err(internal("the resampler ended early"));
        }
        Ok(())
    }
}

fn internal(what: &str) -> ImageError {
    ImageError::Failed(format!(
        "The resize failed ({what}). Try again, or with a smaller image."
    ))
}

