//! Pixel work for the PP-OCR pipeline: area resizing, the detector's input tensor, and
//! rotated crops for the recognizer.

use crate::types::OcrImage;

/// Resize 8 bit pixels with an area (triangle when enlarging) filter, separably.
pub fn resize(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    debug_assert_eq!(src.len(), sw * sh * ch);
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let hw = weights(sw, dw);
    let vw = weights(sh, dh);
    // Horizontal pass into f32 rows, then vertical.
    let mut tmp = vec![0f32; dw * sh * ch];
    for y in 0..sh {
        let row = &src[y * sw * ch..(y + 1) * sw * ch];
        let out = &mut tmp[y * dw * ch..(y + 1) * dw * ch];
        for (x, (start, w)) in hw.iter().enumerate() {
            for c in 0..ch {
                let mut acc = 0f32;
                for (k, wk) in w.iter().enumerate() {
                    acc += f32::from(row[(start + k) * ch + c]) * wk;
                }
                out[x * ch + c] = acc;
            }
        }
    }
    let mut out = vec![0u8; dw * dh * ch];
    for (y, (start, w)) in vw.iter().enumerate() {
        for x in 0..dw * ch {
            let mut acc = 0f32;
            for (k, wk) in w.iter().enumerate() {
                acc += tmp[(start + k) * dw * ch + x] * wk;
            }
            out[y * dw * ch + x] = acc.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// For each output sample: the first source index and the normalized weights over the span.
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f64 / dst as f64;
    // Shrinking: a box as wide as the step (area average). Enlarging: a triangle one source
    // sample wide on each side (bilinear).
    let support = if scale > 1.0 { scale / 2.0 } else { 1.0 };
    (0..dst)
        .map(|i| {
            let centre = (i as f64 + 0.5) * scale;
            let lo = ((centre - support).floor().max(0.0)) as usize;
            let hi = (((centre + support).ceil()) as usize).min(src);
            let hi = hi.max(lo + 1).min(src);
            let lo = lo.min(hi - 1);
            let mut w: Vec<f32> = (lo..hi)
                .map(|s| {
                    let d = (s as f64 + 0.5 - centre).abs();
                    if scale > 1.0 {
                        // Partial coverage at the ends of the box.
                        let left = (s as f64).max(centre - support);
                        let right = ((s + 1) as f64).min(centre + support);
                        (right - left).max(0.0) as f32
                    } else {
                        (1.0 - d).max(0.0) as f32
                    }
                })
                .collect();
            let sum: f32 = w.iter().sum();
            if sum <= 0.0 {
                w.iter_mut().for_each(|x| *x = 1.0);
            }
            let sum: f32 = w.iter().sum();
            w.iter_mut().for_each(|x| *x /= sum);
            (lo, w)
        })
        .collect()
}

/// A rectangle with a direction: `w` along the text, `h` across it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oriented {
    /// Centre x in image pixels.
    pub cx: f32,
    /// Centre y in image pixels.
    pub cy: f32,
    /// Length along the reading direction.
    pub w: f32,
    /// Thickness across it.
    pub h: f32,
    /// Angle of the reading direction in radians, clockwise on screen (y down).
    pub angle: f32,
}

#[cfg(test)]
impl Oriented {
    /// The axis aligned box around the rectangle, `(x, y, w, h)`.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let (s, c) = self.angle.sin_cos();
        let ex = (self.w * c.abs() + self.h * s.abs()) / 2.0;
        let ey = (self.w * s.abs() + self.h * c.abs()) / 2.0;
        (self.cx - ex, self.cy - ey, 2.0 * ex, 2.0 * ey)
    }
}

/// Image mean and deviation the detector was trained with, applied to B, G, R.
const DET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const DET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The page resized for the detector: 8 bit, `ch` channels (1 or 3) of `dw x dh`.
pub fn det_resize(img: &OcrImage, dw: usize, dh: usize) -> Vec<u8> {
    resize(
        &img.data,
        img.width as usize,
        img.height as usize,
        usize::from(img.channels),
        dw,
        dh,
    )
}

/// The detector's input for rows `y0..y1` of a resized page: `3 x (y1 - y0) x dw` floats, channels
/// in B, G, R order (the order PaddleOCR feeds its detector, whose means are written in R, G, B
/// order but applied to B, G, R).
pub fn det_tensor(px: &[u8], ch: usize, dw: usize, y0: usize, y1: usize) -> Vec<f32> {
    let rows = y1 - y0;
    let plane = dw * rows;
    let mut out = vec![0f32; 3 * plane];
    for i in 0..plane {
        let src = (y0 * dw + i) * ch;
        let (r, g, b) = if ch == 3 {
            (px[src], px[src + 1], px[src + 2])
        } else {
            (px[src], px[src], px[src])
        };
        // Channel 0 is B, normalized with the first mean and deviation.
        let bgr = [b, g, r];
        for c in 0..3 {
            out[c * plane + i] = (f32::from(bgr[c]) / 255.0 - DET_MEAN[c]) / DET_STD[c];
        }
    }
    out
}

