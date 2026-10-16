//! `image.remove_watermark`: take a watermark, logo or date stamp off the user's own pictures
//! (docs/specs/id-and-image.md 3.15).
//!
//! **Scope.** This is for pictures the user owns or may edit: their own photos with a camera date
//! stamp, their own exports with a logo they added, their own AI images with a visible badge. The
//! module refuses to run until `own_content` is set, takes no URLs, ships no list of stock-site
//! marks and names no agency. Invisible watermarks and signed provenance (C2PA, SynthID) are not
//! looked for and not touched.
//!
//! **Detection** is classical, no model: overlays are thin bright or dark structures on a local
//! background, so a white and a black top-hat find the strokes; strokes close together merge into
//! text lines and logo blobs; each blob is scored by how text-like it is (several letter-sized
//! parts of similar height), how consistent the stroke color is, and how close it sits to an edge
//! or corner; a second pass looks for saturated warm date-stamp digits in the corners. The result
//! is a mask of the strokes themselves, not a box, so the fill touches as little as possible.
//!
//! **Fill** is the MI-GAN model (as in `image.erase`), or a blur of the surroundings for flat
//! areas. Pixels outside the mask are copied and digest-checked, exactly as in erase.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use panoply_image::error::{ImageError, Result};
use panoply_image::ops::common::{Animation, display_name};
use panoply_image::ops::human_bytes;
use panoply_image::ops::options::Opts;

use crate::VisionIo;
use crate::inpaint::{Inpainter, clusters};
use crate::px::{
    Kernel, Rect, box_mean, dilate, gaussian_blur, parse_rects, reduce_box, reduce_map, resize_u8,
};
use crate::runtime;
use crate::work::{
    MAX_WORK_PIXELS, WriteSpec, file_name, image_from, load_source, parse_format, write_result,
};

/// What kind of mark a region looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Lines of letters.
    Text,
    /// Warm colored digits in a corner (a camera date stamp).
    Stamp,
    /// A compact mark without letter structure.
    Logo,
    /// A rectangle the user gave.
    Manual,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Stamp => "date stamp",
            Kind::Logo => "logo",
            Kind::Manual => "marked area",
        }
    }
}

/// One detected region, in full resolution pixels.
#[derive(Debug, Clone)]
pub struct Region {
    /// Where it is.
    pub rect: Rect,
    /// How sure the detector is, 0 to 1.
    pub score: f32,
    /// What it looks like.
    pub kind: Kind,
}

/// Where to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    /// The whole picture, with a higher bar away from the edges.
    Anywhere,
    /// Only within 18% of an edge (corner stamps, bottom credits).
    Edges,
}

/// What detection found.
pub struct Detection {
    /// The regions, best first.
    pub regions: Vec<Region>,
    /// One byte a pixel at full resolution: 1 on the strokes of the marks (before expansion).
    pub mask: Vec<u8>,
    /// How many candidates were left alone because they sit on a face (or a mark was cut short
    /// where it crosses one).
    pub spared: usize,
}

/// The longest side detection works at.
const ANALYSIS_LONG: usize = 1000;

fn sliding(src: &[f32], out: &mut [f32], r: usize, max: bool) {
    let n = src.len();
    let mut dq: VecDeque<usize> = VecDeque::new();
    let mut next = 0usize;
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        let hi = (i + r).min(n - 1);
        while next <= hi {
            while let Some(&b) = dq.back() {
                if (max && src[b] <= src[next]) || (!max && src[b] >= src[next]) {
                    dq.pop_back();
                } else {
                    break;
                }
            }
            dq.push_back(next);
            next += 1;
        }
        let lo = i.saturating_sub(r);
        while let Some(&f) = dq.front() {
            if f < lo {
                dq.pop_front();
            } else {
                break;
            }
        }
        *slot = src[*dq.front().expect("window is never empty")];
    }
}

/// Grayscale erosion (`max = false`) or dilation (`max = true`) with a square of side `2r + 1`.
fn extremum(src: &[f32], w: usize, h: usize, r: usize, max: bool) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        sliding(
            &src[y * w..(y + 1) * w],
            &mut tmp[y * w..(y + 1) * w],
            r,
            max,
        );
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let o = &mut out[y * w..(y + 1) * w];
        o.copy_from_slice(&tmp[y * w..(y + 1) * w]);
        for yy in y.saturating_sub(r)..=(y + r).min(h - 1) {
            let row = &tmp[yy * w..(yy + 1) * w];
            if max {
                for (a, &b) in o.iter_mut().zip(row) {
                    if b > *a {
                        *a = b;
                    }
                }
            } else {
                for (a, &b) in o.iter_mut().zip(row) {
                    if b < *a {
                        *a = b;
                    }
                }
            }
        }
    }
    out
}

/// White and black top-hats: thin bright structures and thin dark structures (0 to 1).
fn top_hats(y: &[f32], w: usize, h: usize, r: usize) -> (Vec<f32>, Vec<f32>) {
    let eroded = extremum(y, w, h, r, false);
    let opened = extremum(&eroded, w, h, r, true);
    drop(eroded);
    let dilated = extremum(y, w, h, r, true);
    let closed = extremum(&dilated, w, h, r, false);
    drop(dilated);
    let white = y
        .iter()
        .zip(&opened)
        .map(|(a, b)| (a - b).max(0.0))
        .collect();
    let black = y
        .iter()
        .zip(&closed)
        .map(|(a, b)| (b - a).max(0.0))
        .collect();
    (white, black)
}

/// A labelled connected component.
struct Comp {
    rect: Rect,
    pixels: usize,
}

/// 8-connected components of a binary map.
fn components(map: &[u8], w: usize, h: usize) -> Vec<Comp> {
    components_labeled(map, w, h).0
}

