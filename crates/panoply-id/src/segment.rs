//! Cutting the person out of the picture, so the background can be made what the rule wants.
//!
//! The engine ladder (docs/FLOWS.md, ADR 0004): on macOS the system's foreground subject mask
//! (`VNGenerateForegroundInstanceMaskRequest`, soft edges, hair included). Where it is not there
//! (or finds nothing) a plain colour fallback: take the backdrop colour from the picture's border,
//! grow it inward through pixels that stay close to it and change slowly (a studio gradient is
//! fine), and call everything it did not reach the person. That works on a plain wall or sheet, which
//! is what a passport photo is taken against, and it says so honestly when the backdrop is too
//! busy to separate (a flag, a bookshelf): [`Matte::reliable`] is then false and the caller does
//! not paint over a guess.
//!
//! The matte is kept at its own resolution (the long side is at most [`MATTE_SIDE`]) and carries
//! the ratio to the picture, so the crop can sample it with the same map as the pixels.

use panoply_image::error::Result;
use panoply_image::ops::kb::resample_image;
use panoply_image::types::{Image, Layout, Samples};

use crate::face::{EnginePref, Face};

/// Long side of the working picture the matte is made at.
pub const MATTE_SIDE: u32 = 1280;

/// Which engine made the matte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatteEngine {
    /// Apple Vision's foreground subject mask.
    Vision,
    /// The border colour fallback.
    Color,
}

impl MatteEngine {
    /// A name for the result card.
    pub fn label(self) -> &'static str {
        match self {
            MatteEngine::Vision => "Apple Vision subject mask",
            MatteEngine::Color => "plain backdrop colour cut",
        }
    }
}

/// A soft cut-out: 255 is the person, 0 the backdrop.
#[derive(Debug, Clone)]
pub struct Matte {
    /// One byte per pixel.
    pub alpha: Image,
    /// Matte pixels per picture pixel.
    pub scale: f64,
    /// Which engine made it.
    pub engine: MatteEngine,
    /// False when the cut should not be trusted (busy backdrop, nothing found).
    pub reliable: bool,
    /// The backdrop colour found at the border, for the card.
    pub backdrop: [u8; 3],
    /// How uniform the backdrop was, 0 to 1 (1 is one flat colour).
    pub uniform: f64,
}

impl Matte {
    /// Matte value at a position in picture pixels.
    pub fn at(&self, x: f64, y: f64) -> u8 {
        let Samples::U8(v) = &self.alpha.samples else {
            return 0;
        };
        let (mx, my) = (
            ((x * self.scale) as i64).clamp(0, i64::from(self.alpha.width) - 1),
            ((y * self.scale) as i64).clamp(0, i64::from(self.alpha.height) - 1),
        );
        v[my as usize * self.alpha.width as usize + mx as usize]
    }

    /// The topmost row, in picture pixels, where the person covers at least `min_frac` of the
    /// columns `x0..x1` (picture pixels). `None` when nothing is there.
    pub fn top_in_band(&self, x0: f64, x1: f64, min_frac: f64) -> Option<f64> {
        let Samples::U8(v) = &self.alpha.samples else {
            return None;
        };
        let w = self.alpha.width as usize;
        let a = ((x0 * self.scale).max(0.0) as usize).min(w.saturating_sub(1));
        let b = ((x1 * self.scale).max(0.0).ceil() as usize).clamp(a + 1, w);
        let need = ((b - a) as f64 * min_frac).ceil().max(1.0) as usize;
        for y in 0..self.alpha.height as usize {
            let row = &v[y * w + a..y * w + b];
            if row.iter().filter(|&&p| p >= 128).count() >= need {
                return Some(y as f64 / self.scale);
            }
        }
        None
    }
}

