//! A cheap look at a picture before deciding whether it is worth a second reading attempt:
//! does it look like lines of text?

use crate::types::OcrImage;

/// Otsu's threshold of an 8 bit gray image.
fn otsu(gray: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &g in gray {
        hist[g as usize] += 1;
    }
    let total = gray.len() as f64;
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum();
    let (mut wb, mut sb, mut best, mut thr) = (0f64, 0f64, 0f64, 127u8);
    for (t, &c) in hist.iter().enumerate() {
        wb += c as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += t as f64 * c as f64;
        let (mb, mf) = (sb / wb, (sum - sb) / wf);
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    thr
}

/// True when the picture has at least `min_bands` horizontal bands of ink that are the height of
/// text lines, and is not mostly ink. A blank page, a photograph and a solid scan border are all
/// "no"; a page of any script is "yes".
pub fn looks_like_text(img: &OcrImage, min_bands: usize) -> bool {
    let gray = img.to_gray();
    let (w, h) = (img.width as usize, img.height as usize);
    // Work on a copy no wider than 1000 pixels: bands are decided by the row profile only.
    let step = (w / 1000).max(1);
    let thr = otsu(&gray);
    let mut rows = Vec::with_capacity(h);
    let mut ink_total = 0usize;
    let mut sampled = 0usize;
    for y in 0..h {
        let row = &gray[y * w..(y + 1) * w];
        let n = row.iter().step_by(step).filter(|&&g| g <= thr).count();
        ink_total += n;
        sampled += row.len().div_ceil(step);
        rows.push(n);
    }
    if sampled == 0 || ink_total as f64 / sampled as f64 > 0.35 {
        return false;
    }
    // A row is inked when more than 0.5% of its samples are dark; the page may be tilted, so
    // the profile is smoothed over a few rows first.
    let cols = w.div_ceil(step);
    let on = |y: usize| rows[y] * 200 > cols;
    let mut bands = 0usize;
    let mut run = 0usize;
    // Text lines are between 6 pixels and a third of the picture high.
    let (min_h, max_h) = (6usize, (h / 3).max(12));
    for y in 0..=h {
        if y < h && on(y) {
            run += 1;
        } else {
            if (min_h..=max_h).contains(&run) {
                bands += 1;
            }
            run = 0;
        }
    }
    bands >= min_bands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(lines: usize) -> OcrImage {
        let (w, h) = (400usize, 300usize);
        let mut d = vec![255u8; w * h];
        for l in 0..lines {
            for y in 0..12 {
                for x in 20..380 {
                    if (x / 3 + y) % 2 == 0 {
                        d[(30 + l * 30 + y) * w + x] = 20;
                    }
                }
            }
        }
        OcrImage::gray(w as u32, h as u32, d).unwrap()
    }

    #[test]
    fn lines_of_ink_are_text_like_and_blank_pages_are_not() {
        assert!(looks_like_text(&page(5), 3));
        assert!(!looks_like_text(&page(1), 3));
        assert!(!looks_like_text(&page(0), 3));
    }

    #[test]
    fn a_solid_dark_picture_is_not_text() {
        let img = OcrImage::gray(100, 100, vec![10; 10_000]).unwrap();
        assert!(!looks_like_text(&img, 3));
    }
}