/// 8-connected components of a binary map, with a label per pixel (0 for none, else the
/// component's index plus one).
fn components_labeled(map: &[u8], w: usize, h: usize) -> (Vec<Comp>, Vec<u32>) {
    let mut seen = vec![false; w * h];
    let mut labels = vec![0u32; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if map[start] == 0 || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        let mut n = 0usize;
        let label = out.len() as u32 + 1;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            labels[i] = label;
            n += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = ny * w + nx;
                    if map[j] != 0 && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        out.push(Comp {
            rect: Rect {
                x: x0,
                y: y0,
                w: x1 - x0 + 1,
                h: y1 - y0 + 1,
            },
            pixels: n,
        });
    }
    (out, labels)
}

/// Grows a binary map by `rx` pixels horizontally and `ry` vertically (a box).
fn grow_box(map: &[u8], w: usize, h: usize, rx: usize, ry: usize) -> Vec<u8> {
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let row = &map[y * w..(y + 1) * w];
        // Distance to the last set pixel on the left.
        let mut last: Option<usize> = None;
        let mut next_set = vec![usize::MAX; w];
        let mut nxt: Option<usize> = None;
        for x in (0..w).rev() {
            if row[x] != 0 {
                nxt = Some(x);
            }
            if let Some(n) = nxt {
                next_set[x] = n;
            }
        }
        for x in 0..w {
            if row[x] != 0 {
                last = Some(x);
            }
            let near_left = last.is_some_and(|l| x - l <= rx);
            let near_right = next_set[x] != usize::MAX && next_set[x] - x <= rx;
            tmp[y * w + x] = u8::from(near_left || near_right);
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for yy in y.saturating_sub(ry)..=(y + ry).min(h - 1) {
            for x in 0..w {
                if tmp[yy * w + x] != 0 {
                    out[y * w + x] = 1;
                }
            }
        }
    }
    out
}

/// How close a rectangle is to the nearest edge, as a fraction of the picture size (0 at an
/// edge).
fn edge_distance(r: &Rect, w: usize, h: usize) -> f32 {
    let dx = r.x.min(w - (r.x + r.w)) as f32 / w as f32;
    let dy = r.y.min(h - (r.y + r.h)) as f32 / h as f32;
    dx.min(dy)
}

/// A candidate blob of strokes, in analysis coordinates.
struct Blob {
    rect: Rect,
    score: f32,
    kind: Kind,
    /// The strokes inside the rectangle (row major, `rect.w * rect.h`).
    strokes: Vec<u8>,
}

/// The most the three channels may disagree about a mark's opacity (median over its strokes).
const OVERLAY_SPREAD_MAX: f32 = 0.15;

/// How far apart the three colour channels' opacity estimates are, as a median over the strokes
/// of the blob in `r`, or `None` when the picture under the mark has no colour contrast with it
/// to judge by. A stroke pixel `v` over a background `b` that was blended toward white (or black)
/// by an opacity `a` has `(v - b) / (255 - b) = a` in every channel; `b` is the average of the
/// pixels nearby that are not marked.
fn overlay_spread(
    planes: &[Vec<f32>; 3],
    m: &[u8],
    weak: &[u8],
    aw: usize,
    ah: usize,
    r: Rect,
    white: bool,
) -> Option<f32> {
    let reach = (r.h / 5).clamp(3, 10);
    let target = if white { 1.0f32 } else { 0.0 };
    let mut spreads: Vec<f32> = Vec::new();
    let step = ((r.w * r.h) / 3000).max(1);
    let mut seen = 0usize;
    for yy in r.y..r.y + r.h {
        for xx in r.x..r.x + r.w {
            let i = yy * aw + xx;
            if m[i] == 0 {
                continue;
            }
            seen += 1;
            if !seen.is_multiple_of(step) {
                continue;
            }
            let x0 = xx.saturating_sub(reach);
            let y0 = yy.saturating_sub(reach);
            let x1 = (xx + reach + 1).min(aw);
            let y1 = (yy + reach + 1).min(ah);
            let mut bg = [0f32; 3];
            let mut n = 0f32;
            for y2 in y0..y1 {
                for x2 in x0..x1 {
                    let j = y2 * aw + x2;
                    if weak[j] == 0 && m[j] == 0 {
                        for c in 0..3 {
                            bg[c] += planes[c][j];
                        }
                        n += 1.0;
                    }
                }
            }
            if n < 6.0 {
                continue;
            }
            let mut a = [0f32; 3];
            let mut ok = 0;
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for c in 0..3 {
                let b = bg[c] / n;
                let room = (target - b).abs();
                if room > 0.18 {
                    let ac = ((planes[c][i] - b) / (target - b)).clamp(-0.5, 1.5);
                    a[c] = ac;
                    lo = lo.min(ac);
                    hi = hi.max(ac);
                    ok += 1;
                }
            }
            if ok >= 2 {
                spreads.push(hi - lo);
            }
        }
    }
    if spreads.len() < 20 {
        return None;
    }
    spreads.sort_by(f32::total_cmp);
    Some(spreads[spreads.len() / 2])
}

/// Mean of `v` over the pixels where `m` is set inside `r`, with its standard deviation.
fn masked_stats(v: &[f32], m: &[u8], aw: usize, r: Rect) -> (f32, f32, usize) {
    let (mut s, mut s2, mut n) = (0f32, 0f32, 0usize);
    for yy in r.y..r.y + r.h {
        for xx in r.x..r.x + r.w {
            if m[yy * aw + xx] != 0 {
                let x = v[yy * aw + xx];
                s += x;
                s2 += x * x;
                n += 1;
            }
        }
    }
    let mean = s / n.max(1) as f32;
    (
        mean,
        (s2 / n.max(1) as f32 - mean * mean).max(0.0).sqrt(),
        n,
    )
}

/// Blobs of strokes of one polarity (`white` strokes are brighter than their surroundings, `black`
/// darker), scored for how much they look like an overlay.
fn blobs_for(
    y: &[f32],
    planes: &[Vec<f32>; 3],
    aw: usize,
    ah: usize,
    white: bool,
    search: Search,
) -> Vec<Blob> {
    // Three widths of stroke: fine print, ordinary text and bold text. A structure element is
    // wider than the strokes it keeps, so the narrow one cannot fuse a letter with a wide object
    // that happens to touch it.
    let mut out = Vec::new();
    for frac in [0.004f32, 0.011, 0.028] {
        out.extend(blobs_at_scale(y, planes, aw, ah, white, search, frac));
    }
    out
}

