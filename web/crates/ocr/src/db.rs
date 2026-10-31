//! Turning the detector's probability map into text line rectangles (the DB post-process of
//! PaddleOCR, without OpenCV).
//!
//! The map is thresholded, its connected regions become candidate lines, each region's
//! principal axis gives the line's direction (so a page scanned a couple of degrees crooked
//! still gets straight crops), and the rectangle is grown back out by the DB unclip rule,
//! because the network predicts a shrunk core of each line.

use super::imgops::Oriented;

/// Post-process settings. The defaults are PP-OCRv5's own (`inference.yml`).
#[derive(Debug, Clone, Copy)]
pub struct DbParams {
    /// Probability above which a pixel is text.
    pub thresh: f32,
    /// Regions whose mean probability is below this are dropped.
    pub box_thresh: f32,
    /// How far the shrunk core is grown back: distance = area * ratio / perimeter.
    pub unclip_ratio: f32,
    /// Regions thinner than this many map pixels are noise.
    pub min_size: f32,
    /// Most regions kept.
    pub max_candidates: usize,
}

impl Default for DbParams {
    fn default() -> Self {
        DbParams {
            thresh: 0.3,
            box_thresh: 0.6,
            unclip_ratio: 1.5,
            min_size: 3.0,
            max_candidates: 1000,
        }
    }
}

/// One detected line, in the coordinates of the probability map: its rectangle, grown by the
/// unclip rule.
#[derive(Debug, Clone, Copy)]
pub struct DetBox {
    /// The rectangle.
    pub rect: Oriented,
}

