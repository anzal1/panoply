//! Pixel helpers the modules share: resizing, blurs, distance dilation, color conversion in linear
//! light, and the small parsers for rectangle options.

use std::sync::OnceLock;

use fast_image_resize as fir;
use fir::images::{Image as FirImage, ImageRef};
use panoply_image::error::{ImageError, Result};

/// A resize kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kernel {
    /// Linear interpolation, for soft masks.
    Bilinear,
    /// Catmull-Rom cubic, sharper than bilinear without ringing much, for enlarging.
    CatmullRom,
    /// Lanczos 3, for shrinking photographs.
    Lanczos3,
    /// Box (area average), for exact integer reductions.
    Box,
}

fn alg(k: Kernel) -> fir::ResizeAlg {
    fir::ResizeAlg::Convolution(match k {
        Kernel::Bilinear => fir::FilterType::Bilinear,
        Kernel::CatmullRom => fir::FilterType::CatmullRom,
        Kernel::Lanczos3 => fir::FilterType::Lanczos3,
        Kernel::Box => fir::FilterType::Box,
    })
}

fn resize_err(e: impl std::fmt::Display) -> ImageError {
    ImageError::Failed(format!("The picture could not be resized ({e})."))
}

/// Resize interleaved 8 bit pixels with `ch` channels (1, 2, 3 or 4). Resampling is on the stored
/// values, which is right for masks and good enough for the model inputs.
pub fn resize_u8(
    src: &[u8],
    sw: usize,
    sh: usize,
    ch: usize,
    dw: usize,
    dh: usize,
    kernel: Kernel,
) -> Result<Vec<u8>> {
    if sw == dw && sh == dh {
        return Ok(src.to_vec());
    }
    let pt = match ch {
        1 => fir::PixelType::U8,
        2 => fir::PixelType::U8x2,
        3 => fir::PixelType::U8x3,
        4 => fir::PixelType::U8x4,
        _ => return Err(resize_err("unsupported channel count")),
    };
    let s = ImageRef::new(sw as u32, sh as u32, src, pt).map_err(resize_err)?;
    let mut d = FirImage::new(dw as u32, dh as u32, pt);
    fir::Resizer::new()
        .resize(
            &s,
            &mut d,
            &fir::ResizeOptions::new().resize_alg(alg(kernel)),
        )
        .map_err(resize_err)?;
    Ok(d.into_vec())
}