/// Blobs of strokes no wider than `frac` of the picture's long side.
fn blobs_at_scale(
    y: &[f32],
    planes: &[Vec<f32>; 3],
    aw: usize,
    ah: usize,
    white: bool,
    search: Search,
    frac: f32,
) -> Vec<Blob> {
    let long = aw.max(ah);
    // Strokes: top-hat thresholded against the local texture level so a flat sky and a furry
    // dog are each judged by their own standard. `m` holds the strong strokes that locate
    // letters, `weak` the fainter ones that complete the shape of a mark once it is located.
    let mut m = vec![0u8; aw * ah];
    let mut weak = vec![0u8; aw * ah];
    let mut strength = vec![0f32; aw * ah];
    {
        let r = ((frac * long as f32).round() as usize).max(2);
        let (tw, tb) = top_hats(y, aw, ah, r);
        let t = if white { tw } else { tb };
        // The texture level around each pixel. Large values are clipped first, so a mark's own
        // strokes do not raise the bar that they must clear.
        let clipped: Vec<f32> = t.iter().map(|&v| v.min(0.08)).collect();
        let mut local = vec![0f32; aw * ah];
        box_mean(&clipped, aw, ah, (r * 4).max(8), &mut local);
        for i in 0..aw * ah {
            // How much room the background leaves for this polarity of mark: a white overlay on
            // a bright sky can only add a little, and a black one on a dark suit only take a
            // little, however opaque it is. The bar a stroke must clear shrinks with that room
            // (never under a noise floor), so a pale mark on a pale ground is not lost, and the
            // alpha it implies (stroke over room) is what the mark's consistency is judged on.
            let bg = if white { y[i] - t[i] } else { y[i] + t[i] };
            let room = if white { 1.0 - bg } else { bg }.max(0.1);
            let strong = (0.3 * room).clamp(0.03, 0.09);
            let faint = (0.15 * room).clamp(0.02, 0.045);
            if t[i] > (2.2 * local[i]).max(strong) {
                m[i] = 1;
                strength[i] = (t[i] / room).min(1.0);
            }
            if t[i] > (1.5 * local[i]).max(faint) {
                weak[i] = 1;
            }
        }
    }
    // Letters of one line, across the spaces between words.
    let gap = ((long as f32 * 0.03).round() as usize).max(3);
    // Keep only letter-sized pieces: a glyph is small, roughly as wide as it is tall, and fills
    // part of its box. Large connected masses (fur, foliage, rock) and specks are dropped.
    let (comps, labels) = components_labeled(&m, aw, ah);
    let min_h = ((ah as f32 * 0.012) as usize).max(5);
    let max_h = (ah as f32 * 0.12) as usize;
    let keep: Vec<bool> = comps
        .iter()
        .map(|c| {
            let r = c.rect;
            let box_fill = c.pixels as f32 / (r.w * r.h) as f32;
            c.pixels >= 8
                && r.h >= min_h
                && r.h <= max_h
                && r.w <= r.h * 3
                && r.w * 8 >= r.h
                && (0.12..=0.95).contains(&box_fill)
        })
        .collect();
    for (v, &l) in m.iter_mut().zip(&labels) {
        if l == 0 || !keep[l as usize - 1] {
            *v = 0;
        }
    }
    let letters: Vec<Comp> = comps
        .into_iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c)
        .collect();
    let merged = grow_box(&m, aw, ah, gap, 1);
    let mut out = Vec::new();
    for c in components(&merged, aw, ah) {
        let r = c.rect;
        if r.h < (ah as f32 * 0.012) as usize + 3 || r.h as f32 > ah as f32 * 0.22 {
            continue;
        }
        if (r.w as f32) < r.h as f32 * 1.3 || r.w as f32 > aw as f32 * 0.8 {
            continue;
        }
        let inside: Vec<&Comp> = letters
            .iter()
            .filter(|l| {
                l.rect.x >= r.x
                    && l.rect.y >= r.y
                    && l.rect.x + l.rect.w <= r.x + r.w
                    && l.rect.y + l.rect.h <= r.y + r.h
                    && l.pixels >= 4
                    && l.rect.h >= 3
            })
            .collect();
        // A mark has a margin: strokes running into the picture's edge are texture or part of
        // the composition, not an overlay placed by someone. Measured on the letters
        // themselves, not on the grown blob.
        if let Some(tight) = inside.iter().map(|l| l.rect).reduce(|a, b| a.union(&b)) {
            let margin = tight
                .x
                .min(tight.y)
                .min(aw - (tight.x + tight.w))
                .min(ah - (tight.y + tight.h));
            if margin <= 1 {
                continue;
            }
        }
        let stroke_px: usize = inside.iter().map(|l| l.pixels).sum();
        let fill = stroke_px as f32 / (r.w * r.h) as f32;
        if !(0.06..=0.7).contains(&fill) {
            continue;
        }
        // Letters: parts of similar height whose bottoms (or tops) sit on a common line.
        let mut heights: Vec<usize> = inside.iter().map(|l| l.rect.h).collect();
        heights.sort_unstable();
        let Some(&med_h) = heights.get(heights.len() / 2) else {
            continue;
        };
        let like: Vec<&&Comp> = inside
            .iter()
            .filter(|l| {
                (l.rect.h as f32) >= med_h as f32 * 0.6 && (l.rect.h as f32) <= med_h as f32 * 1.5
            })
            .collect();
        let letter_count = like.len();
        if letter_count < if frac < 0.006 { 5 } else { 4 } {
            continue;
        }
        let line_frac = |pick: &dyn Fn(&Comp) -> usize| -> f32 {
            let mut v: Vec<usize> = like.iter().map(|l| pick(l)).collect();
            v.sort_unstable();
            let med = v[v.len() / 2] as f32;
            v.iter()
                .filter(|&&x| (x as f32 - med).abs() <= 0.22 * med_h as f32)
                .count() as f32
                / v.len() as f32
        };
        let aligned = line_frac(&|l| l.rect.y + l.rect.h).max(line_frac(&|l| l.rect.y));
        if aligned < 0.55 {
            continue;
        }
        // Strokes of an overlay have a steady strength; texture varies a lot.
        let (smean, sstd, _) = masked_stats(&strength, &m, aw, r);
        let cv = sstd / smean.max(1e-4);
        let steady = (1.0 - (cv - 0.25) / 0.6).clamp(0.0, 1.0);
        // Overlay strokes all lean one way against the background around them.
        let (lmean, _, _) = masked_stats(y, &m, aw, r);
        let ring = r.grown(gap.max(3) + 2, aw, ah);
        let (mut rs, mut rn) = (0f32, 0f32);
        for yy in ring.y..ring.y + ring.h {
            for xx in ring.x..ring.x + ring.w {
                let in_blob = xx >= r.x && xx < r.x + r.w && yy >= r.y && yy < r.y + r.h;
                if !in_blob {
                    rs += y[yy * aw + xx];
                    rn += 1.0;
                }
            }
        }
        let lean = (lmean - rs / rn.max(1.0)).abs();
        let lean_term = (lean / 0.10).clamp(0.0, 1.0);
        let ed = edge_distance(&r, aw, ah);
        let near_edge = ed <= 0.18;
        let edge_term = (1.0 - ed / 0.18).clamp(0.0, 1.0);
        let letters_term = (letter_count as f32 / 8.0).clamp(0.0, 1.0);
        let score = 0.28 * letters_term
            + 0.22 * aligned
            + 0.22 * steady
            + 0.14 * lean_term
            + 0.14 * edge_term;
        // Fine strokes are the easiest to confuse with texture, so they must score higher.
        let fine = if frac < 0.006 { 0.08 } else { 0.0 };
        let bar = fine
            + match (search, near_edge) {
                (Search::Edges, false) => continue,
                (_, true) => 0.58,
                (Search::Anywhere, false) => 0.72,
            };
        if score < bar {
            continue;
        }
        // A real overlay blends one colour into whatever is under it, so every colour channel
        // shows the same opacity. Printed letters, stars on a flag, a label on a bottle and the
        // texture of a wall each have a colour of their own and the channels disagree.
        let spread = overlay_spread(planes, &m, &weak, aw, ah, r, white);
        if spread.is_some_and(|s| s > OVERLAY_SPREAD_MAX) {
            continue;
        }
        // The strokes of the mark: the strong ones, plus the faint ones right next to them.
        let mut core = vec![0u8; r.w * r.h];
        for yy in 0..r.h {
            for xx in 0..r.w {
                core[yy * r.w + xx] = m[(r.y + yy) * aw + r.x + xx];
            }
        }
        let near = dilate(&core, r.w, r.h, 2);
        let mut sub = core;
        for yy in 0..r.h {
            for xx in 0..r.w {
                if near[yy * r.w + xx] != 0 && weak[(r.y + yy) * aw + r.x + xx] != 0 {
                    sub[yy * r.w + xx] = 1;
                }
            }
        }
        out.push(Blob {
            rect: r,
            score,
            kind: Kind::Text,
            strokes: sub,
        });
    }
    out
}