/// Make the matte for `img` (8 bit RGB) around `face`.
pub fn compute(img: &Image, face: &Face, pref: EnginePref) -> Result<Matte> {
    let long = img.width.max(img.height);
    let scale = if long > MATTE_SIDE {
        f64::from(MATTE_SIDE) / f64::from(long)
    } else {
        1.0
    };
    let small;
    let work = if scale < 1.0 {
        small = resample_image(
            img,
            ((f64::from(img.width) * scale).round() as u32).max(1),
            ((f64::from(img.height) * scale).round() as u32).max(1),
        )?;
        &small
    } else {
        img
    };
    let actual = f64::from(work.width) / f64::from(img.width);
    #[cfg(target_os = "macos")]
    {
        use panoply_platform_macos::faces;
        if pref != EnginePref::YuNet && faces::subject_mask_available() {
            let Samples::U8(px) = &work.samples else {
                unreachable!("the caller passes 8 bit RGB");
            };
            if let Ok(Some(mask)) = faces::subject_mask(px, work.width, work.height) {
                let alpha = Image {
                    width: work.width,
                    height: work.height,
                    layout: Layout::Gray,
                    samples: Samples::U8(mask),
                    icc: None,
                };
                let (backdrop, uniform) = border_stats(work, &alpha);
                let covered = coverage(&alpha);
                return Ok(Matte {
                    alpha,
                    scale: actual,
                    engine: MatteEngine::Vision,
                    reliable: covered > 0.03 && covered < 0.97,
                    backdrop,
                    uniform,
                });
            }
        }
    }
    let _ = pref;
    Ok(color_cut(work, actual, face))
}

fn coverage(alpha: &Image) -> f64 {
    let Samples::U8(v) = &alpha.samples else {
        return 0.0;
    };
    v.iter().filter(|&&p| p >= 128).count() as f64 / v.len().max(1) as f64
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn px3(v: &[u8], i: usize) -> [f32; 3] {
    [
        f32::from(v[i * 3]),
        f32::from(v[i * 3 + 1]),
        f32::from(v[i * 3 + 2]),
    ]
}

/// Median colour of the border pixels the matte calls backdrop, and how much of the border
/// sits near that colour.
fn border_stats(img: &Image, alpha: &Image) -> ([u8; 3], f64) {
    let (Samples::U8(v), Samples::U8(a)) = (&img.samples, &alpha.samples) else {
        return ([255; 3], 0.0);
    };
    let (w, h) = (img.width as usize, img.height as usize);
    let m = (w.min(h) / 25).max(2);
    let mut pts: Vec<[f32; 3]> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if (x < m || y < m || x >= w - m) && a[y * w + x] < 64 {
                pts.push(px3(v, y * w + x));
            }
        }
    }
    if pts.is_empty() {
        return ([255; 3], 0.0);
    }
    let mut med = [0f32; 3];
    for c in 0..3 {
        let mut ch: Vec<f32> = pts.iter().map(|p| p[c]).collect();
        ch.sort_by(f32::total_cmp);
        med[c] = ch[ch.len() / 2];
    }
    let near = pts.iter().filter(|p| dist3(**p, med) < 40.0).count();
    (
        [med[0] as u8, med[1] as u8, med[2] as u8],
        near as f64 / pts.len() as f64,
    )
}