/// Resize one plane of 32 bit floats.
pub fn resize_f32(
    src: &[f32],
    sw: usize,
    sh: usize,
    dw: usize,
    dh: usize,
    kernel: Kernel,
) -> Result<Vec<f32>> {
    if sw == dw && sh == dh {
        return Ok(src.to_vec());
    }
    let bytes: Vec<u8> = src.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let s = ImageRef::new(sw as u32, sh as u32, &bytes, fir::PixelType::F32).map_err(resize_err)?;
    let mut d = FirImage::new(dw as u32, dh as u32, fir::PixelType::F32);
    fir::Resizer::new()
        .resize(
            &s,
            &mut d,
            &fir::ResizeOptions::new().resize_alg(alg(kernel)),
        )
        .map_err(resize_err)?;
    Ok(d.buffer()
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Mean of a window of radius `r` around every pixel (edges replicated), written to `out`.
/// A running sum in `f64`, so long rows do not drift.
pub fn box_mean(src: &[f32], w: usize, h: usize, r: usize, out: &mut [f32]) {
    debug_assert_eq!(src.len(), w * h);
    debug_assert_eq!(out.len(), w * h);
    if w == 0 || h == 0 {
        return;
    }
    let n = (2 * r + 1) as f64;
    // Horizontal pass into `tmp`.
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut sum = row[0] as f64 * (r + 1) as f64;
        for j in 1..=r {
            sum += row[j.min(w - 1)] as f64;
        }
        let o = &mut tmp[y * w..(y + 1) * w];
        for x in 0..w {
            o[x] = (sum / n) as f32;
            sum += row[(x + r + 1).min(w - 1)] as f64 - row[x.saturating_sub(r)] as f64;
        }
    }
    // Vertical pass, row by row so memory access stays sequential.
    let mut acc: Vec<f64> = tmp[..w]
        .iter()
        .map(|&v| v as f64 * (r + 1) as f64)
        .collect();
    for j in 1..=r {
        let row = &tmp[j.min(h - 1) * w..(j.min(h - 1) + 1) * w];
        for (a, &v) in acc.iter_mut().zip(row) {
            *a += v as f64;
        }
    }
    for y in 0..h {
        let o = &mut out[y * w..(y + 1) * w];
        for (ov, a) in o.iter_mut().zip(&acc) {
            *ov = (*a / n) as f32;
        }
        let add = &tmp[(y + r + 1).min(h - 1) * w..((y + r + 1).min(h - 1) + 1) * w];
        let sub = &tmp[y.saturating_sub(r) * w..(y.saturating_sub(r) + 1) * w];
        for ((a, &p), &m) in acc.iter_mut().zip(add).zip(sub) {
            *a += p as f64 - m as f64;
        }
    }
}

/// A blur close to a Gaussian of this sigma: three box passes.
pub fn gaussian_blur(plane: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    // Three box passes of width b approximate a Gaussian with sigma^2 = 3 (b^2 - 1) / 12.
    let b = ((12.0 * sigma * sigma / 3.0 + 1.0).sqrt()).max(1.0);
    let r = (((b - 1.0) / 2.0).round() as usize).max(1);
    let mut tmp = vec![0f32; w * h];
    for _ in 0..3 {
        box_mean(plane, w, h, r, &mut tmp);
        plane.copy_from_slice(&tmp);
    }
}

/// Grows the true pixels of `mask` by `r` pixels (a disk, by a two pass chamfer distance).
pub fn dilate(mask: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    if r == 0 {
        return mask.to_vec();
    }
    // 3-4 chamfer distance (units of 1/3 pixel) to the nearest set pixel.
    const INF: u32 = u32::MAX / 2;
    let mut d: Vec<u32> = mask.iter().map(|&m| if m != 0 { 0 } else { INF }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 3);
            }
            if y > 0 {
                v = v.min(d[i - w] + 3);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 4);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 3);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 3);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 4);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    let lim = (r as u32) * 3 + 1;
    d.iter().map(|&v| u8::from(v <= lim)).collect()
}

/// Area average reduction by an integer factor (the last partial cells are averaged too).
pub fn reduce_box(src: &[f32], w: usize, h: usize, f: usize) -> (Vec<f32>, usize, usize) {
    let (dw, dh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0f32; dw * dh];
    for dy in 0..dh {
        for dx in 0..dw {
            let (x0, y0) = (dx * f, dy * f);
            let (x1, y1) = ((x0 + f).min(w), (y0 + f).min(h));
            let mut s = 0f32;
            for y in y0..y1 {
                for &v in &src[y * w + x0..y * w + x1] {
                    s += v;
                }
            }
            out[dy * dw + dx] = s / ((x1 - x0) * (y1 - y0)) as f32;
        }
    }
    (out, dw, dh)
}

/// Like [`reduce_box`], for values computed from the pixel index, so no full size plane has to
/// exist first.
pub fn reduce_map(
    w: usize,
    h: usize,
    f: usize,
    value: impl Fn(usize) -> f32,
) -> (Vec<f32>, usize, usize) {
    let (dw, dh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0f32; dw * dh];
    for dy in 0..dh {
        for dx in 0..dw {
            let (x0, y0) = (dx * f, dy * f);
            let (x1, y1) = ((x0 + f).min(w), (y0 + f).min(h));
            let mut s = 0f32;
            for y in y0..y1 {
                for x in x0..x1 {
                    s += value(y * w + x);
                }
            }
            out[dy * dw + dx] = s / ((x1 - x0) * (y1 - y0)) as f32;
        }
    }
    (out, dw, dh)
}