/// Finds watermark-like marks in an 8 bit sRGB picture.
pub fn detect(rgb: &[u8], w: usize, h: usize, search: Search) -> Detection {
    detect_with(rgb, w, h, search, Vec::new)
}

/// The head around a detected face box: the box grown for hair above, and a little on the sides
/// and below. Nothing in it is ever taken for a mark (UP-38).
pub fn head_box(face: &Rect, w: usize, h: usize) -> Rect {
    let up = face.h * 6 / 10;
    let side = face.w / 4;
    let down = face.h / 5;
    let x0 = face.x.saturating_sub(side);
    let y0 = face.y.saturating_sub(up);
    let x1 = (face.x + face.w + side).min(w);
    let y1 = (face.y + face.h + down).min(h);
    Rect {
        x: x0,
        y: y0,
        w: x1.saturating_sub(x0),
        h: y1.saturating_sub(y0),
    }
}

/// How much of `r` lies inside `o`, 0 to 1.
fn covered_by(r: &Rect, o: &Rect) -> f32 {
    let x0 = r.x.max(o.x);
    let y0 = r.y.max(o.y);
    let x1 = (r.x + r.w).min(o.x + o.w);
    let y1 = (r.y + r.h).min(o.y + o.h);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    ((x1 - x0) * (y1 - y0)) as f32 / (r.w * r.h).max(1) as f32
}

