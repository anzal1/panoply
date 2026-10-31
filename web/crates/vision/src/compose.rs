//! Compose helpers copied unchanged from panoply-vision cutout.rs.
use crate::error::{ImageError, Result};
use crate::px::{reduce_map, sample, to_linear, to_srgb};
use panoply_core::CancelToken;

/// The subject over a solid color, in linear light (IPC-2: alpha composites in linear light, or
/// soft edges get a dark fringe).
pub fn compose_color(fg: &[u8], alpha: &[u8], w: usize, h: usize, bg: [u8; 3]) -> Vec<u8> {
    let bgl = [to_linear(bg[0]), to_linear(bg[1]), to_linear(bg[2])];
    let mut out = Vec::with_capacity(w * h * 3);
    for (p, &a) in fg.chunks_exact(3).zip(alpha) {
        match a {
            255 => out.extend_from_slice(p),
            0 => out.extend_from_slice(&bg),
            _ => {
                let af = f32::from(a) / 255.0;
                for k in 0..3 {
                    out.push(to_srgb(to_linear(p[k]) * af + bgl[k] * (1.0 - af)));
                }
            }
        }
    }
    out
}

/// The subject over a blurred copy of the picture. The blur is made from the background only
/// (normalized convolution), so the subject's colors never bleed into it.
pub fn compose_blur(
    original: &[u8],
    fg: &[u8],
    alpha: &[u8],
    w: usize,
    h: usize,
    sigma: f32,
    cancel: &CancelToken,
) -> Result<Vec<u8>> {
    let f = (w.max(h) / 800).max(1);
    let s_lo = (sigma / f as f32).max(0.5);
    let weight_of = |i: usize| 1.0 - f32::from(alpha[i]) / 255.0;
    // Weight of the background and the background's linear colors times that weight, on a
    // reduced grid. Blurring both the same way and dividing gives the blur of the background
    // alone.
    let (mut wb, lw, lh) = reduce_map(w, h, f, weight_of);
    crate::px::gaussian_blur(&mut wb, lw, lh, s_lo);
    let mut planes: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (k, plane) in planes.iter_mut().enumerate() {
        let (mut lo, _, _) = reduce_map(w, h, f, |i| to_linear(original[i * 3 + k]) * weight_of(i));
        crate::px::gaussian_blur(&mut lo, lw, lh, s_lo);
        *plane = lo;
    }
    // Where no background is near (the subject fills the picture), fall back to a plain blur of
    // the whole picture there.
    let lacking: Vec<usize> = (0..lw * lh).filter(|&i| wb[i] <= 1e-3).collect();
    for (k, plane) in planes.iter_mut().enumerate() {
        for (v, &wt) in plane.iter_mut().zip(&wb) {
            if wt > 1e-3 {
                *v /= wt;
            }
        }
        if !lacking.is_empty() {
            let (mut all, _, _) = reduce_map(w, h, f, |i| to_linear(original[i * 3 + k]));
            crate::px::gaussian_blur(&mut all, lw, lh, s_lo);
            for &i in &lacking {
                plane[i] = all[i];
            }
        }
    }
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        if y % 256 == 0 && cancel.is_cancelled() {
            return Err(ImageError::Cancelled);
        }
        let fy = (y as f32 + 0.5) / f as f32 - 0.5;
        for x in 0..w {
            let i = y * w + x;
            let a = alpha[i];
            if a == 255 {
                out.extend_from_slice(&fg[i * 3..i * 3 + 3]);
                continue;
            }
            let fx = (x as f32 + 0.5) / f as f32 - 0.5;
            let af = f32::from(a) / 255.0;
            for (k, plane) in planes.iter().enumerate() {
                let b = sample(plane, lw, lh, fx, fy);
                out.push(to_srgb(to_linear(fg[i * 3 + k]) * af + b * (1.0 - af)));
            }
        }
    }
    Ok(out)
}
