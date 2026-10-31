//! Turning and cropping a picture into an exact output size in one pass.
//!
//! The passport crop needs three things at once: level the head, cut the rectangle the rule asks
//! for, and land on exact pixels. [`render`] does that with one affine map from output pixel to
//! source position. Large reductions are done first with the pipeline's resampler on just the
//! region that is needed (so no output pixel averages fewer than about one source pixel), then a
//! bilinear sample finishes the turn. Anything the map reaches outside the picture is filled
//! with a colour and counted, so the caller can say how much was added.

use crate::error::{ImageError, Result};
use crate::image::resample_image;
use crate::image::{Image, Layout, Samples};

/// Map from output pixel centre `(u, v)` to source position:
/// `x = a u + b v + c`, `y = d u + e v + f` (source pixel centres are at half integers).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    /// x from u.
    pub a: f64,
    /// x from v.
    pub b: f64,
    /// x offset.
    pub c: f64,
    /// y from u.
    pub d: f64,
    /// y from v.
    pub e: f64,
    /// y offset.
    pub f: f64,
}

impl Affine {
    /// Source position of output position `(u, v)`.
    pub fn at(&self, u: f64, v: f64) -> (f64, f64) {
        (
            self.a * u + self.b * v + self.c,
            self.d * u + self.e * v + self.f,
        )
    }

    /// Source pixels per output pixel along x (the map is a scaled turn).
    pub fn scale(&self) -> f64 {
        (self.a * self.a + self.d * self.d).sqrt()
    }
}

/// How much of the output the source did not reach.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Coverage {
    /// Fraction of output pixels whose source position was outside the picture.
    pub outside: f64,
    /// Rows at the top of the output that are entirely outside.
    pub top_rows: u32,
    /// Rows at the bottom that are entirely outside.
    pub bottom_rows: u32,
    /// Columns at the left that are entirely outside.
    pub left_cols: u32,
    /// Columns at the right that are entirely outside.
    pub right_cols: u32,
}

fn channels(l: Layout) -> Result<usize> {
    match l {
        Layout::Gray => Ok(1),
        Layout::Rgb => Ok(3),
        _ => Err(ImageError::Failed(
            "internal: the crop works on gray or RGB pictures".into(),
        )),
    }
}

