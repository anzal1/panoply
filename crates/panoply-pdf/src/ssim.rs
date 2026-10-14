//! Structural similarity (SSIM) of two 8-bit gray images, our own code (ADR 0002: shared engine,
//! no dependency).
//!
//! Definition, so a reported number can be reproduced: 8x8 windows, uniform weights, population
//! variance, K1 = 0.01, K2 = 0.03, data range 255, mean over windows. Windows start every 4
//! pixels, and every few more on very large images so a comparison costs at most about half a
//! million windows. Images smaller than one window are compared as a single window.

const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
const WIN: usize = 8;

/// Mean SSIM of two gray images of the same size. 1.0 is identical.
pub fn ssim_gray(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
    assert_eq!(a.len(), w * h);
    assert_eq!(b.len(), w * h);
    if w == 0 || h == 0 {
        return 1.0;
    }
    if w < WIN || h < WIN {
        return window(a, b, w, 0, 0, w, h);
    }
    let stride = (((w * h) as f64 / 500_000.0).sqrt().ceil() as usize).max(4);
    let mut total = 0.0;
    let mut count = 0u64;
    let mut y = 0;
    while y + WIN <= h {
        let mut x = 0;
        while x + WIN <= w {
            total += window(a, b, w, x, y, WIN, WIN);
            count += 1;
            x += stride;
        }
        y += stride;
    }
    if count == 0 {
        1.0
    } else {
        total / count as f64
    }
}

/// Like [`ssim_gray`], but only over the windows that touch at least one pixel where `vis` is 1
/// (one byte per pixel). It is the measure for a picture that is only partly shown, such as the
/// colour layer of a scan under a stencil mask: what the mask hides cannot look wrong. Returns 1.0
/// when nothing is shown.
pub fn ssim_gray_masked(a: &[u8], b: &[u8], w: usize, h: usize, vis: &[u8]) -> f64 {
    assert_eq!(a.len(), w * h);
    assert_eq!(b.len(), w * h);
    assert_eq!(vis.len(), w * h);
    if w < WIN || h < WIN {
        return if vis.contains(&1) {
            window(a, b, w, 0, 0, w, h)
        } else {
            1.0
        };
    }
    // Summed-area table of the visible pixels: "does this window touch any" in four lookups.
    let sw = w + 1;
    let mut sat = vec![0u32; sw * (h + 1)];
    for y in 0..h {
        let mut run = 0u32;
        for x in 0..w {
            run += u32::from(vis[y * w + x] != 0);
            sat[(y + 1) * sw + x + 1] = sat[y * sw + x + 1] + run;
        }
    }
    let touched = |x: usize, y: usize| {
        sat[(y + WIN) * sw + x + WIN] + sat[y * sw + x]
            > sat[y * sw + x + WIN] + sat[(y + WIN) * sw + x]
    };
    let stride = (((w * h) as f64 / 500_000.0).sqrt().ceil() as usize).max(4);
    let (mut total, mut count) = (0.0, 0u64);
    let mut y = 0;
    while y + WIN <= h {
        let mut x = 0;
        while x + WIN <= w {
            if touched(x, y) {
                total += window(a, b, w, x, y, WIN, WIN);
                count += 1;
            }
            x += stride;
        }
        y += stride;
    }
    if count == 0 {
        1.0
    } else {
        total / count as f64
    }
}

fn window(a: &[u8], b: &[u8], w: usize, x0: usize, y0: usize, ww: usize, wh: usize) -> f64 {
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for y in y0..y0 + wh {
        let ra = &a[y * w + x0..y * w + x0 + ww];
        let rb = &b[y * w + x0..y * w + x0 + ww];
        for (&p, &q) in ra.iter().zip(rb) {
            let (p, q) = (u64::from(p), u64::from(q));
            sx += p;
            sy += q;
            sxx += p * p;
            syy += q * q;
            sxy += p * q;
        }
    }
    let n = (ww * wh) as f64;
    let (mx, my) = (sx as f64 / n, sy as f64 / n);
    let vx = sxx as f64 / n - mx * mx;
    let vy = syy as f64 / n - my * my;
    let cov = sxy as f64 / n - mx * my;
    ((2.0 * mx * my + C1) * (2.0 * cov + C2)) / ((mx * mx + my * my + C1) * (vx + vy + C2))
}

/// Luma of interleaved 8-bit samples (1 or 3 channels).
pub fn luma(data: &[u8], channels: usize) -> Vec<u8> {
    if channels == 1 {
        return data.to_vec();
    }
    data.chunks_exact(channels)
        .map(|p| ((77 * u32::from(p[0]) + 150 * u32::from(p[1]) + 29 * u32::from(p[2])) >> 8) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        (0..w * h)
            .map(|i| ((i % w) * 255 / w.max(1)) as u8)
            .collect()
    }

    #[test]
    fn the_masked_score_ignores_what_is_hidden() {
        let (w, h) = (64, 32);
        let a = gradient(w, h);
        let mut b = a.clone();
        // Wreck the right half; show only the left half.
        for y in 0..h {
            for x in 32..w {
                b[y * w + x] = 255 - b[y * w + x];
            }
        }
        let vis: Vec<u8> = (0..w * h).map(|i| u8::from(i % w < 24)).collect();
        assert!(ssim_gray(&a, &b, w, h) < 0.9);
        assert!((ssim_gray_masked(&a, &b, w, h, &vis) - 1.0).abs() < 1e-12);
        // Show everything and the damage counts.
        let all = vec![1u8; w * h];
        assert!(ssim_gray_masked(&a, &b, w, h, &all) < 0.9);
        assert_eq!(ssim_gray_masked(&a, &b, w, h, &vec![0u8; w * h]), 1.0);
    }

    #[test]
    fn identical_images_score_one() {
        let a = gradient(64, 48);
        assert!((ssim_gray(&a, &a, 64, 48) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn noise_and_inversion_lower_the_score() {
        let a = gradient(64, 64);
        let mut noisy = a.clone();
        for (i, v) in noisy.iter_mut().enumerate() {
            *v = v.wrapping_add(((i * 2654435761usize) >> 13) as u8 % 40);
        }
        let s_noisy = ssim_gray(&a, &noisy, 64, 64);
        let inv: Vec<u8> = a.iter().map(|v| 255 - v).collect();
        let s_inv = ssim_gray(&a, &inv, 64, 64);
        assert!(s_noisy < 0.95, "{s_noisy}");
        assert!(s_inv < s_noisy, "{s_inv} {s_noisy}");
    }

    #[test]
    fn tiny_images_and_luma() {
        let a = vec![10u8; 9];
        assert!((ssim_gray(&a, &a, 3, 3) - 1.0).abs() < 1e-12);
        assert_eq!(luma(&[255, 255, 255, 0, 0, 0], 3), vec![255, 0]);
    }
}