/// Bilinear sample of a plane at fractional coordinates, edges clamped.
#[inline]
pub fn sample(plane: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let top = plane[y0 * w + x0] * (1.0 - fx) + plane[y0 * w + x1] * fx;
    let bot = plane[y1 * w + x0] * (1.0 - fx) + plane[y1 * w + x1] * fx;
    top * (1.0 - fy) + bot * fy
}

/// Upsamples a low resolution plane to `dw x dh` with bilinear interpolation (pixel centers
/// aligned), for the guided filter's coefficient planes.
pub fn upsample_bilinear(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let mut out = vec![0f32; dw * dh];
    let (sx, sy) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sy - 0.5;
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sx - 0.5;
            out[y * dw + x] = sample(src, sw, sh, fx, fy);
        }
    }
    out
}

struct Luts {
    to_linear: [f32; 256],
    to_srgb: Vec<u8>,
}

const SRGB_STEPS: usize = 4096;

fn luts() -> &'static Luts {
    static L: OnceLock<Luts> = OnceLock::new();
    L.get_or_init(|| {
        let mut to_linear = [0f32; 256];
        for (i, v) in to_linear.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        let to_srgb = (0..=SRGB_STEPS)
            .map(|i| {
                let l = i as f32 / SRGB_STEPS as f32;
                let c = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8
            })
            .collect();
        Luts { to_linear, to_srgb }
    })
}

/// sRGB 8 bit to linear light, 0 to 1.
#[inline]
pub fn to_linear(c: u8) -> f32 {
    luts().to_linear[c as usize]
}

/// Linear light, 0 to 1, to sRGB 8 bit.
#[inline]
pub fn to_srgb(l: f32) -> u8 {
    luts().to_srgb[(l.clamp(0.0, 1.0) * SRGB_STEPS as f32 + 0.5) as usize]
}

/// Luma of an sRGB pixel, 0 to 255.
#[inline]
pub fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// An axis aligned rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: usize,
    /// Top edge.
    pub y: usize,
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
}