/// Find text lines in a `w x h` probability map.
pub fn boxes(prob: &[f32], w: usize, h: usize, p: &DbParams) -> Vec<DetBox> {
    debug_assert_eq!(prob.len(), w * h);
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut pix: Vec<(f32, f32)> = Vec::new();
    for start in 0..w * h {
        if seen[start] || prob[start] <= p.thresh {
            continue;
        }
        pix.clear();
        let mut psum = 0f32;
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            pix.push((x as f32, y as f32));
            psum += prob[i];
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && prob[j] > p.thresh {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if pix.len() < 4 {
            continue;
        }
        let score = psum / pix.len() as f32;
        if score < p.box_thresh {
            continue;
        }
        let Some(rect) = fit(&pix) else { continue };
        if rect.w.min(rect.h) < p.min_size {
            continue;
        }
        let rect = unclip(rect, p.unclip_ratio);
        if rect.w.min(rect.h) < p.min_size + 2.0 {
            continue;
        }
        out.push(DetBox { rect });
        if out.len() >= p.max_candidates {
            break;
        }
    }
    out
}

/// The rectangle through a region's pixels, along its principal axis.
fn fit(pix: &[(f32, f32)]) -> Option<Oriented> {
    let n = pix.len() as f32;
    let (mut mx, mut my) = (0f32, 0f32);
    for &(x, y) in pix {
        mx += x;
        my += y;
    }
    mx /= n;
    my /= n;
    let (mut sxx, mut sxy, mut syy) = (0f32, 0f32, 0f32);
    for &(x, y) in pix {
        let (dx, dy) = (x - mx, y - my);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    sxx /= n;
    sxy /= n;
    syy /= n;
    // Principal axis angle, and how elongated the region is (ratio of the two spreads).
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let tr = sxx + syy;
    let det = sxx * syy - sxy * sxy;
    let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
    let (l1, l2) = (tr / 2.0 + disc, (tr / 2.0 - disc).max(1e-6));
    let elong = (l1 / l2).sqrt();
    // A region that is not clearly elongated has no reliable direction, and one whose long
    // axis is more than 45 degrees off horizontal is a single tall mark (a character, a rule)
    // far more often than a vertical line of text: keep those axis aligned.
    let angle = if elong < 2.0 || angle.abs() > std::f32::consts::FRAC_PI_4 {
        0.0
    } else {
        angle
    };
    let (s, c) = angle.sin_cos();
    let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in pix {
        let (dx, dy) = (x - mx, y - my);
        let u = dx * c + dy * s;
        let v = -dx * s + dy * c;
        umin = umin.min(u);
        umax = umax.max(u);
        vmin = vmin.min(v);
        vmax = vmax.max(v);
    }
    let (w, h) = (umax - umin + 1.0, vmax - vmin + 1.0);
    let (cu, cv) = ((umax + umin) / 2.0, (vmax + vmin) / 2.0);
    // Back to image coordinates; the +0.5 puts the centre in the middle of the pixel square.
    Some(Oriented {
        cx: mx + cu * c - cv * s + 0.5,
        cy: my + cu * s + cv * c + 0.5,
        w,
        h,
        angle,
    })
}

/// Grow a rectangle by the DB rule: distance = area * ratio / perimeter on every side.
fn unclip(r: Oriented, ratio: f32) -> Oriented {
    let d = r.w * r.h * ratio / (2.0 * (r.w + r.h));
    Oriented {
        w: r.w + 2.0 * d,
        h: r.h + 2.0 * d,
        ..r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn bar(map: &mut [f32], w: usize, cx: f32, cy: f32, len: f32, thick: f32, ang: f32, val: f32) {
        let (s, c) = ang.sin_cos();
        for y in 0..map.len() / w {
            for x in 0..w {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let u = dx * c + dy * s;
                let v = -dx * s + dy * c;
                if u.abs() <= len / 2.0 && v.abs() <= thick / 2.0 {
                    map[y * w + x] = val;
                }
            }
        }
    }

    #[test]
    fn a_horizontal_bar_gives_one_grown_rectangle() {
        let (w, h) = (200, 100);
        let mut m = vec![0f32; w * h];
        bar(&mut m, w, 100.0, 50.0, 120.0, 10.0, 0.0, 0.9);
        let b = boxes(&m, w, h, &DbParams::default());
        assert_eq!(b.len(), 1);
        let r = b[0].rect;
        assert!(r.angle.abs() < 0.01);
        assert!(
            (r.cx - 100.0).abs() < 1.5 && (r.cy - 50.0).abs() < 1.5,
            "{r:?}"
        );
        // 120 x 10 grown by d = 1200 * 1.5 / 260 = 6.9 on every side.
        assert!((r.w - (120.0 + 13.8)).abs() < 3.0, "{r:?}");
        assert!((r.h - (10.0 + 13.8)).abs() < 3.0, "{r:?}");
    }

    #[test]
    fn a_crooked_line_keeps_its_angle() {
        let (w, h) = (300, 200);
        let mut m = vec![0f32; w * h];
        let ang = 3f32.to_radians();
        bar(&mut m, w, 150.0, 100.0, 200.0, 10.0, ang, 0.95);
        let b = boxes(&m, w, h, &DbParams::default());
        assert_eq!(b.len(), 1);
        assert!((b[0].rect.angle - ang).abs() < 0.01, "{:?}", b[0].rect);
        // The thickness is the line's, not inflated by the tilt.
        assert!(b[0].rect.h < 10.0 + 18.0, "{:?}", b[0].rect);
    }

    #[test]
    fn two_lines_are_two_boxes_and_noise_is_dropped() {
        let (w, h) = (200, 120);
        let mut m = vec![0f32; w * h];
        bar(&mut m, w, 100.0, 30.0, 150.0, 10.0, 0.0, 0.9);
        bar(&mut m, w, 100.0, 80.0, 150.0, 10.0, 0.0, 0.9);
        // A weak region (below the box threshold) and a speck (too thin).
        bar(&mut m, w, 30.0, 110.0, 20.0, 6.0, 0.0, 0.4);
        bar(&mut m, w, 180.0, 110.0, 4.0, 1.0, 0.0, 0.95);
        let b = boxes(&m, w, h, &DbParams::default());
        assert_eq!(b.len(), 2, "{b:?}");
    }

    #[test]
    fn a_tall_mark_stays_axis_aligned() {
        let (w, h) = (100, 100);
        let mut m = vec![0f32; w * h];
        // A 6 x 30 vertical mark: elongated, but not a vertical line of text.
        bar(&mut m, w, 50.0, 50.0, 6.0, 30.0, 0.0, 0.9);
        let b = boxes(&m, w, h, &DbParams::default());
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].rect.angle, 0.0);
        assert!(b[0].rect.h > b[0].rect.w);
    }

    #[test]
    fn an_empty_map_has_no_boxes() {
        assert!(boxes(&vec![0.0; 64 * 64], 64, 64, &DbParams::default()).is_empty());
    }
}
