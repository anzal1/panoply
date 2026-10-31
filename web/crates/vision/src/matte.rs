//! Matte work after a mask has been found: snapping the soft edge to the picture's own edges with
//! a guided filter, and cleaning the edge colors of the old background (decontamination).
//!
//! Both run in tiles with a halo, only where the mask has an edge, so a 12 megapixel picture
//! costs memory for a few tiles, not for whole-picture float planes.
//!
//! * **Edge refine** is the color guided filter of He, Sun and Tang (2010), in its fast form
//!   (coefficients computed on a reduced copy, applied at full resolution). A model's mask is
//!   made at 1024 pixels and a system mask is smooth; the filter pulls both onto the hair, fur
//!   and fabric edges of the full resolution picture.
//! * **Decontamination** is the blur fusion foreground estimate of Forte and Pitié (2021): the
//!   foreground color under a soft edge is estimated from the colors nearby, so a cutout placed
//!   on a new background has no halo of the old one.

use panoply_core::CancelToken;
use crate::error::{ImageError, Result};

use crate::px::{Rect, box_mean, reduce_box, upsample_bilinear};
use crate::tile::{copy_out, cores, window};

/// Cell size of the band map, in pixels.
const CELL: usize = 8;
/// Side of one processing tile's core, in pixels.
const CORE: usize = 384;

fn check(cancel: &CancelToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(ImageError::Cancelled)
    } else {
        Ok(())
    }
}

/// Settings for the guided filter.
#[derive(Debug, Clone, Copy)]
pub struct Refine {
    /// Window radius in pixels.
    pub radius: usize,
    /// Regularisation (larger keeps the input mask's shape, smaller follows the picture).
    pub eps: f32,
}

impl Refine {
    /// Defaults from the picture size: the radius grows with the picture so a 12 MP photo and a
    /// 1 MP one get the same look.
    pub fn for_size(w: usize, h: usize) -> Self {
        let long = w.max(h) as f32;
        Refine {
            radius: ((long / 350.0).round() as usize).clamp(3, 24),
            eps: 2e-4,
        }
    }
}

/// Which `CELL x CELL` cells of the picture hold an alpha edge, grown by `grow` pixels.
struct Band {
    gw: usize,
    gh: usize,
    cells: Vec<bool>,
}

impl Band {
    fn new(alpha: &[u8], w: usize, h: usize, grow: usize, soft_only: bool) -> Band {
        let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
        let mut cells = vec![false; gw * gh];
        for y in 0..h {
            let row = &alpha[y * w..(y + 1) * w];
            let below = (y + 1 < h).then(|| &alpha[(y + 1) * w..(y + 2) * w]);
            for x in 0..w {
                let a = row[x];
                let mut edge = a > 3 && a < 252;
                if !soft_only && !edge {
                    if x + 1 < w {
                        edge = (i32::from(a) - i32::from(row[x + 1])).abs() > 64;
                    }
                    if !edge && let Some(b) = below {
                        edge = (i32::from(a) - i32::from(b[x])).abs() > 64;
                    }
                }
                if edge {
                    cells[(y / CELL) * gw + x / CELL] = true;
                }
            }
        }
        let g = grow.div_ceil(CELL);
        if g > 0 {
            // Separable Chebyshev dilation of the cell grid.
            let mut tmp = vec![false; gw * gh];
            for y in 0..gh {
                for x in 0..gw {
                    if cells[y * gw + x] {
                        for xx in x.saturating_sub(g)..=(x + g).min(gw - 1) {
                            tmp[y * gw + xx] = true;
                        }
                    }
                }
            }
            let mut out = vec![false; gw * gh];
            for y in 0..gh {
                for x in 0..gw {
                    if tmp[y * gw + x] {
                        for yy in y.saturating_sub(g)..=(y + g).min(gh - 1) {
                            out[yy * gw + x] = true;
                        }
                    }
                }
            }
            cells = out;
        }
        Band { gw, gh, cells }
    }

    fn at(&self, x: usize, y: usize) -> bool {
        self.cells[(y / CELL) * self.gw + x / CELL]
    }