/// Like [`detect`], with the faces of the picture (full resolution boxes, as the face engines
/// give them). `faces` is only called when there is a candidate to check, so a picture with
/// nothing in it never pays for a face search. A candidate mostly on a head is not a mark, and where a mark crosses a head the
/// head is cut out of the mask, so a face is never filled in. The caller reads
/// [`Detection::spared`] to say so.
pub fn detect_with(
    rgb: &[u8],
    w: usize,
    h: usize,
    search: Search,
    faces: impl FnOnce() -> Vec<Rect>,
) -> Detection {
    // Work on a reduced copy.
    let f = w.max(h).div_ceil(ANALYSIS_LONG).max(1);
    let lum: Vec<f32> = rgb
        .chunks_exact(3)
        .map(|p| {
            (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])) / 255.0
        })
        .collect();
    let (y, aw, ah) = reduce_box(&lum, w, h, f);
    drop(lum);
    // Warm saturated pixels (orange, amber, red: red leads, blue trails) for date stamps.
    let warm: Vec<f32> = rgb
        .chunks_exact(3)
        .map(|p| {
            let (r, g, b) = (f32::from(p[0]), f32::from(p[1]), f32::from(p[2]));
            let mx = r.max(g).max(b);
            let sat = if mx > 0.0 {
                (mx - r.min(g).min(b)) / mx
            } else {
                0.0
            };
            f32::from(u8::from(r >= g && g >= b && mx > 165.0 && sat > 0.65))
        })
        .collect();
    let (warm_lo, _, _) = reduce_box(&warm, w, h, f);
    let planes: [Vec<f32>; 3] =
        std::array::from_fn(|c| reduce_map(w, h, f, |i| f32::from(rgb[i * 3 + c]) / 255.0).0);
    drop(warm);
    let long = aw.max(ah);
    let gap = ((long as f32 * 0.012).round() as usize).max(2);

    let mut blobs: Vec<Blob> = Vec::new();
    for white in [true, false] {
        blobs.extend(blobs_for(&y, &planes, aw, ah, white, search));
    }

    // Date stamps: warm saturated digits near a corner.
    let warm_map: Vec<u8> = warm_lo.iter().map(|&v| u8::from(v > 0.4)).collect();
    let warm_merged = grow_box(&warm_map, aw, ah, gap, 1);
    let warm_letters = components(&warm_map, aw, ah);
    for c in components(&warm_merged, aw, ah) {
        let r = c.rect;
        let ed = edge_distance(&r, aw, ah);
        if ed > 0.15 || r.h < 4 || r.h as f32 > ah as f32 * 0.12 || (r.w as f32) < r.h as f32 * 1.5
        {
            continue;
        }
        let digits = warm_letters
            .iter()
            .filter(|l| {
                l.rect.x >= r.x
                    && l.rect.y >= r.y
                    && l.rect.x + l.rect.w <= r.x + r.w
                    && l.rect.y + l.rect.h <= r.y + r.h
                    && l.pixels >= 3
            })
            .count();
        if digits < 4 {
            continue;
        }
        // Digits are alike: one height, standing on one line. Skin, a flag stripe or a warm
        // sunset make parts of every size and no line.
        let parts: Vec<&Comp> = warm_letters
            .iter()
            .filter(|l| {
                l.rect.x >= r.x
                    && l.rect.y >= r.y
                    && l.rect.x + l.rect.w <= r.x + r.w
                    && l.rect.y + l.rect.h <= r.y + r.h
                    && l.pixels >= 3
            })
            .collect();
        let mut hs: Vec<usize> = parts.iter().map(|l| l.rect.h).collect();
        hs.sort_unstable();
        let med = hs[hs.len() / 2] as f32;
        let like: Vec<&&Comp> = parts
            .iter()
            .filter(|l| (l.rect.h as f32) >= med * 0.6 && (l.rect.h as f32) <= med * 1.5)
            .collect();
        if like.len() < 4 || (like.len() as f32) < parts.len() as f32 * 0.6 {
            continue;
        }
        let mut bottoms: Vec<usize> = like.iter().map(|l| l.rect.y + l.rect.h).collect();
        bottoms.sort_unstable();
        let base = bottoms[bottoms.len() / 2] as f32;
        let on_line = bottoms
            .iter()
            .filter(|&&b| (b as f32 - base).abs() <= 0.25 * med)
            .count();
        if (on_line as f32) < like.len() as f32 * 0.7 {
            continue;
        }
        let mut sub = vec![0u8; r.w * r.h];
        for yy in 0..r.h {
            for xx in 0..r.w {
                sub[yy * r.w + xx] = warm_map[(r.y + yy) * aw + r.x + xx];
            }
        }
        blobs.push(Blob {
            rect: r,
            score: 0.8,
            kind: Kind::Stamp,
            strokes: sub,
        });
    }

    // A candidate that sits mostly on a head is skin, hair, teeth or a collar, not an overlay. It
    // is dropped before the best of each group is picked, so it cannot hide a real mark beside it.
    let faces = if blobs.is_empty() {
        Vec::new()
    } else {
        faces()
    };
    let heads: Vec<Rect> = faces.iter().map(|f| head_box(f, w, h)).collect();
    let heads_lo: Vec<Rect> = heads
        .iter()
        .map(|r| Rect {
            x: r.x / f,
            y: r.y / f,
            w: (r.w / f).max(1),
            h: (r.h / f).max(1),
        })
        .collect();
    let before = blobs.len();
    blobs.retain(|b| {
        !heads_lo
            .iter()
            .any(|hd| covered_by(&b.rect, hd) >= 0.4 || contains_center(hd, &b.rect))
    });
    let spared = before - blobs.len();

    // Overlapping candidates are one mark: keep the best scoring one of each group.
    blobs.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Blob> = Vec::new();
    for b in blobs {
        if !kept.iter().any(|k| k.rect.overlaps(&b.rect)) {
            kept.push(b);
        }
    }

    // Rasterize: the strokes, closed by a few pixels, at full resolution.
    let mut mask_lo = vec![0u8; aw * ah];
    let close_r = ((long as f32 * 0.006).round() as usize).max(1);
    for b in &kept {
        let r = b.rect;
        let closed = dilate(&b.strokes, r.w, r.h, close_r);
        for yy in 0..r.h {
            for xx in 0..r.w {
                if closed[yy * r.w + xx] != 0 {
                    mask_lo[(r.y + yy) * aw + r.x + xx] = 255;
                }
            }
        }
    }
    let mask = if f == 1 {
        mask_lo.iter().map(|&v| u8::from(v != 0)).collect()
    } else {
        resize_u8(&mask_lo, aw, ah, 1, w, h, Kernel::Bilinear)
            .map(|m| m.iter().map(|&v| u8::from(v > 60)).collect())
            .unwrap_or_else(|_| vec![0; w * h])
    };
    // Where a mark crosses a head, the head stays as it is.
    let mut mask = mask;
    let mut clipped = false;
    for hd in &heads {
        for y in hd.y..(hd.y + hd.h).min(h) {
            for v in &mut mask[y * w + hd.x..y * w + (hd.x + hd.w).min(w)] {
                if *v != 0 {
                    *v = 0;
                    clipped = true;
                }
            }
        }
    }
    let regions = kept
        .into_iter()
        .map(|b| Region {
            rect: Rect {
                x: b.rect.x * f,
                y: b.rect.y * f,
                w: (b.rect.w * f).min(w - b.rect.x * f),
                h: (b.rect.h * f).min(h - b.rect.y * f),
            },
            score: b.score,
            kind: b.kind,
        })
        .collect();
    Detection {
        regions,
        mask,
        spared: spared + usize::from(clipped),
    }
}