/// A rotated crop for the recognizer: the rectangle straightened and scaled to `out_h` rows,
/// `3 x out_h x out_w` floats in B, G, R order, normalized to -1..1. Samples are averaged over
/// a small grid when the crop shrinks, so thin strokes do not alias.
pub fn rec_tensor(img: &OcrImage, rect: &Oriented, out_h: usize, out_w: usize) -> Vec<f32> {
    let (iw, ih) = (img.width as usize, img.height as usize);
    let ch = usize::from(img.channels);
    let (s, c) = rect.angle.sin_cos();
    let sx = ((rect.w / out_w as f32).round() as usize).clamp(1, 4);
    let sy = ((rect.h / out_h as f32).round() as usize).clamp(1, 4);
    let plane = out_h * out_w;
    let mut out = vec![0f32; 3 * plane];
    let get = |x: i64, y: i64, k: usize| -> f32 {
        let xi = x.clamp(0, iw as i64 - 1) as usize;
        let yi = y.clamp(0, ih as i64 - 1) as usize;
        f32::from(img.data[(yi * iw + xi) * ch + k.min(ch - 1)])
    };
    for j in 0..out_h {
        for i in 0..out_w {
            let mut acc = [0f32; 3];
            for b in 0..sy {
                for a in 0..sx {
                    let fu = (i as f32 + (a as f32 + 0.5) / sx as f32) / out_w as f32 - 0.5;
                    let fv = (j as f32 + (b as f32 + 0.5) / sy as f32) / out_h as f32 - 0.5;
                    let (lu, lv) = (fu * rect.w, fv * rect.h);
                    let x = rect.cx + lu * c - lv * s - 0.5;
                    let y = rect.cy + lu * s + lv * c - 0.5;
                    let (x0, y0) = (x.floor(), y.floor());
                    let (tx, ty) = (x - x0, y - y0);
                    let (x0, y0) = (x0 as i64, y0 as i64);
                    for (k, a) in acc.iter_mut().enumerate() {
                        // k is B, G, R: source channel 2 - k for color, 0 for gray.
                        let sk = if ch == 3 { 2 - k } else { 0 };
                        let p00 = get(x0, y0, sk);
                        let p10 = get(x0 + 1, y0, sk);
                        let p01 = get(x0, y0 + 1, sk);
                        let p11 = get(x0 + 1, y0 + 1, sk);
                        *a += (p00 * (1.0 - tx) + p10 * tx) * (1.0 - ty)
                            + (p01 * (1.0 - tx) + p11 * tx) * ty;
                    }
                }
            }
            let n = (sx * sy) as f32;
            for (k, a) in acc.iter().enumerate() {
                out[k * plane + j * out_w + i] = (a / n / 255.0 - 0.5) / 0.5;
            }
        }
    }
    out
}