impl Rect {
    /// The rectangle grown by `m` pixels on every side, clamped to a `w x h` picture.
    pub fn grown(self, m: usize, w: usize, h: usize) -> Rect {
        let x0 = self.x.saturating_sub(m);
        let y0 = self.y.saturating_sub(m);
        let x1 = (self.x + self.w + m).min(w);
        let y1 = (self.y + self.h + m).min(h);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    /// True when the two rectangles share at least one pixel.
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

/// Parses the `rect` option: one or more rectangles separated by `;`, each `x,y,w,h` in pixels,
/// or with a trailing `%` on every number in fractions of the picture (`70%,85%,28%,12%`).
/// Rectangles are clamped to the picture; an empty result after clamping is an error.
pub fn parse_rects(text: &str, w: usize, h: usize) -> Result<Vec<Rect>> {
    let bad = |what: &str| {
        ImageError::BadOption(format!(
            "One of the options is not right: the rectangle \"{what}\" is not x,y,w,h. Use pixels such as 100,200,300,80 or percentages such as 70%,85%,28%,12%, and separate several with a semicolon."
        ))
    };
    let mut out = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let nums: Vec<&str> = part.split(',').map(str::trim).collect();
        if nums.len() != 4 {
            return Err(bad(part));
        }
        let mut v = [0f64; 4];
        for (i, n) in nums.iter().enumerate() {
            let (num, pct) = match n.strip_suffix('%') {
                Some(s) => (s.trim(), true),
                None => (*n, false),
            };
            let x: f64 = num.parse().map_err(|_| bad(part))?;
            if !x.is_finite() || x < 0.0 {
                return Err(bad(part));
            }
            let base = if i % 2 == 0 { w } else { h } as f64;
            v[i] = if pct { x / 100.0 * base } else { x };
        }
        let x0 = (v[0].round() as usize).min(w);
        let y0 = (v[1].round() as usize).min(h);
        let x1 = ((v[0] + v[2]).round() as usize).min(w);
        let y1 = ((v[1] + v[3]).round() as usize).min(h);
        if x1 > x0 && y1 > y0 {
            out.push(Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            });
        }
    }
    if out.is_empty() {
        return Err(ImageError::BadOption(
            "One of the options is not right: the rectangle is empty or outside the picture. Check the numbers and try again.".into(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_mean_of_a_constant_is_constant() {
        let src = vec![0.5f32; 30 * 20];
        let mut out = vec![0f32; 30 * 20];
        box_mean(&src, 30, 20, 4, &mut out);
        assert!(out.iter().all(|&v| (v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn box_mean_matches_a_direct_average() {
        let (w, h, r) = (17usize, 13usize, 3usize);
        let src: Vec<f32> = (0..w * h)
            .map(|i| ((i * 37) % 101) as f32 / 100.0)
            .collect();
        let mut out = vec![0f32; w * h];
        box_mean(&src, w, h, r, &mut out);
        for y in 0..h {
            for x in 0..w {
                let mut s = 0f64;
                for dy in -(r as i64)..=r as i64 {
                    for dx in -(r as i64)..=r as i64 {
                        let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                        let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                        s += src[yy * w + xx] as f64;
                    }
                }
                let want = (s / ((2 * r + 1) * (2 * r + 1)) as f64) as f32;
                assert!((out[y * w + x] - want).abs() < 1e-5, "{x},{y}");
            }
        }
    }

    #[test]
    fn dilation_grows_a_point_into_a_disk() {
        let (w, h) = (41usize, 41usize);
        let mut m = vec![0u8; w * h];
        m[20 * w + 20] = 1;
        let d = dilate(&m, w, h, 8);
        assert_eq!(d[20 * w + 20], 1);
        assert_eq!(d[20 * w + 28], 1);
        assert_eq!(d[20 * w + 30], 0);
        assert_eq!(d[12 * w + 20], 1);
        // The corner of the 17 x 17 square is farther than 8 px away.
        assert_eq!(d[12 * w + 12], 0);
        assert_eq!(dilate(&m, w, h, 0), m);
    }

    #[test]
    fn srgb_roundtrip_is_exact() {
        for c in 0..=255u8 {
            assert_eq!(to_srgb(to_linear(c)), c, "{c}");
        }
    }

    #[test]
    fn rects_parse_pixels_and_percentages() {
        let r = parse_rects("10,20,30,40; 50%,50%,10%,10%", 200, 100).unwrap();
        assert_eq!(
            r[0],
            Rect {
                x: 10,
                y: 20,
                w: 30,
                h: 40
            }
        );
        assert_eq!(
            r[1],
            Rect {
                x: 100,
                y: 50,
                w: 20,
                h: 10
            }
        );
        assert!(parse_rects("1,2,3", 10, 10).is_err());
        assert!(parse_rects("a,b,c,d", 10, 10).is_err());
        assert!(parse_rects("500,500,5,5", 10, 10).is_err());
        // Clamped to the picture.
        let c = parse_rects("5,5,100,100", 10, 10).unwrap();
        assert_eq!(c[0].w, 5);
    }

    #[test]
    fn resizing_keeps_a_flat_color() {
        let src = vec![200u8; 8 * 8 * 3];
        let out = resize_u8(&src, 8, 8, 3, 4, 4, Kernel::Lanczos3).unwrap();
        assert_eq!(out.len(), 4 * 4 * 3);
        assert!(out.iter().all(|&v| v == 200));
    }

    #[test]
    fn rect_helpers() {
        let a = Rect {
            x: 10,
            y: 10,
            w: 10,
            h: 10,
        };
        let b = Rect {
            x: 15,
            y: 15,
            w: 10,
            h: 10,
        };
        assert!(a.overlaps(&b));
        assert_eq!(a.union(&b).w, 15);
        assert_eq!(a.grown(20, 100, 100).x, 0);
    }
}
