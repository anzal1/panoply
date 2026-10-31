//! Shrinking images by area averaging: every output pixel is the average of the source pixels it
//! covers, with fractional weights at the edges. It is the honest filter for shrinking (no
//! aliasing, no ringing) and it works in constant extra memory, one output row at a time.

/// Shrink interleaved 8-bit samples from `sw x sh` to `dw x dh` (each at most the source size).
pub fn shrink(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    assert!(dw >= 1 && dh >= 1 && dw <= sw && dh <= sh);
    assert_eq!(src.len(), sw * sh * ch);
    if dw == sw && dh == sh {
        return src.to_vec();
    }
    // Horizontal spans: for each output column, the first source column and the weights.
    let xs = spans(sw, dw);
    let ys = spans(sh, dh);
    let mut out = vec![0u8; dw * dh * ch];
    let mut row_cache: Option<(usize, Vec<f32>)> = None;
    let mut acc = vec![0f32; dw * ch];
    for (dy, (y0, wy)) in ys.iter().enumerate() {
        acc.iter_mut().for_each(|v| *v = 0.0);
        for (k, wgt) in wy.iter().enumerate() {
            let sy = y0 + k;
            if row_cache.as_ref().map(|(cy, _)| *cy) != Some(sy) {
                row_cache = Some((
                    sy,
                    h_row(&src[sy * sw * ch..(sy + 1) * sw * ch], ch, &xs, dw),
                ));
            }
            let row = &row_cache.as_ref().expect("row was just cached").1;
            for (a, v) in acc.iter_mut().zip(row) {
                *a += v * wgt;
            }
        }
        let dst = &mut out[dy * dw * ch..(dy + 1) * dw * ch];
        for (o, a) in dst.iter_mut().zip(&acc) {
            *o = (a + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Enlarge interleaved 8-bit samples from `sw x sh` to `dw x dh` (each at least the source size)
/// with bilinear interpolation, the way a viewer stretches a small picture over a larger area.
pub fn enlarge(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    assert!(sw >= 1 && sh >= 1 && dw >= sw && dh >= sh);
    assert_eq!(src.len(), sw * sh * ch);
    if (dw, dh) == (sw, sh) {
        return src.to_vec();
    }
    // Source position of an output pixel centre, with the neighbours and the weight of the second.
    let axis = |n_src: usize, n_dst: usize| -> Vec<(usize, usize, f32)> {
        (0..n_dst)
            .map(|d| {
                let pos = ((d as f64 + 0.5) * n_src as f64 / n_dst as f64 - 0.5)
                    .clamp(0.0, (n_src - 1) as f64);
                let i0 = pos.floor() as usize;
                let i1 = (i0 + 1).min(n_src - 1);
                (i0, i1, (pos - i0 as f64) as f32)
            })
            .collect()
    };
    let (xs, ys) = (axis(sw, dw), axis(sh, dh));
    let mut out = vec![0u8; dw * dh * ch];
    for (dy, &(y0, y1, fy)) in ys.iter().enumerate() {
        let (r0, r1) = (
            &src[y0 * sw * ch..(y0 + 1) * sw * ch],
            &src[y1 * sw * ch..(y1 + 1) * sw * ch],
        );
        let dst = &mut out[dy * dw * ch..(dy + 1) * dw * ch];
        for (dx, &(x0, x1, fx)) in xs.iter().enumerate() {
            for c in 0..ch {
                let top = f32::from(r0[x0 * ch + c]) * (1.0 - fx) + f32::from(r0[x1 * ch + c]) * fx;
                let bot = f32::from(r1[x0 * ch + c]) * (1.0 - fx) + f32::from(r1[x1 * ch + c]) * fx;
                dst[dx * ch + c] = (top * (1.0 - fy) + bot * fy + 0.5) as u8;
            }
        }
    }
    out
}

/// For each of `dn` output positions: first source index, and normalised weights.
fn spans(sn: usize, dn: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = sn as f64 / dn as f64;
    (0..dn)
        .map(|d| {
            let lo = d as f64 * scale;
            let hi = ((d + 1) as f64 * scale).min(sn as f64);
            let first = lo.floor() as usize;
            let last = (hi.ceil() as usize).max(first + 1).min(sn);
            let mut w: Vec<f32> = (first..last)
                .map(|i| {
                    let a = (i as f64).max(lo);
                    let b = ((i + 1) as f64).min(hi);
                    (b - a).max(0.0) as f32
                })
                .collect();
            let total: f32 = w.iter().sum();
            if total > 0.0 {
                w.iter_mut().for_each(|x| *x /= total);
            } else {
                w = vec![1.0];
            }
            (first, w)
        })
        .collect()
}

fn h_row(row: &[u8], ch: usize, xs: &[(usize, Vec<f32>)], dw: usize) -> Vec<f32> {
    let mut out = vec![0f32; dw * ch];
    for (dx, (x0, wx)) in xs.iter().enumerate() {
        for c in 0..ch {
            let mut s = 0f32;
            for (k, w) in wx.iter().enumerate() {
                s += f32::from(row[(x0 + k) * ch + c]) * w;
            }
            out[dx * ch + c] = s;
        }
    }
    out
}

/// The output size for shrinking by `scale` (0 to 1), never below one pixel.
pub fn scaled_size(w: usize, h: usize, scale: f64) -> (usize, usize) {
    let s = scale.clamp(0.0, 1.0);
    (
        ((w as f64 * s).round() as usize).clamp(1, w),
        ((h as f64 * s).round() as usize).clamp(1, h),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_blocks() {
        // 4x2 gray: [0 100 | 200 40] / [100 0 | 40 200]
        let src = [0u8, 100, 200, 40, 100, 0, 40, 200];
        let out = shrink(&src, 4, 2, 1, 2, 1);
        assert_eq!(out, vec![50, 120]);
    }

    #[test]
    fn flat_images_stay_flat_at_any_ratio() {
        let src = vec![77u8; 101 * 53 * 3];
        let out = shrink(&src, 101, 53, 3, 33, 17);
        assert!(out.iter().all(|&v| v == 77));
        assert_eq!(out.len(), 33 * 17 * 3);
    }

    #[test]
    fn enlarging_keeps_flat_areas_and_ramps() {
        let flat = vec![90u8; 7 * 5 * 3];
        assert!(enlarge(&flat, 7, 5, 3, 29, 21).iter().all(|&v| v == 90));
        let ramp = [0u8, 100, 200];
        let out = enlarge(&ramp, 3, 1, 1, 12, 1);
        assert_eq!(out.len(), 12);
        assert!(out.windows(2).all(|p| p[0] <= p[1]));
        assert_eq!((out[0], out[11]), (0, 200));
    }

    #[test]
    fn identity_and_sizes() {
        let src: Vec<u8> = (0..60).collect();
        assert_eq!(shrink(&src, 10, 6, 1, 10, 6), src);
        assert_eq!(scaled_size(100, 50, 0.5), (50, 25));
        assert_eq!(scaled_size(3, 3, 0.01), (1, 1));
    }
}