    fn any_in(&self, r: Rect) -> bool {
        let (x0, x1) = (r.x / CELL, (r.x + r.w - 1) / CELL);
        let (y0, y1) = (r.y / CELL, (r.y + r.h - 1) / CELL);
        (y0..=y1.min(self.gh - 1)).any(|y| {
            self.cells[y * self.gw + x0..=(y * self.gw + x1.min(self.gw - 1))]
                .iter()
                .any(|&c| c)
        })
    }
}

/// The color guided filter on one window: `guide` holds the 3 planes (0 to 1), `p` the mask
/// plane (0 to 1). Coefficients are computed on a copy reduced by `s`.
fn guided_color(
    guide: [&[f32]; 3],
    p: &[f32],
    w: usize,
    h: usize,
    r: usize,
    eps: f32,
    s: usize,
) -> Vec<f32> {
    let (ig, pl, lw, lh): (Vec<Vec<f32>>, Vec<f32>, usize, usize) = if s > 1 {
        let (a, lw, lh) = reduce_box(guide[0], w, h, s);
        let (b, _, _) = reduce_box(guide[1], w, h, s);
        let (c, _, _) = reduce_box(guide[2], w, h, s);
        let (pp, _, _) = reduce_box(p, w, h, s);
        (vec![a, b, c], pp, lw, lh)
    } else {
        (
            vec![guide[0].to_vec(), guide[1].to_vec(), guide[2].to_vec()],
            p.to_vec(),
            w,
            h,
        )
    };
    let n = lw * lh;
    let r = (r / s).max(1);
    let mean = |src: &[f32]| -> Vec<f32> {
        let mut out = vec![0f32; n];
        box_mean(src, lw, lh, r, &mut out);
        out
    };
    let m: Vec<Vec<f32>> = ig.iter().map(|c| mean(c)).collect();
    let mp = mean(&pl);
    let prod = |a: &[f32], b: &[f32]| -> Vec<f32> { a.iter().zip(b).map(|(x, y)| x * y).collect() };
    let rr = mean(&prod(&ig[0], &ig[0]));
    let rg = mean(&prod(&ig[0], &ig[1]));
    let rb = mean(&prod(&ig[0], &ig[2]));
    let gg = mean(&prod(&ig[1], &ig[1]));
    let gb = mean(&prod(&ig[1], &ig[2]));
    let bb = mean(&prod(&ig[2], &ig[2]));
    let ipr = mean(&prod(&ig[0], &pl));
    let ipg = mean(&prod(&ig[1], &pl));
    let ipb = mean(&prod(&ig[2], &pl));
    let mut a0 = vec![0f32; n];
    let mut a1 = vec![0f32; n];
    let mut a2 = vec![0f32; n];
    let mut b = vec![0f32; n];
    for i in 0..n {
        let (mr, mg, mb) = (m[0][i], m[1][i], m[2][i]);
        let s00 = rr[i] - mr * mr + eps;
        let s01 = rg[i] - mr * mg;
        let s02 = rb[i] - mr * mb;
        let s11 = gg[i] - mg * mg + eps;
        let s12 = gb[i] - mg * mb;
        let s22 = bb[i] - mb * mb + eps;
        let (c0, c1, c2) = (
            ipr[i] - mr * mp[i],
            ipg[i] - mg * mp[i],
            ipb[i] - mb * mp[i],
        );
        // Inverse of the symmetric 3 x 3 matrix by cofactors.
        let i00 = s11 * s22 - s12 * s12;
        let i01 = s02 * s12 - s01 * s22;
        let i02 = s01 * s12 - s02 * s11;
        let i11 = s00 * s22 - s02 * s02;
        let i12 = s01 * s02 - s00 * s12;
        let i22 = s00 * s11 - s01 * s01;
        let det = s00 * i00 + s01 * i01 + s02 * i02;
        let inv = if det.abs() > 1e-20 { 1.0 / det } else { 0.0 };
        let (x0, x1, x2) = (
            (i00 * c0 + i01 * c1 + i02 * c2) * inv,
            (i01 * c0 + i11 * c1 + i12 * c2) * inv,
            (i02 * c0 + i12 * c1 + i22 * c2) * inv,
        );
        a0[i] = x0;
        a1[i] = x1;
        a2[i] = x2;
        b[i] = mp[i] - x0 * mr - x1 * mg - x2 * mb;
    }
    let (ma0, ma1, ma2, mb_) = (mean(&a0), mean(&a1), mean(&a2), mean(&b));
    let (u0, u1, u2, ub) = if s > 1 {
        (
            upsample_bilinear(&ma0, lw, lh, w, h),
            upsample_bilinear(&ma1, lw, lh, w, h),
            upsample_bilinear(&ma2, lw, lh, w, h),
            upsample_bilinear(&mb_, lw, lh, w, h),
        )
    } else {
        (ma0, ma1, ma2, mb_)
    };
    (0..w * h)
        .map(|i| u0[i] * guide[0][i] + u1[i] * guide[1][i] + u2[i] * guide[2][i] + ub[i])
        .collect()
}