/// The bounds, in image pixels, of the part of a line rectangle between `u0` and `u1` measured
/// along the line from its centre (negative to the left), as `(x, y, w, h)`.
pub fn span_bounds(rect: &Oriented, u0: f32, u1: f32) -> (f32, f32, f32, f32) {
    let (s, c) = rect.angle.sin_cos();
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    for (u, v) in [
        (u0, -rect.h / 2.0),
        (u1, -rect.h / 2.0),
        (u0, rect.h / 2.0),
        (u1, rect.h / 2.0),
    ] {
        let x = rect.cx + u * c - v * s;
        let y = rect.cy + u * s + v * c;
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    (min.0, min.1, max.0 - min.0, max.1 - min.1)
}

/// A straightened crop of a line as 8 bit gray, `out_h` rows high (the width follows the
/// line's shape) with `pad` background pixels on every side. Returns the pixels, the width and
/// the height including the padding.
pub fn gray_crop(
    img: &OcrImage,
    rect: &Oriented,
    out_h: usize,
    pad: usize,
) -> (Vec<u8>, usize, usize) {
    let out_w = ((out_h as f32 * rect.w / rect.h.max(1.0)).ceil() as usize).clamp(16, 8000);
    let t = rec_tensor(img, rect, out_h, out_w);
    // Channel 1 (G) back to 8 bit.
    let plane = &t[out_h * out_w..2 * out_h * out_w];
    let gray: Vec<u8> = plane
        .iter()
        .map(|v| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();
    // The paper: the 90th percentile of the crop.
    let mut sorted = gray.clone();
    sorted.sort_unstable();
    let bg = sorted[sorted.len() * 9 / 10];
    let (w, h) = (out_w + 2 * pad, out_h + 2 * pad);
    let mut out = vec![bg; w * h];
    for y in 0..out_h {
        out[(y + pad) * w + pad..(y + pad) * w + pad + out_w]
            .copy_from_slice(&gray[y * out_w..(y + 1) * out_w]);
    }
    (out, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_a_flat_image_keeps_it_flat() {
        let src = vec![200u8; 40 * 30];
        for (dw, dh) in [(20, 15), (80, 60), (33, 17), (40, 30)] {
            let out = resize(&src, 40, 30, 1, dw, dh);
            assert_eq!(out.len(), dw * dh);
            assert!(out.iter().all(|&v| v == 200), "{dw}x{dh}");
        }
    }

    #[test]
    fn shrinking_averages_instead_of_dropping_samples() {
        // Alternating columns of 0 and 255 shrunk 2x must come out mid gray, not black or white.
        let mut src = vec![0u8; 8 * 2];
        for y in 0..2 {
            for x in 0..8 {
                src[y * 8 + x] = if x % 2 == 0 { 0 } else { 255 };
            }
        }
        let out = resize(&src, 8, 2, 1, 4, 1);
        assert!(out.iter().all(|&v| (126..=129).contains(&v)), "{out:?}");
    }

    #[test]
    fn the_detector_input_is_bgr_normalized() {
        // One red pixel image.
        let img = OcrImage::rgb(32, 32, [255u8, 0, 0].repeat(32 * 32)).unwrap();
        let t = det_tensor(&det_resize(&img, 32, 32), 3, 32, 0, 32);
        assert_eq!(t.len(), 3 * 32 * 32);
        // Channel 0 is blue (0 -> -mean/std), channel 2 is red (255 -> (1-mean)/std).
        assert!((t[0] - (0.0 - 0.485) / 0.229).abs() < 1e-4);
        assert!((t[2 * 32 * 32] - (1.0 - 0.406) / 0.225).abs() < 1e-4);
    }

    #[test]
    fn a_straight_crop_copies_the_pixels() {
        // 8 x 4 ramp, crop the whole image at its own size.
        let data: Vec<u8> = (0..32).map(|i| (i * 8) as u8).collect();
        let img = OcrImage::gray(8, 4, data.clone()).unwrap();
        let r = Oriented {
            cx: 4.0,
            cy: 2.0,
            w: 8.0,
            h: 4.0,
            angle: 0.0,
        };
        let t = rec_tensor(&img, &r, 4, 8);
        for (i, &v) in data.iter().enumerate() {
            let back = (t[i] * 0.5 + 0.5) * 255.0;
            assert!((back - f32::from(v)).abs() < 0.5, "{i}: {back} vs {v}");
        }
    }

    #[test]
    fn a_tilted_rectangle_is_straightened() {
        // A bright horizontal bar drawn at 10 degrees on a dark image; crop along it.
        let (w, h) = (200usize, 120usize);
        let ang = 10f32.to_radians();
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f32 - 100.0, y as f32 - 60.0);
                let u = dx * ang.cos() + dy * ang.sin();
                let v = -dx * ang.sin() + dy * ang.cos();
                if u.abs() < 70.0 && v.abs() < 8.0 {
                    data[y * w + x] = 255;
                }
            }
        }
        let img = OcrImage::gray(w as u32, h as u32, data).unwrap();
        let r = Oriented {
            cx: 100.0,
            cy: 60.0,
            w: 140.0,
            h: 32.0,
            angle: ang,
        };
        let t = rec_tensor(&img, &r, 32, 140);
        // After straightening, the bar fills the middle 16 rows and is dark above and below.
        let mid: f32 = (0..140).map(|x| t[16 * 140 + x]).sum::<f32>() / 140.0;
        let edge: f32 = (0..140).map(|x| t[2 * 140 + x]).sum::<f32>() / 140.0;
        assert!(mid > 0.9, "mid {mid}");
        assert!(edge < -0.9, "edge {edge}");
    }

    #[test]
    fn bounds_of_a_rotated_rectangle_hold_it() {
        let r = Oriented {
            cx: 50.0,
            cy: 50.0,
            w: 100.0,
            h: 20.0,
            angle: 0.0,
        };
        assert_eq!(r.bounds(), (0.0, 40.0, 100.0, 20.0));
        let r2 = Oriented {
            angle: std::f32::consts::FRAC_PI_2,
            ..r
        };
        let (x, y, w, h) = r2.bounds();
        assert!((x - 40.0).abs() < 1e-3 && (y - 0.0).abs() < 1e-3);
        assert!((w - 20.0).abs() < 1e-3 && (h - 100.0).abs() < 1e-3);
    }
}