/// True when the middle of `r` is inside `o`.
fn contains_center(o: &Rect, r: &Rect) -> bool {
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    cx >= o.x && cx < o.x + o.w && cy >= o.y && cy < o.y + o.h
}

/// Where a rectangle sits in words ("bottom right", "top left", "center").
pub fn place_words(r: &Rect, w: usize, h: usize) -> String {
    let cx = (r.x as f32 + r.w as f32 / 2.0) / w as f32;
    let cy = (r.y as f32 + r.h as f32 / 2.0) / h as f32;
    let v = if cy < 0.33 {
        "top"
    } else if cy > 0.67 {
        "bottom"
    } else {
        "middle"
    };
    let hh = if cx < 0.33 {
        "left"
    } else if cx > 0.67 {
        "right"
    } else {
        "center"
    };
    match (v, hh) {
        ("middle", "center") => "center".into(),
        ("middle", s) => s.into(),
        (s, "center") => s.into(),
        (a, b) => format!("{a} {b}"),
    }
}

/// Run the module on each input.
pub fn run(io: &dyn VisionIo, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let _scope = runtime::JobScope;
    if !opts.flag("own_content", false) {
        return Err(ImageError::BadOption(
            "This tool is for pictures you own or have the right to edit: your own photos with a date stamp, your own exports with a logo you added, your own AI images with a visible badge. Set own_content to true to confirm that, then run it again.".into(),
        ));
    }
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        if io.cancel().is_cancelled() {
            return Err(ImageError::Cancelled);
        }
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Looking for marks in {}", display_name(path)),
        );
        match one(io, path, opts) {
            Ok(Some(p)) => outputs.push(p),
            Ok(None) => {}
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) if inputs.len() > 1 => {
                io.note(format!("{}: skipped. {e}", display_name(path)));
                first_err.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    if outputs.is_empty() {
        if let Some(e) = first_err {
            return Err(e);
        }
        if inputs.len() == 1 {
            return Err(ImageError::Failed(
                "No watermark, logo or date stamp was found, so nothing was changed. Mark the area yourself with rect=x,y,w,h (pixels or percentages).".into(),
            ));
        }
    }
    Ok(outputs)
}

fn one(io: &dyn VisionIo, path: &std::path::Path, opts: &Opts) -> Result<Option<PathBuf>> {
    let started = Instant::now();
    let animation = Animation::from_opt(opts.str("animation"));
    let expand = opts.u32("expand").unwrap_or(4) as usize;
    let search = if opts.str("search") == Some("edges") {
        Search::Edges
    } else {
        Search::Anywhere
    };
    let mut src = load_source(io, path, animation, MAX_WORK_PIXELS)?;
    let name = src.name.clone();
    let (w, h) = (src.w, src.h);
    // Faces are found first and never filled in: not by the detector, not by a box typed by hand.
    let mut spared = 0usize;
    let (regions, mask) = match opts.str("rect") {
        Some(t) => {
            let rects = parse_rects(t, w, h)?;
            let faces = crate::faces::find(&src.rgb, w, h);
            if rects.iter().any(|r| {
                faces
                    .iter()
                    .any(|f| covered_by(f, r) >= 0.5 || contains_center(f, r))
            }) {
                return Err(ImageError::Unsupported(
                    "That area is a face. Panoply never fills in a face, because the result would be a stranger's. Mark only the watermark itself with rect, or leave the face out of the box.".into(),
                ));
            }
            let mut m = vec![0u8; w * h];
            for r in &rects {
                for y in r.y..r.y + r.h {
                    m[y * w + r.x..y * w + r.x + r.w].fill(1);
                }
            }
            let regions = rects
                .into_iter()
                .map(|rect| Region {
                    rect,
                    score: 1.0,
                    kind: Kind::Manual,
                })
                .collect::<Vec<_>>();
            (regions, m)
        }
        None => {
            let d = detect_with(&src.rgb, w, h, search, || {
                crate::faces::find(&src.rgb, w, h)
            });
            spared = d.spared;
            (d.regions, d.mask)
        }
    };
    if spared > 0 {
        io.note(format!(
            "{name}: left {} on a face alone; Panoply never fills in a face",
            if spared == 1 {
                "one spot".to_string()
            } else {
                format!("{spared} spots")
            }
        ));
    }
    if regions.is_empty() {
        io.note(format!(
            "{name}: no watermark, logo or date stamp found; nothing changed"
        ));
        return Ok(None);
    }
    for r in &regions {
        io.note(format!(
            "{name}: found {} at the {} ({:.0}% x {:.0}% of the picture)",
            r.kind.word(),
            place_words(&r.rect, w, h),
            100.0 * r.rect.w as f64 / w as f64,
            100.0 * r.rect.h as f64 / h as f64
        ));
    }
    let covered = mask.iter().filter(|&&v| v != 0).count();
    if covered as f64 > 0.35 * (w * h) as f64 {
        return Err(ImageError::Unsupported(
            "The marks cover a large part of the picture. A fill cannot restore that faithfully, so nothing was changed. Mark a smaller area with rect.".into(),
        ));
    }
    let fill_mode = opts.str("fill").unwrap_or("auto");
    let changed = if fill_mode == "blur" {
        blur_fill(&mut src.rgb, w, h, &mask, expand)
    } else {
        let inpainter = Inpainter::new(io)?;
        inpainter
            .fill(io, &mut src.rgb, w, h, &mask, expand)?
            .changed
    };
    let changed_px = changed.iter().filter(|&&v| v != 0).count();
    let alpha = src.alpha.take().map(|mut a| {
        for (v, &c) in a.iter_mut().zip(&changed) {
            if c != 0 {
                *v = 255;
            }
        }
        a
    });
    let img = image_from(w, h, &src.rgb, alpha.as_deref());
    let format = parse_format(opts.str("format").filter(|f| *f != "keep"), src.info.format);
    let (out_path, bytes) = write_result(
        io,
        &img,
        &WriteSpec {
            src: &src,
            label: "clean",
            format,
            quality: opts.f64("quality"),
            keep_metadata: opts.flag("keep_metadata", true),
        },
    )?;
    if opts.flag("save_mask", false) {
        let m255: Vec<u8> = changed.iter().map(|&v| v * 255).collect();
        let gray = image_from(
            w,
            h,
            &m255.iter().flat_map(|&v| [v, v, v]).collect::<Vec<_>>(),
            None,
        );
        let _ = write_result(
            io,
            &gray,
            &WriteSpec {
                src: &src,
                label: "mask",
                format: panoply_image::format::Format::Png,
                quality: None,
                keep_metadata: false,
            },
        )?;
    }
    io.note(format!(
        "{name}: {} mark{} cleaned ({:.2}% of the picture changed) in {:.1} s, {}; pixels outside the mask are unchanged. Wrote {}",
        regions.len(),
        if regions.len() == 1 { "" } else { "s" },
        100.0 * changed_px as f64 / (w * h) as f64,
        started.elapsed().as_secs_f64(),
        human_bytes(bytes as u64),
        file_name(&out_path)
    ));
    for n in src.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(Some(out_path))
}