/// Snaps the soft edge of `alpha` onto the edges of `rgb` (both `w x h`). Pixels away from an
/// edge are not touched, so the interior stays solid and the background stays clear.
pub fn refine_alpha(
    rgb: &[u8],
    w: usize,
    h: usize,
    alpha: &mut [u8],
    p: Refine,
    cancel: &CancelToken,
) -> Result<()> {
    let band = Band::new(alpha, w, h, p.radius * 3 / 2, false);
    let halo = 2 * p.radius + 8;
    let s = if p.radius >= 16 {
        4
    } else if p.radius >= 6 {
        2
    } else {
        1
    };
    let source = alpha.to_vec();
    for core in cores(w, h, CORE) {
        check(cancel)?;
        if !band.any_in(core) {
            continue;
        }
        let win = window(core, halo, w, h);
        let px = copy_out(rgb, w, 3, win);
        let pa = copy_out(&source, w, 1, win);
        let n = win.w * win.h;
        let mut g = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
        for (i, c) in px.chunks_exact(3).enumerate() {
            g[0][i] = f32::from(c[0]) / 255.0;
            g[1][i] = f32::from(c[1]) / 255.0;
            g[2][i] = f32::from(c[2]) / 255.0;
        }
        // Push the near-solid and near-clear ends first, so the fit sees a mask with a clear
        // "inside" and "outside" and its plateaus land close to 0 and 1.
        let pf: Vec<f32> = pa
            .iter()
            .map(|&a| ((f32::from(a) / 255.0 - 0.06) / 0.88).clamp(0.0, 1.0))
            .collect();
        let q = guided_color([&g[0], &g[1], &g[2]], &pf, win.w, win.h, p.radius, p.eps, s);
        for y in core.y..core.y + core.h {
            for x in core.x..core.x + core.w {
                if !band.at(x, y) {
                    continue;
                }
                // The fit leaves the plateaus a little off 0 and 1; stretch them back and snap the
                // ends, so no faint veil is left behind.
                let v = ((q[(y - win.y) * win.w + (x - win.x)] - 0.1) / 0.8).clamp(0.0, 1.0);
                alpha[y * w + x] = if v < 0.02 {
                    0
                } else if v > 0.98 {
                    255
                } else {
                    (v * 255.0 + 0.5) as u8
                };
            }
        }
    }
    Ok(())
}

