//! Signature cleaner: the desktop pixel pipeline (paper estimate, ink separation, ruled-line removal, stroke thickening, fit to a
//! canvas), copied from panoply-id signature.rs. Encoding to an exact KB happens in the browser with MozJPEG or the PNG encoder.
#![allow(dead_code)]
use crate::image::{Image, Layout, Samples, resample_image};
use crate::error::{ImageError, Result};

const WORK_SIDE: u32 = 1600;
/// Padding around the ink, as a fraction of the larger ink side.
const PAD: f64 = 0.04;
/// Thinnest stroke wanted after the reduction, in pixels.
const MIN_STROKE_PX: f64 = 1.6;

/// What was asked for.
#[derive(Debug, Clone)]
pub struct Params {
    /// `keep`, `black` or `blue`.
    pub ink: String,
    /// Output size in pixels, when known.
    pub size: Option<(u32, u32)>,
    /// White JPG instead of transparent PNG.
    pub white: bool,
    /// Take ruled lines out.
    pub remove_lines: bool,
    /// Thicken thin strokes before a reduction.
    pub thicken: bool,
}

/// The cleaned signature before it is sized.
pub struct Cleaned {
    /// Ink amount, 0 to 255 per pixel, cropped with padding.
    pub alpha: Image,
    /// Where the alpha picture's top left pixel sits in the working picture (can be negative
    /// when the padding reaches past the photo).
    pub origin: (i64, i64),
    /// The ink colour.
    pub ink: [u8; 3],
    /// Median stroke thickness in pixels.
    pub stroke_px: f64,
    /// Ruled lines found (rows), whether or not removed.
    pub lines_found: usize,
    /// Whether the lines were removed.
    pub lines_removed: bool,
    /// Notes for the card.
    pub notes: Vec<String>,
}

fn luma(p: &[u8]) -> f32 {
    0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])
}

/// Paper brightness under the pen: a morphological closing (grow bright, then shrink back) whose
/// reach is a little more than the thickest stroke. A closing fills anything dark that is thinner
/// than its window, so strokes vanish into paper, while a large step (the edge of a shadow, a
/// fold) and a slow lighting gradient come back unchanged. That is why strokes can be told apart
/// from shadows without a shadow model.
fn paper_brightness(l: &[f32], w: usize, h: usize) -> (Vec<f32>, Vec<f32>) {
    let r = (w.max(h) / 150).max(4);
    // A touch of smoothing keeps sensor noise from being mistaken for fine ink.
    let sm = box_smooth(l, w, h, 1);
    let grown = max_filter(&sm, w, h, r);
    (min_filter(&grown, w, h, r), sm)
}

/// Separable minimum over a square of radius `r`.
fn min_filter(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = src[y * w + a..=y * w + b]
                .iter()
                .fold(f32::MAX, |m, &v| m.min(v));
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            out[y * w + x] = (a..=b).fold(f32::MAX, |m, yy| m.min(tmp[yy * w + x]));
        }
    }
    out
}

/// Separable maximum over a square of radius `r`.
fn max_filter(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = src[y * w + a..=y * w + b]
                .iter()
                .fold(0f32, |m, &v| m.max(v));
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            out[y * w + x] = (a..=b).fold(0f32, |m, yy| m.max(tmp[yy * w + x]));
        }
    }
    out
}