/// Replaces the masked pixels with a blur of the surroundings (for flat areas). Returns where
/// pixels were allowed to change.
pub fn blur_fill(rgb: &mut [u8], w: usize, h: usize, mask: &[u8], expand: usize) -> Vec<u8> {
    let mut changed = vec![0u8; w * h];
    for b in clusters(mask, w, h) {
        let win = b.grown(expand + 48 + b.w.max(b.h) / 2, w, h);
        let mut m = vec![0u8; win.w * win.h];
        for y in 0..win.h {
            for x in 0..win.w {
                m[y * win.w + x] = mask[(win.y + y) * w + win.x + x];
            }
        }
        let grown = dilate(&m, win.w, win.h, expand);
        let sigma = (b.w.min(b.h) as f32 * 0.5).max(4.0);
        let weight: Vec<f32> = grown.iter().map(|&g| 1.0 - f32::from(g)).collect();
        let mut wb = weight.clone();
        gaussian_blur(&mut wb, win.w, win.h, sigma);
        let mut planes: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (k, plane) in planes.iter_mut().enumerate() {
            let mut p: Vec<f32> = (0..win.w * win.h)
                .map(|i| {
                    let (x, y) = (i % win.w, i / win.w);
                    f32::from(rgb[((win.y + y) * w + win.x + x) * 3 + k]) * weight[i]
                })
                .collect();
            gaussian_blur(&mut p, win.w, win.h, sigma);
            *plane = p;
        }
        for y in 0..win.h {
            for x in 0..win.w {
                let i = y * win.w + x;
                if grown[i] != 0 && wb[i] > 1e-3 {
                    let d = ((win.y + y) * w + win.x + x) * 3;
                    for k in 0..3 {
                        rgb[d + k] = (planes[k][i] / wb[i]).clamp(0.0, 255.0) as u8;
                    }
                    changed[(win.y + y) * w + win.x + x] = 1;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth gradient picture.
    fn background(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let t = (x + y) as f32 / (w + h) as f32;
                v.extend_from_slice(&[
                    (60.0 + 90.0 * t) as u8,
                    (90.0 + 60.0 * t) as u8,
                    (120.0 - 40.0 * t) as u8,
                ]);
            }
        }
        v
    }

    /// Draws `n` block letters (filled 6 x 9 rectangles with gaps) as a white overlay with the
    /// given opacity, left edge at `x0`.
    fn stamp_letters(rgb: &mut [u8], w: usize, x0: usize, y0: usize, n: usize, alpha: f32) {
        for k in 0..n {
            for y in 0..14 {
                for x in 0..8 {
                    // Leave a notch so the letters are not plain boxes.
                    if x > 2 && x < 5 && y > 3 && y < 10 {
                        continue;
                    }
                    let i = ((y0 + y) * w + x0 + k * 13 + x) * 3;
                    for c in 0..3 {
                        rgb[i + c] = (f32::from(rgb[i + c]) * (1.0 - alpha) + 255.0 * alpha) as u8;
                    }
                }
            }
        }
    }

    #[test]
    fn sliding_extremum_matches_a_direct_scan() {
        let v: Vec<f32> = (0..40).map(|i| ((i * 29) % 17) as f32).collect();
        let mut mx = vec![0f32; 40];
        let mut mn = vec![0f32; 40];
        sliding(&v, &mut mx, 3, true);
        sliding(&v, &mut mn, 3, false);
        for i in 0..40usize {
            let lo = i.saturating_sub(3);
            let hi = (i + 3).min(39);
            let slice = &v[lo..=hi];
            assert_eq!(mx[i], slice.iter().cloned().fold(f32::MIN, f32::max));
            assert_eq!(mn[i], slice.iter().cloned().fold(f32::MAX, f32::min));
        }
    }

    #[test]
    fn a_translucent_corner_stamp_is_found() {
        let (w, h) = (640usize, 400usize);
        let mut img = background(w, h);
        stamp_letters(&mut img, w, 470, 360, 10, 0.55);
        let d = detect(&img, w, h, Search::Anywhere);
        assert!(!d.regions.is_empty(), "nothing found");
        let r = &d.regions[0];
        assert!(r.rect.x + r.rect.w > 540 && r.rect.y > 330, "{:?}", r.rect);
        // The mask covers most of the letter pixels and little else.
        let mut inside = 0;
        let mut total = 0;
        for y in 360..374 {
            for x in 470..470 + 10 * 13 {
                if (x - 470) % 13 < 8 {
                    total += 1;
                    inside += usize::from(d.mask[y * w + x] != 0);
                }
            }
        }
        assert!(inside as f32 / total as f32 > 0.85, "{inside}/{total}");
        let outside_top = d.mask[..w * 300].iter().filter(|&&v| v != 0).count();
        assert_eq!(outside_top, 0);
    }

    #[test]
    fn a_clean_picture_has_no_marks() {
        let (w, h) = (640usize, 400usize);
        let img = background(w, h);
        assert!(detect(&img, w, h, Search::Anywhere).regions.is_empty());
    }

    #[test]
    fn place_words_name_the_corner() {
        let r = Rect {
            x: 900,
            y: 950,
            w: 90,
            h: 30,
        };
        assert_eq!(place_words(&r, 1000, 1000), "bottom right");
        let c = Rect {
            x: 450,
            y: 480,
            w: 100,
            h: 40,
        };
        assert_eq!(place_words(&c, 1000, 1000), "center");
    }

    #[test]
    fn blur_fill_changes_only_inside_the_expanded_mask() {
        let (w, h) = (200usize, 120usize);
        let mut img = background(w, h);
        let orig = img.clone();
        let mut mask = vec![0u8; w * h];
        for y in 50..60 {
            for x in 80..120 {
                mask[y * w + x] = 1;
            }
        }
        for v in img[(55 * w + 90) * 3..(55 * w + 90) * 3 + 3].iter_mut() {
            *v = 250;
        }
        let changed = blur_fill(&mut img, w, h, &mask, 3);
        for i in 0..w * h {
            if changed[i] == 0 {
                assert_eq!(&img[i * 3..i * 3 + 3], &orig[i * 3..i * 3 + 3], "pixel {i}");
            }
        }
        // The bright dot is gone.
        assert!(img[(55 * w + 90) * 3] < 200);
    }

    /// A picture with a face-sized patch of skin and teeth: bright small parts in a row, the kind
    /// of structure that read as "text on the chin" before UP-38.
    fn portrait(w: usize, h: usize) -> (Vec<u8>, Rect) {
        let mut img = background(w, h);
        let face = Rect {
            x: w / 3,
            y: h / 5,
            w: w / 3,
            h: h / 2,
        };
        for y in face.y..face.y + face.h {
            for x in face.x..face.x + face.w {
                let i = (y * w + x) * 3;
                img[i..i + 3].copy_from_slice(&[205, 150, 120]);
            }
        }
        // Teeth: a row of light blocks across the lower face.
        let ty = face.y + face.h * 7 / 10;
        stamp_letters(&mut img, w, face.x + face.w / 6, ty, 8, 0.5);
        (img, face)
    }

    #[test]
    fn a_row_of_light_parts_on_a_face_is_not_a_mark_and_is_never_filled() {
        let (w, h) = (640usize, 400usize);
        let (img, face) = portrait(w, h);
        // Without the face the row of teeth looks like text.
        let bare = detect(&img, w, h, Search::Anywhere);
        assert!(
            !bare.regions.is_empty(),
            "the test picture needs a candidate"
        );
        let d = detect_with(&img, w, h, Search::Anywhere, || vec![face]);
        assert!(d.regions.is_empty(), "{:?}", d.regions);
        assert!(d.mask.iter().all(|&v| v == 0));
        assert!(d.spared >= 1);
    }

    #[test]
    fn a_mark_that_crosses_a_face_is_found_beside_it_and_the_head_is_cut_out_of_the_mask() {
        let (w, h) = (640usize, 400usize);
        let (mut img, face) = portrait(w, h);
        // A stamp in the corner, far from the head, and a long row across the head and beyond.
        stamp_letters(&mut img, w, 470, 360, 10, 0.55);
        let d = detect_with(&img, w, h, Search::Anywhere, || vec![face]);
        assert!(
            d.regions.iter().any(|r| r.rect.y > 330),
            "the corner mark is still found: {:?}",
            d.regions
        );
        let head = head_box(&face, w, h);
        for y in head.y..head.y + head.h {
            for x in head.x..head.x + head.w {
                assert_eq!(d.mask[y * w + x], 0, "({x},{y}) is on the head");
            }
        }
    }

    #[test]
    fn the_head_box_is_taller_than_the_face_for_hair_and_stays_in_the_picture() {
        let f = Rect {
            x: 100,
            y: 20,
            w: 80,
            h: 100,
        };
        let b = head_box(&f, 300, 400);
        assert!(b.y == 0 && b.h >= f.h + 20 && b.x < f.x && b.x + b.w > f.x + f.w);
        let edge = Rect {
            x: 250,
            y: 380,
            w: 50,
            h: 20,
        };
        let b = head_box(&edge, 300, 400);
        assert!(b.x + b.w <= 300 && b.y + b.h <= 400);
    }

    /// Cream letters printed on a blue ground, the way stars sit on a flag or a label on a bottle:
    /// a colour of their own, not a blend of white into what is under them.
    #[test]
    fn printed_lettering_is_not_taken_for_an_overlay_but_a_translucent_blend_is() {
        let (w, h) = (640usize, 400usize);
        let blue = |img: &mut Vec<u8>| {
            for p in img.chunks_exact_mut(3) {
                p.copy_from_slice(&[20, 30, 110]);
            }
        };
        let mut printed = background(w, h);
        blue(&mut printed);
        // Opaque cream ink, edge distance within the corner band.
        for k in 0..10 {
            for y in 360..374 {
                for x in 0..8 {
                    if x > 2 && x < 5 && y > 363 && y < 370 {
                        continue;
                    }
                    let i = (y * w + 470 + k * 13 + x) * 3;
                    printed[i..i + 3].copy_from_slice(&[238, 226, 170]);
                }
            }
        }
        assert!(
            detect(&printed, w, h, Search::Anywhere).regions.is_empty(),
            "printed cream letters are scene text"
        );
        let mut blended = background(w, h);
        blue(&mut blended);
        stamp_letters(&mut blended, w, 470, 360, 10, 0.55);
        assert!(
            !detect(&blended, w, h, Search::Anywhere).regions.is_empty(),
            "a white overlay at 55% is still found"
        );
    }
}