/// The foreground colors for a picture with this alpha: the original colors where the picture is
/// solid, an estimate of the colors without the old background where the edge is soft, and the
/// estimate (or black far from any edge) where it is clear.
pub fn foreground_colors(
    rgb: &[u8],
    alpha: &[u8],
    w: usize,
    h: usize,
    cancel: &CancelToken,
) -> Result<Vec<u8>> {
    let long = w.max(h);
    let r_big = (long / 40).clamp(8, 90);
    let r_small = (long / 600).clamp(2, 6);
    let mut out = rgb.to_vec();
    // Pixels that need an estimate: not solid, and within reach of a soft pixel.
    let band = Band::new(alpha, w, h, 2 * r_small + 2, true);
    let halo = r_big + r_small + 4;
    for core in cores(w, h, CORE) {
        check(cancel)?;
        if !band.any_in(core) {
            // Far from any edge: clear pixels stay black, solid ones keep their color.
            for y in core.y..core.y + core.h {
                for x in core.x..core.x + core.w {
                    if alpha[y * w + x] == 0 {
                        out[(y * w + x) * 3..(y * w + x) * 3 + 3].fill(0);
                    }
                }
            }
            continue;
        }
        let win = window(core, halo, w, h);
        let n = win.w * win.h;
        let px = copy_out(rgb, w, 3, win);
        let pa = copy_out(alpha, w, 1, win);
        let a: Vec<f32> = pa.iter().map(|&v| f32::from(v) / 255.0).collect();
        let mut img = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
        for (i, c) in px.chunks_exact(3).enumerate() {
            for k in 0..3 {
                img[k][i] = f32::from(c[k]) / 255.0;
            }
        }
        let eps = 1e-5f32;
        // Stage one, at a quarter of the resolution because the radius is large.
        let s = 4usize;
        let (a_lo, lw, lh) = reduce_box(&a, win.w, win.h, s);
        let rl = (r_big / s).max(1);
        let mut blur_a = vec![0f32; lw * lh];
        box_mean(&a_lo, lw, lh, rl, &mut blur_a);
        let mut big_f: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut big_b: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for k in 0..3 {
            let fa: Vec<f32> = img[k].iter().zip(&a).map(|(c, a)| c * a).collect();
            let ba: Vec<f32> = img[k].iter().zip(&a).map(|(c, a)| c * (1.0 - a)).collect();
            let (fa_lo, _, _) = reduce_box(&fa, win.w, win.h, s);
            let (ba_lo, _, _) = reduce_box(&ba, win.w, win.h, s);
            let mut bf = vec![0f32; lw * lh];
            let mut bb = vec![0f32; lw * lh];
            box_mean(&fa_lo, lw, lh, rl, &mut bf);
            box_mean(&ba_lo, lw, lh, rl, &mut bb);
            for i in 0..lw * lh {
                bf[i] /= blur_a[i] + eps;
                bb[i] /= (1.0 - blur_a[i]) + eps;
            }
            big_f[k] = upsample_bilinear(&bf, lw, lh, win.w, win.h);
            big_b[k] = upsample_bilinear(&bb, lw, lh, win.w, win.h);
        }
        // F after stage one, and the blurred background kept for stage two.
        let mut f1: [Vec<f32>; 3] = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
        for k in 0..3 {
            for i in 0..n {
                let v = big_f[k][i]
                    + a[i] * (img[k][i] - a[i] * big_f[k][i] - (1.0 - a[i]) * big_b[k][i]);
                f1[k][i] = v.clamp(0.0, 1.0);
            }
        }
        // Stage two, at full resolution with a small radius.
        let mut blur_a2 = vec![0f32; n];
        box_mean(&a, win.w, win.h, r_small, &mut blur_a2);
        let mut f2: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for k in 0..3 {
            let fa: Vec<f32> = f1[k].iter().zip(&a).map(|(c, a)| c * a).collect();
            let ba: Vec<f32> = big_b[k]
                .iter()
                .zip(&a)
                .map(|(c, a)| c * (1.0 - a))
                .collect();
            let mut bf = vec![0f32; n];
            let mut bb = vec![0f32; n];
            box_mean(&fa, win.w, win.h, r_small, &mut bf);
            box_mean(&ba, win.w, win.h, r_small, &mut bb);
            let mut res = vec![0f32; n];
            for i in 0..n {
                let bfi = bf[i] / (blur_a2[i] + eps);
                let bbi = bb[i] / ((1.0 - blur_a2[i]) + eps);
                let v = bfi + a[i] * (img[k][i] - a[i] * bfi - (1.0 - a[i]) * bbi);
                res[i] = v.clamp(0.0, 1.0);
            }
            f2[k] = res;
        }
        for y in core.y..core.y + core.h {
            for x in core.x..core.x + core.w {
                let av = alpha[y * w + x];
                let o = (y * w + x) * 3;
                if av == 255 {
                    continue;
                }
                if av == 0 && !band.at(x, y) {
                    out[o..o + 3].fill(0);
                    continue;
                }
                let wi = (y - win.y) * win.w + (x - win.x);
                for k in 0..3 {
                    out[o + k] = (f2[k][wi] * 255.0 + 0.5) as u8;
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with a hard vertical edge at x = 32: left dark, right bright.
    fn edge_picture(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let c = if x < 32 { 40 } else { 210 };
                v.extend_from_slice(&[c, c, c]);
            }
        }
        v
    }

    #[test]
    fn refine_snaps_a_blurry_mask_onto_the_picture_edge() {
        let (w, h) = (96usize, 64usize);
        let rgb = edge_picture(w, h);
        // A mask whose edge is off by 1 pixel and ramps over 4 pixels, the kind of soft edge a
        // model's mask has after it is scaled up.
        let mut alpha = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let t = ((x as f32 - 33.0) / 4.0 + 0.5).clamp(0.0, 1.0);
                alpha[y * w + x] = (t * 255.0) as u8;
            }
        }
        let before = alpha.clone();
        let cancel = CancelToken::new();
        refine_alpha(
            &rgb,
            w,
            h,
            &mut alpha,
            Refine {
                radius: 8,
                eps: 1e-4,
            },
            &cancel,
        )
        .unwrap();
        let err = |a: &[u8]| -> f32 {
            let mut e = 0f32;
            for y in 0..h {
                for x in 0..w {
                    let truth = if x >= 32 { 255.0 } else { 0.0 };
                    e += (f32::from(a[y * w + x]) - truth).abs();
                }
            }
            e / (w * h) as f32
        };
        assert!(
            err(&alpha) < err(&before) * 0.5,
            "refined {} vs before {}",
            err(&alpha),
            err(&before)
        );
    }

    #[test]
    fn a_solid_mask_is_left_alone() {
        let (w, h) = (64usize, 64usize);
        let rgb = vec![120u8; w * h * 3];
        let mut alpha = vec![255u8; w * h];
        let cancel = CancelToken::new();
        refine_alpha(&rgb, w, h, &mut alpha, Refine::for_size(w, h), &cancel).unwrap();
        assert!(alpha.iter().all(|&a| a == 255));
    }

    #[test]
    fn decontamination_removes_the_old_background_from_edge_colors() {
        // A red subject on a green background, half mixed along a 4 pixel soft edge.
        let (w, h) = (96usize, 48usize);
        let mut rgb = Vec::new();
        let mut alpha = vec![0u8; w * h];
        for _ in 0..h {
            for x in 0..w {
                let a = ((x as f32 - 46.0) / 4.0 + 0.5).clamp(0.0, 1.0);
                let (fr, fg) = (220.0, 30.0);
                let (br, bg) = (30.0, 220.0);
                let r = fr * a + br * (1.0 - a);
                let g = fg * a + bg * (1.0 - a);
                rgb.extend_from_slice(&[r as u8, g as u8, 30]);
            }
        }
        for y in 0..h {
            for x in 0..w {
                let a = ((x as f32 - 46.0) / 4.0 + 0.5).clamp(0.0, 1.0);
                alpha[y * w + x] = (a * 255.0) as u8;
            }
        }
        let cancel = CancelToken::new();
        let f = foreground_colors(&rgb, &alpha, w, h, &cancel).unwrap();
        // Mid edge pixel: the original is a brown mix; the estimate is much redder.
        let i = (10 * w + 46) * 3;
        assert!(f[i] > rgb[i] + 40, "red {} vs {}", f[i], rgb[i]);
        assert!(f[i + 1] < rgb[i + 1].saturating_sub(40), "green");
        // A solid pixel keeps its color; a far clear pixel is black.
        assert_eq!(
            &f[(10 * w + 90) * 3..(10 * w + 90) * 3 + 3],
            &rgb[(10 * w + 90) * 3..(10 * w + 90) * 3 + 3]
        );
        assert_eq!(&f[(10 * w + 2) * 3..(10 * w + 2) * 3 + 3], &[0, 0, 0]);
    }
}