/// Resample `src` through `m` into a `w` by `h` picture, filling what lies outside with `fill`
/// (one value per channel).
pub fn render(src: &Image, m: &Affine, w: u32, h: u32, fill: &[u8]) -> Result<(Image, Coverage)> {
    let ch = channels(src.layout)?;
    let Samples::U8(_) = &src.samples else {
        return Err(ImageError::Failed(
            "internal: the crop needs 8 bit pictures".into(),
        ));
    };
    if fill.len() != ch || w == 0 || h == 0 {
        return Err(ImageError::Failed("internal: bad crop request".into()));
    }
    // The part of the source the output can see, with room for the filter.
    let corners = [
        m.at(0.0, 0.0),
        m.at(f64::from(w), 0.0),
        m.at(0.0, f64::from(h)),
        m.at(f64::from(w), f64::from(h)),
    ];
    let k = m.scale().max(1e-6);
    let pad = (2.0 * k).ceil() + 2.0;
    let min_x = corners.iter().map(|p| p.0).fold(f64::MAX, f64::min) - pad;
    let max_x = corners.iter().map(|p| p.0).fold(f64::MIN, f64::max) + pad;
    let min_y = corners.iter().map(|p| p.1).fold(f64::MAX, f64::min) - pad;
    let max_y = corners.iter().map(|p| p.1).fold(f64::MIN, f64::max) + pad;
    let x0 = min_x.floor().clamp(0.0, f64::from(src.width));
    let y0 = min_y.floor().clamp(0.0, f64::from(src.height));
    let x1 = max_x.ceil().clamp(0.0, f64::from(src.width));
    let y1 = max_y.ceil().clamp(0.0, f64::from(src.height));
    let (cw, chh) = ((x1 - x0) as u32, (y1 - y0) as u32);

    let mut local = *m;
    local.c -= x0;
    local.f -= y0;
    let owned;
    let (work, mut mm): (&Image, Affine) = if cw == 0 || chh == 0 {
        // Nothing of the picture is visible: an all outside result.
        (src, local)
    } else if (cw, chh) == (src.width, src.height) {
        (src, local)
    } else {
        owned = src.crop(x0 as u32, y0 as u32, cw, chh);
        (&owned, local)
    };
    let reduced;
    let mut work = work;
    if k >= 1.5 && cw > 0 && chh > 0 {
        let (nw, nh) = (
            ((f64::from(work.width) / k).round() as u32).max(1),
            ((f64::from(work.height) / k).round() as u32).max(1),
        );
        if (nw, nh) != (work.width, work.height) {
            reduced = resample_image(work, nw, nh)?;
            let (kx, ky) = (
                f64::from(work.width) / f64::from(nw),
                f64::from(work.height) / f64::from(nh),
            );
            mm = Affine {
                a: mm.a / kx,
                b: mm.b / kx,
                c: mm.c / kx,
                d: mm.d / ky,
                e: mm.e / ky,
                f: mm.f / ky,
            };
            work = &reduced;
        }
    }
    let Samples::U8(sv) = &work.samples else {
        return Err(ImageError::Failed(
            "internal: the crop needs 8 bit pictures".into(),
        ));
    };
    let (sw, sh) = (work.width as i64, work.height as i64);
    let mut out = vec![0u8; w as usize * h as usize * ch];
    let mut outside_rows = vec![0u32; h as usize];
    let mut outside_cols = vec![0u32; w as usize];
    let mut outside = 0u64;
    let rl = sw as usize * ch;
    for v in 0..h {
        for u in 0..w {
            let (sx, sy) = mm.at(f64::from(u) + 0.5, f64::from(v) + 0.5);
            // Position among pixel centres.
            let (fx, fy) = (sx - 0.5, sy - 0.5);
            let o = (v as usize * w as usize + u as usize) * ch;
            if sw == 0
                || sh == 0
                || fx < -0.5
                || fy < -0.5
                || fx > sw as f64 - 0.5
                || fy > sh as f64 - 0.5
            {
                out[o..o + ch].copy_from_slice(fill);
                outside += 1;
                outside_rows[v as usize] += 1;
                outside_cols[u as usize] += 1;
                continue;
            }
            let (ix, iy) = (fx.floor(), fy.floor());
            let (tx, ty) = ((fx - ix) as f32, (fy - iy) as f32);
            let (x_a, y_a) = ((ix as i64).clamp(0, sw - 1), (iy as i64).clamp(0, sh - 1));
            let (x_b, y_b) = (
                (ix as i64 + 1).clamp(0, sw - 1),
                (iy as i64 + 1).clamp(0, sh - 1),
            );
            for c in 0..ch {
                let p = |x: i64, y: i64| f32::from(sv[y as usize * rl + x as usize * ch + c]);
                let top = p(x_a, y_a) * (1.0 - tx) + p(x_b, y_a) * tx;
                let bot = p(x_a, y_b) * (1.0 - tx) + p(x_b, y_b) * tx;
                out[o + c] = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
            }
        }
    }
    let rows_full = |counts: &[u32], len: u32| -> (u32, u32) {
        let lead = counts.iter().take_while(|&&n| n == len).count() as u32;
        let trail = counts.iter().rev().take_while(|&&n| n == len).count() as u32;
        (lead, trail)
    };
    let (top_rows, bottom_rows) = rows_full(&outside_rows, w);
    let (left_cols, right_cols) = rows_full(&outside_cols, h);
    Ok((
        Image {
            width: w,
            height: h,
            layout: src.layout,
            samples: Samples::U8(out),
            icc: src.icc.clone(),
        },
        Coverage {
            outside: outside as f64 / (f64::from(w) * f64::from(h)),
            top_rows,
            bottom_rows,
            left_cols,
            right_cols,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: u32, h: u32) -> Image {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[x as u8, y as u8, 7]);
            }
        }
        Image {
            width: w,
            height: h,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        }
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 3] {
        let Samples::U8(v) = &img.samples else {
            panic!()
        };
        let o = (y as usize * img.width as usize + x as usize) * 3;
        [v[o], v[o + 1], v[o + 2]]
    }

    #[test]
    fn the_identity_map_copies() {
        let src = ramp(30, 20);
        let m = Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: 0.0,
        };
        let (out, cov) = render(&src, &m, 30, 20, &[0, 0, 0]).unwrap();
        assert_eq!(out, src);
        assert_eq!(cov.outside, 0.0);
    }

    #[test]
    fn a_crop_takes_the_rectangle() {
        let src = ramp(40, 40);
        let m = Affine {
            a: 1.0,
            b: 0.0,
            c: 10.0,
            d: 0.0,
            e: 1.0,
            f: 5.0,
        };
        let (out, _) = render(&src, &m, 10, 10, &[0, 0, 0]).unwrap();
        assert_eq!(px(&out, 0, 0), [10, 5, 7]);
        assert_eq!(px(&out, 9, 9), [19, 14, 7]);
    }

    #[test]
    fn reaching_past_the_edge_fills_and_counts() {
        let src = ramp(20, 20);
        // The output starts 5 rows above the picture.
        let m = Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: -5.0,
        };
        let (out, cov) = render(&src, &m, 20, 15, &[255, 255, 255]).unwrap();
        assert_eq!(px(&out, 3, 2), [255, 255, 255]);
        assert_eq!(cov.top_rows, 5);
        assert_eq!(cov.bottom_rows, 0);
        assert!((cov.outside - 5.0 / 15.0).abs() < 1e-9);
    }

    #[test]
    fn a_quarter_turn_swaps_the_axes() {
        let src = ramp(20, 20);
        // Output x runs down the source, output y runs left (a 90 degree turn).
        let m = Affine {
            a: 0.0,
            b: -1.0,
            c: 20.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        };
        let (out, _) = render(&src, &m, 20, 20, &[0, 0, 0]).unwrap();
        // Output (0, 0) samples source around (19.5, 0.5) -> pixel (19, 0).
        assert_eq!(px(&out, 0, 0), [19, 0, 7]);
        // Output (5, 0) samples source around (19.5, 5.5) -> pixel (19, 5).
        assert_eq!(px(&out, 5, 0), [19, 5, 7]);
    }

    #[test]
    fn a_big_reduction_averages_instead_of_skipping() {
        // Alternating columns of 0 and 200 averaged by a 4x reduction land on the mean in linear
        // light (IPC-2: resampling happens in linear light), 146 in sRGB, not on a skipped column.
        let (w, h) = (64u32, 64u32);
        let mut v = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let g = if x % 2 == 0 { 0 } else { 200 };
                v.extend_from_slice(&[g, g, g]);
            }
        }
        let src = Image {
            width: w,
            height: h,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        };
        let m = Affine {
            a: 4.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 4.0,
            f: 0.0,
        };
        let (out, _) = render(&src, &m, 16, 16, &[0, 0, 0]).unwrap();
        let p = px(&out, 8, 8)[0];
        assert!((138..=154).contains(&p), "{p}");
    }
}