fn box_smooth(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let s: f32 = src[y * w + a..=y * w + b].iter().sum();
            tmp[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            let s: f32 = (a..=b).map(|yy| tmp[yy * w + x]).sum();
            out[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    out
}

/// Connected components of `mask` (8-neighbour): label per pixel (0 is none) and each one's
/// area and bounding box `(x0, y0, x1, y1)`.
#[allow(clippy::type_complexity)]
fn components(
    mask: &[bool],
    w: usize,
    h: usize,
) -> (Vec<u32>, Vec<(usize, (usize, usize, usize, usize))>) {
    let mut label = vec![0u32; w * h];
    let mut info = vec![(0usize, (0usize, 0usize, 0usize, 0usize))];
    let mut stack = Vec::new();
    for s in 0..w * h {
        if !mask[s] || label[s] != 0 {
            continue;
        }
        let id = info.len() as u32;
        label[s] = id;
        stack.push(s);
        let mut area = 0usize;
        let mut bb = (s % w, s / w, s % w, s / w);
        while let Some(i) = stack.pop() {
            area += 1;
            let (x, y) = (i % w, i / w);
            bb = (bb.0.min(x), bb.1.min(y), bb.2.max(x), bb.3.max(y));
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if mask[j] && label[j] == 0 {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        info.push((area, bb));
    }
    (label, info)
}

/// Clean the signature in `img` (RGB or RGBA, upright). A picture that already has transparency
/// (a drawn signature) uses its alpha as the ink.
pub fn clean(img: &Image, p: &Params) -> Result<Cleaned> {
    let (w, h) = (img.width as usize, img.height as usize);
    let Samples::U8(px) = &img.samples else {
        return Err(ImageError::Failed(
            "internal: the signature cleaner needs 8 bit pictures".into(),
        ));
    };
    let ch = img.layout.channels();
    let mut notes = Vec::new();
    let drawn = img.layout.has_alpha() && !img.is_opaque();
    // Ink amount per pixel, 0 to 1.
    let mut ink: Vec<f32>;
    let mut colours: Vec<(f32, [u8; 3])> = Vec::new();
    if drawn {
        notes.push("the picture already had transparency, so its alpha was used as the ink".into());
        ink = (0..w * h)
            .map(|i| f32::from(px[i * ch + 3]) / 255.0)
            .collect();
        for i in 0..w * h {
            if ink[i] > 0.9 {
                colours.push((0.0, [px[i * ch], px[i * ch + 1], px[i * ch + 2]]));
            }
        }
    } else {
        let l: Vec<f32> = (0..w * h).map(|i| luma(&px[i * ch..i * ch + 3])).collect();
        let (paper, smooth) = paper_brightness(&l, w, h);
        // Darkness against the local paper: 0 for paper, near 1 for black ink.
        let dark: Vec<f32> = smooth
            .iter()
            .zip(&paper)
            .map(|(&v, &b)| (1.0 - v / b.max(24.0)).clamp(0.0, 1.0))
            .collect();
        // The paper's own noise sets where ink starts.
        let mut sample: Vec<f32> = dark.iter().step_by(7).copied().collect();
        sample.sort_by(f32::total_cmp);
        let med = sample[sample.len() / 2];
        let mut dev: Vec<f32> = sample.iter().map(|&d| (d - med).abs()).collect();
        dev.sort_by(f32::total_cmp);
        let sigma = 1.4826 * dev[dev.len() / 2];
        let lo = (med + 3.0 * sigma + 0.03).clamp(0.05, 0.3);
        // Where ink is solid: the dark end of what the pen did. The ramp from `lo` to `solid` is
        // linear in coverage, so half ink (alpha 0.5) sits on the edge of the stroke.
        let mut top: Vec<f32> = dark
            .iter()
            .step_by(3)
            .copied()
            .filter(|&d| d > lo)
            .collect();
        top.sort_by(f32::total_cmp);
        let solid = top
            .get(top.len() * 19 / 20)
            .copied()
            .unwrap_or(0.5)
            .clamp(lo + 0.12, 0.85);
        let hi = solid;
        ink = dark
            .iter()
            .map(|&d| ((d - lo) / (hi - lo)).clamp(0.0, 1.0))
            .collect();
        // Colour of the pen, from the picture itself with the lighting divided out (ink in a
        // shadow is darker in the photo than the pen is), over the solid ink only, and taking the
        // darker half so the lighter edge pixels do not wash it out.
        let mut strong: Vec<(f32, [u8; 3])> = (0..w * h)
            .filter(|&i| ink[i] > 0.95)
            .map(|i| {
                let k = 235.0 / paper[i].max(24.0);
                let c = [
                    (f32::from(px[i * ch]) * k).min(255.0) as u8,
                    (f32::from(px[i * ch + 1]) * k).min(255.0) as u8,
                    (f32::from(px[i * ch + 2]) * k).min(255.0) as u8,
                ];
                (luma(&c), c)
            })
            .collect();
        strong.sort_by(|a, b| a.0.total_cmp(&b.0));
        strong.truncate((strong.len() / 2).max(1));
        colours = strong;
    }
    let ink_px = ink.iter().filter(|&&v| v > 0.5).count();
    if ink_px < (w * h / 20_000).max(60) {
        return Err(ImageError::Failed(
            "No ink was found. Photograph the signature in good light, dark pen on white paper, with the page filling the frame.".into(),
        ));
    }
    // Ruled lines are lighter than the pen on most paper: find them before the cleanup, which
    // drops anything faint that is not part of a stroke.
    let (faint_lines, _) = find_lines(&ink, w, h, 0.15);
    // Cleanup: components.
    let mask: Vec<bool> = ink.iter().map(|&v| v > 0.5).collect();
    let (label, info) = components(&mask, w, h);
    let biggest = info.iter().skip(1).map(|c| c.0).max().unwrap_or(0);
    let min_area = ((w * h) / 400_000).max(6);
    // The main cluster: components of a fair size.
    let mut bb = (usize::MAX, usize::MAX, 0usize, 0usize);
    for (a, b) in info.iter().skip(1) {
        if *a * 20 >= biggest {
            bb = (bb.0.min(b.0), bb.1.min(b.1), bb.2.max(b.2), bb.3.max(b.3));
        }
    }
    let (bw, bh) = ((bb.2 - bb.0 + 1) as f64, (bb.3 - bb.1 + 1) as f64);
    let reach = (bw.max(bh) * 0.35) as i64;
    let keep: Vec<bool> = info
        .iter()
        .enumerate()
        .map(|(i, (a, b))| {
            if i == 0 {
                return false;
            }
            if *a < min_area {
                return false;
            }
            // Far from the main cluster and not large: a page edge or a shadow, not signature.
            let far = (b.0 as i64) > bb.2 as i64 + reach
                || (b.2 as i64) < bb.0 as i64 - reach
                || (b.1 as i64) > bb.3 as i64 + reach
                || (b.3 as i64) < bb.1 as i64 - reach;
            !(far && *a * 4 < biggest)
        })
        .collect();
    let mut dropped = 0usize;
    for i in 0..w * h {
        if label[i] != 0 && !keep[label[i] as usize] {
            ink[i] = 0.0;
            dropped += 1;
        } else if label[i] == 0 && ink[i] > 0.0 {
            // Soft fringe next to a removed component: handled below by the distance rule.
        }
    }
    // Fringe pixels (alpha under 0.5) whose neighbourhood had no kept ink are cleared.
    let kept_mask: Vec<bool> = (0..w * h)
        .map(|i| label[i] != 0 && keep[label[i] as usize])
        .collect();
    let near = max_bool(&kept_mask, w, h, 2);
    for i in 0..w * h {
        if !near[i] {
            ink[i] = 0.0;
        }
    }
    if dropped > 0 {
        notes.push("specks and marks far from the signature were left out".into());
    }
    // Lines as dark as the pen stay, unless asked to go.
    let (lines, thickness_lines) = find_lines(&ink, w, h, 0.5);
    let mut removed = false;
    if !lines.is_empty() {
        if p.remove_lines {
            remove_lines(&mut ink, w, h, &lines, thickness_lines);
            removed = true;
            notes.push(format!(
                "{} ruled line{} as dark as the pen removed where the line was only a line; strokes that cross it were kept",
                lines.len(),
                if lines.len() == 1 { "" } else { "s" }
            ));
        } else {
            notes.push(format!(
                "{} ruled line{} as dark as the pen found in the photo and left in. Run again with remove_lines=true to take {} out",
                lines.len(),
                if lines.len() == 1 { "" } else { "s" },
                if lines.len() == 1 { "it" } else { "them" }
            ));
        }
    }
    if faint_lines.len() > lines.len() {
        notes.push(format!(
            "{} ruled line{} on the paper {} lighter than the pen, so {} not in the result",
            faint_lines.len() - lines.len(),
            if faint_lines.len() - lines.len() == 1 {
                ""
            } else {
                "s"
            },
            if faint_lines.len() - lines.len() == 1 {
                "was"
            } else {
                "were"
            },
            if faint_lines.len() - lines.len() == 1 {
                "it is"
            } else {
                "they are"
            }
        ));
    }
    // Crop with padding.
    let mut b = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if ink[y * w + x] > 0.35 {
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
    }
    if b.0 == usize::MAX {
        return Err(ImageError::Failed(
            "No ink was left after the cleanup. Try a photo with more contrast.".into(),
        ));
    }
    let (iw, ih) = (b.2 - b.0 + 1, b.3 - b.1 + 1);
    let pad = (iw.max(ih) as f64 * PAD).ceil() as usize;
    let (x0, y0) = (
        b.0.saturating_sub(pad) as i64,
        b.1.saturating_sub(pad) as i64,
    );
    // Padding may reach past the photo: the canvas is bigger than the crop of the photo then.
    let (cw, chh) = (iw + 2 * pad, ih + 2 * pad);
    let (ox, oy) = (b.0 as i64 - pad as i64 - x0, b.1 as i64 - pad as i64 - y0);
    let _ = (ox, oy);
    let mut out = vec![0u8; cw * chh];
    for y in 0..chh {
        for x in 0..cw {
            let (sx, sy) = (
                b.0 as i64 - pad as i64 + x as i64,
                b.1 as i64 - pad as i64 + y as i64,
            );
            if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                out[y * cw + x] = (ink[sy as usize * w + sx as usize] * 255.0 + 0.5) as u8;
            }
        }
    }
    // Stroke thickness: area over half the perimeter of the ink.
    let stroke = stroke_thickness(&out, cw, chh);
    // Colour.
    let pen = match p.ink.as_str() {
        "black" => [15, 15, 15],
        "blue" => [20, 45, 150],
        _ => {
            if colours.is_empty() {
                [15, 15, 15]
            } else {
                let n = colours.len() as u32;
                let s = colours.iter().fold([0u32; 3], |mut a, (_, c)| {
                    for k in 0..3 {
                        a[k] += u32::from(c[k]);
                    }
                    a
                });
                [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
            }
        }
    };
    Ok(Cleaned {
        origin: (b.0 as i64 - pad as i64, b.1 as i64 - pad as i64),
        alpha: Image {
            width: cw as u32,
            height: chh as u32,
            layout: Layout::Gray,
            samples: Samples::U8(out),
            icc: None,
        },
        ink: pen,
        stroke_px: stroke,
        lines_found: faint_lines.len().max(lines.len()),
        lines_removed: removed,
        notes,
    })
}

fn max_bool(src: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let f: Vec<f32> = src.iter().map(|&b| f32::from(u8::from(b))).collect();
    max_filter(&f, w, h, r)
        .into_iter()
        .map(|v| v > 0.5)
        .collect()
}

fn stroke_thickness(alpha: &[u8], w: usize, h: usize) -> f64 {
    let (mut area, mut perim) = (0u64, 0u64);
    for y in 0..h {
        for x in 0..w {
            if alpha[y * w + x] >= 128 {
                area += 1;
                let edge = x == 0
                    || y == 0
                    || x + 1 == w
                    || y + 1 == h
                    || alpha[y * w + x - 1] < 128
                    || alpha[y * w + x + 1] < 128
                    || alpha[(y - 1) * w + x] < 128
                    || alpha[(y + 1) * w + x] < 128;
                if edge {
                    perim += 1;
                }
            }
        }
    }
    if perim == 0 {
        1.0
    } else {
        (2.0 * area as f64 / perim as f64).max(1.0)
    }
}

/// Rows that are a long horizontal run of ink or faint ink (ruled paper is lighter than the pen):
/// `(first row, last row)` of each line, and the median line thickness. A run is "long" when it
/// covers more than 55 percent of the width the ink spans.
fn find_lines(ink: &[f32], w: usize, h: usize, faint: f32) -> (Vec<(usize, usize)>, usize) {
    let (mut x0, mut x1) = (w, 0usize);
    for y in 0..h {
        for x in 0..w {
            if ink[y * w + x] > faint {
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
    }
    if x1 <= x0 {
        return (vec![], 0);
    }
    let span = x1 - x0 + 1;
    let mut rows: Vec<bool> = vec![false; h];
    for y in 0..h {
        let mut best = 0usize;
        let mut run = 0usize;
        for x in 0..w {
            if ink[y * w + x] > faint {
                run += 1;
                best = best.max(run);
            } else if run > 0 {
                // A one pixel gap does not end a ruled line.
                if x + 1 < w && ink[y * w + x + 1] > faint {
                    run += 1;
                } else {
                    run = 0;
                }
            }
        }
        rows[y] = best as f64 > span as f64 * 0.55;
    }
    let mut lines = Vec::new();
    let mut y = 0;
    while y < h {
        if rows[y] {
            let s = y;
            while y < h && rows[y] {
                y += 1;
            }
            lines.push((s, y - 1));
        } else {
            y += 1;
        }
    }
    // A signature's own underline is one line; ruled paper makes several, or one that is thin.
    // Thickness decides: a line thinner than the thinnest cluster of strokes is paper.
    let thick = {
        let mut t: Vec<usize> = lines.iter().map(|l| l.1 - l.0 + 1).collect();
        t.sort_unstable();
        t.get(t.len() / 2).copied().unwrap_or(0)
    };
    (lines, thick)
}

fn remove_lines(ink: &mut [f32], w: usize, h: usize, lines: &[(usize, usize)], _thick: usize) {
    let pad = 2usize;
    for &(y0, y1) in lines {
        for x in 0..w {
            // Ink above or below the line at this column means a stroke crosses it: keep it.
            let above = (1..=pad + 1).any(|d| y0 >= d && ink[(y0 - d) * w + x] > 0.5);
            let below = (1..=pad + 1).any(|d| y1 + d < h && ink[(y1 + d) * w + x] > 0.5);
            if above || below {
                continue;
            }
            for y in y0.saturating_sub(1)..=(y1 + 1).min(h - 1) {
                ink[y * w + x] = 0.0;
            }
        }
    }
}

/// Thicken ink by growing the alpha with a disc of radius `r`, then softening it.
fn thicken(alpha: &Image, r: usize) -> Image {
    let (w, h) = (alpha.width as usize, alpha.height as usize);
    let Samples::U8(a) = &alpha.samples else {
        return alpha.clone();
    };
    let f: Vec<f32> = a.iter().map(|&v| f32::from(v) / 255.0).collect();
    let g = max_filter(&f, w, h, r);
    let s = box_smooth(&g, w, h, 1);
    Image {
        width: alpha.width,
        height: alpha.height,
        layout: Layout::Gray,
        samples: Samples::U8(
            s.iter()
                .map(|&v| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8)
                .collect(),
        ),
        icc: None,
    }
}

/// The finished picture.
pub struct Made {
    /// RGBA with the ink colour and the alpha (or RGB flattened on white).
    pub image: Image,
    /// Notes for the card.
    pub notes: Vec<String>,
}

/// Fit the cleaned signature into `size` (or keep its own size) and make the final picture.
pub fn finish(c: &Cleaned, p: &Params) -> Result<Made> {
    let mut notes = c.notes.clone();
    let (cw, chh) = (c.alpha.width, c.alpha.height);
    let (tw, th) = p.size.unwrap_or((cw, chh));
    let s = (f64::from(tw) / f64::from(cw)).min(f64::from(th) / f64::from(chh));
    let (nw, nh) = (
        ((f64::from(cw) * s).round() as u32).clamp(1, tw),
        ((f64::from(chh) * s).round() as u32).clamp(1, th),
    );
    let mut alpha = c.alpha.clone();
    if p.thicken && s < 1.0 {
        let after = c.stroke_px * s;
        if after < MIN_STROKE_PX {
            let r = (((MIN_STROKE_PX / s - c.stroke_px) / 2.0).ceil() as usize).clamp(1, 12);
            alpha = thicken(&alpha, r);
            notes.push(format!(
                "strokes would have been {after:.1} px thick at this size, so they were thickened first to keep them from fading"
            ));
        }
    }
    let scaled = if (nw, nh) == (cw, chh) {
        alpha
    } else {
        resample_image(&alpha, nw, nh)?
    };
    let Samples::U8(a) = &scaled.samples else {
        return Err(ImageError::Failed("internal: bad alpha".into()));
    };
    // Place on the canvas, centred.
    let (ox, oy) = ((tw - nw) / 2, (th - nh) / 2);
    let mut canvas = vec![0u8; tw as usize * th as usize];
    for y in 0..nh as usize {
        let d = (y + oy as usize) * tw as usize + ox as usize;
        canvas[d..d + nw as usize].copy_from_slice(&a[y * nw as usize..(y + 1) * nw as usize]);
    }
    let image = if p.white {
        let mut v = Vec::with_capacity(canvas.len() * 3);
        for &al in &canvas {
            let al = u32::from(al);
            for k in 0..3 {
                v.push(((u32::from(c.ink[k]) * al + 255 * (255 - al) + 127) / 255) as u8);
            }
        }
        Image {
            width: tw,
            height: th,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        }
    } else {
        let mut v = Vec::with_capacity(canvas.len() * 4);
        for &al in &canvas {
            v.extend_from_slice(&[c.ink[0], c.ink[1], c.ink[2], al]);
        }
        Image {
            width: tw,
            height: th,
            layout: Layout::Rgba,
            samples: Samples::U8(v),
            icc: None,
        }
    };
    Ok(Made { image, notes })
}