/// The plain backdrop fallback.
fn color_cut(img: &Image, scale: f64, face: &Face) -> Matte {
    let Samples::U8(v) = &img.samples else {
        unreachable!("the caller passes 8 bit RGB");
    };
    let (w, h) = (img.width as usize, img.height as usize);
    // The ring the backdrop is read from: top rows and the side columns, away from the face.
    let m = (w.min(h) / 25).max(2);
    let mut ring: Vec<usize> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if y < m || x < m || x >= w - m {
                ring.push(y * w + x);
            }
        }
    }
    let mut med = [0f32; 3];
    for (c, slot) in med.iter_mut().enumerate() {
        let mut ch: Vec<f32> = ring.iter().map(|&i| px3(v, i)[c]).collect();
        ch.sort_by(f32::total_cmp);
        *slot = ch[ch.len() / 2];
    }
    // Tolerances in 8 bit units: how far from the median colour a pixel may be (a lit gradient
    // moves a good deal), and how fast the colour may change from one pixel to the next.
    let (far, step) = (70.0f32, 9.0f32);
    let mut bg = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    for &i in &ring {
        if dist3(px3(v, i), med) < far && !bg[i] {
            bg[i] = true;
            stack.push(i);
        }
    }
    let seeds = stack.len();
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let here = px3(v, i);
        let try_n = |j: usize, stack: &mut Vec<usize>, bg: &mut Vec<bool>| {
            if !bg[j] {
                let c = px3(v, j);
                if dist3(c, here) < step && dist3(c, med) < far {
                    bg[j] = true;
                    stack.push(j);
                }
            }
        };
        if x > 0 {
            try_n(i - 1, &mut stack, &mut bg);
        }
        if x + 1 < w {
            try_n(i + 1, &mut stack, &mut bg);
        }
        if y > 0 {
            try_n(i - w, &mut stack, &mut bg);
        }
        if y + 1 < h {
            try_n(i + w, &mut stack, &mut bg);
        }
    }
    let uniform = seeds as f64 / ring.len() as f64;
    // The face is always the person, whatever its colour does.
    let (cx, cy) = (
        (face.x + face.w / 2.0) * scale,
        (face.y + face.h / 2.0) * scale,
    );
    let (rx, ry) = (face.w * 0.3 * scale, face.h * 0.35 * scale);
    let mut alpha_f = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let inside = ((x as f64 - cx) / rx).powi(2) + ((y as f64 - cy) / ry).powi(2) <= 1.0;
            alpha_f[i] = if inside || !bg[i] { 1.0 } else { 0.0 };
        }
    }
    // Drop specks: keep person regions that touch the face or are large.
    let alpha_f = keep_main_region(
        &alpha_f,
        w,
        h,
        (cx as usize).min(w - 1),
        (cy as usize).min(h - 1),
    );
    let soft = blur(&alpha_f, w, h, 2);
    let bytes: Vec<u8> = soft
        .iter()
        .map(|&p| (p.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();
    let alpha = Image {
        width: img.width,
        height: img.height,
        layout: Layout::Gray,
        samples: Samples::U8(bytes),
        icc: None,
    };
    let covered = coverage(&alpha);
    Matte {
        alpha,
        scale,
        engine: MatteEngine::Color,
        // A flat backdrop reaches most of the border, and the cut leaves a person-sized shape.
        reliable: uniform > 0.8 && covered > 0.05 && covered < 0.9,
        backdrop: [med[0] as u8, med[1] as u8, med[2] as u8],
        uniform,
    }
}

/// Keep the 4-connected region of `alpha >= 0.5` that holds `(sx, sy)`, plus any other region of
/// at least 2 percent of the picture (a raised hand, a shoulder cut by the frame).
fn keep_main_region(alpha: &[f32], w: usize, h: usize, sx: usize, sy: usize) -> Vec<f32> {
    let mut label = vec![0u32; w * h];
    let mut sizes: Vec<usize> = vec![0];
    let mut next = 1u32;
    let mut stack = Vec::new();
    for start in 0..w * h {
        if alpha[start] < 0.5 || label[start] != 0 {
            continue;
        }
        let id = next;
        next += 1;
        sizes.push(0);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            sizes[id as usize] += 1;
            let (x, y) = (i % w, i / w);
            let mut push = |j: usize| {
                if alpha[j] >= 0.5 && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
    }
    let main = label[sy * w + sx];
    let floor = (w * h) / 50;
    alpha
        .iter()
        .zip(&label)
        .map(|(_, &l)| {
            if l != 0 && (l == main || sizes[l as usize] >= floor) {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}

/// A box blur of radius `r`, applied twice (close to a Gaussian), on a single channel.
fn blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let pass = |inp: &[f32]| -> Vec<f32> {
        let mut tmp = vec![0f32; w * h];
        for y in 0..h {
            let row = &inp[y * w..(y + 1) * w];
            let mut acc: f32 = row[..=r.min(w - 1)].iter().sum();
            let mut count = r.min(w - 1) + 1;
            for x in 0..w {
                tmp[y * w + x] = acc / count as f32;
                if x + r + 1 < w {
                    acc += row[x + r + 1];
                    count += 1;
                }
                if x >= r {
                    acc -= row[x - r];
                    count -= 1;
                }
            }
        }
        let mut out = vec![0f32; w * h];
        for x in 0..w {
            let mut acc: f32 = (0..=r.min(h - 1)).map(|y| tmp[y * w + x]).sum();
            let mut count = r.min(h - 1) + 1;
            for y in 0..h {
                out[y * w + x] = acc / count as f32;
                if y + r + 1 < h {
                    acc += tmp[(y + r + 1) * w + x];
                    count += 1;
                }
                if y >= r {
                    acc -= tmp[(y - r) * w + x];
                    count -= 1;
                }
            }
        }
        out
    };
    pass(&pass(src))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::Engine;

    /// A blue studio gradient with a skin coloured head and a dark jacket.
    fn scene(w: u32, h: u32) -> (Image, Vec<bool>, Face) {
        let mut v = Vec::new();
        let mut truth = vec![false; (w * h) as usize];
        let (cx, cy, rx, ry) = (
            f64::from(w) / 2.0,
            f64::from(h) * 0.38,
            f64::from(w) * 0.16,
            f64::from(h) * 0.2,
        );
        for y in 0..h {
            for x in 0..w {
                let t = f64::from(y) / f64::from(h);
                let mut c = [
                    (70.0 + 40.0 * t) as u8,
                    (110.0 + 40.0 * t) as u8,
                    (190.0 - 30.0 * t) as u8,
                ];
                let head =
                    ((f64::from(x) - cx) / rx).powi(2) + ((f64::from(y) - cy) / ry).powi(2) <= 1.0;
                let body = f64::from(y) > cy + ry * 0.9
                    && f64::from(x) > f64::from(w) * 0.2
                    && f64::from(x) < f64::from(w) * 0.8;
                if head {
                    c = [224, 172, 140];
                } else if body {
                    c = [30, 32, 40];
                }
                if head || body {
                    truth[(y * w + x) as usize] = true;
                }
                v.extend_from_slice(&c);
            }
        }
        let img = Image {
            width: w,
            height: h,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        };
        let face = Face {
            x: cx - rx,
            y: cy - ry,
            w: rx * 2.0,
            h: ry * 2.0,
            confidence: 1.0,
            eye_l: (cx - rx * 0.4, cy - ry * 0.1),
            eye_r: (cx + rx * 0.4, cy - ry * 0.1),
            nose: (cx, cy + ry * 0.1),
            mouth_l: (cx - rx * 0.3, cy + ry * 0.4),
            mouth_r: (cx + rx * 0.3, cy + ry * 0.4),
            chin: None,
            jaw: vec![],
            yaw_deg: None,
            eye_open: None,
            mouth_open: None,
            engine: Engine::YuNet,
        };
        (img, truth, face)
    }

    #[test]
    fn a_plain_gradient_backdrop_is_cut_cleanly() {
        let (img, truth, face) = scene(400, 520);
        let m = color_cut(&img, 1.0, &face);
        assert!(m.reliable, "uniform {}", m.uniform);
        let Samples::U8(a) = &m.alpha.samples else {
            panic!()
        };
        let (mut inter, mut union) = (0usize, 0usize);
        for (p, &t) in a.iter().zip(&truth) {
            let on = *p >= 128;
            inter += usize::from(on && t);
            union += usize::from(on || t);
        }
        let iou = inter as f64 / union as f64;
        assert!(iou > 0.97, "IoU {iou}");
    }

    #[test]
    fn a_busy_backdrop_is_not_trusted() {
        let (mut img, _, face) = scene(300, 400);
        // Stripes over the whole backdrop, like a flag.
        if let Samples::U8(v) = &mut img.samples {
            for y in 0..400usize {
                for x in 0..300usize {
                    if (y / 12) % 2 == 0 && !(x > 90 && x < 210 && y > 60 && y < 220) {
                        let o = (y * 300 + x) * 3;
                        v[o..o + 3].copy_from_slice(&[200, 30, 40]);
                    }
                }
            }
        }
        let m = color_cut(&img, 1.0, &face);
        assert!(!m.reliable, "uniform {}", m.uniform);
    }

    #[test]
    fn the_top_of_the_head_is_found_in_a_band() {
        let (img, _, face) = scene(400, 520);
        let m = color_cut(&img, 1.0, &face);
        let top = m.top_in_band(face.x, face.x + face.w, 0.1).unwrap();
        let want = 520.0 * 0.38 - 520.0 * 0.2;
        assert!((top - want).abs() < 6.0, "{top} vs {want}");
    }

    #[test]
    fn blur_keeps_a_flat_field_flat() {
        let b = blur(&vec![1.0; 30 * 20], 30, 20, 2);
        assert!(b.iter().all(|&p| (p - 1.0).abs() < 1e-5));
    }
}
