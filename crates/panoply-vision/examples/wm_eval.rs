//! Measures watermark detection against synthetic marks with known masks.
//!
//! Usage: wm_eval DIR  (DIR has `marked/*.jpg`, `gt/*.png` with the same names, `clean/*.jpg`)
//! Prints one line per case: pixel recall of the marks, precision of the detected mask, the
//! detected area, and the time detection took.
#![allow(clippy::chunks_exact_to_as_chunks)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use panoply_image::ops::FsIo;
use panoply_image::ops::common::Animation;
use panoply_vision::px::dilate;
use panoply_vision::watermark::{Search, detect};
use panoply_vision::work::load_source;

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == ext))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("DIR"));
    let search = if std::env::args().nth(2).as_deref() == Some("edges") {
        Search::Edges
    } else {
        Search::Anywhere
    };
    let io = FsIo::new();
    let (mut sum_r, mut sum_p, mut n) = (0.0, 0.0, 0);
    println!("case\trecall\tprecision\tdetected%\tregions\tms");
    for m in files(&dir.join("marked"), "jpg") {
        let stem = m.file_stem().unwrap().to_string_lossy().into_owned();
        let src = load_source(&io, &m, Animation::First, 24_000_000).unwrap();
        let gt = load_source(
            &io,
            &dir.join("gt").join(format!("{stem}.png")),
            Animation::First,
            24_000_000,
        )
        .unwrap();
        let gtm: Vec<u8> = gt
            .rgb
            .chunks_exact(3)
            .map(|p| u8::from(p[0] > 127))
            .collect();
        let t = Instant::now();
        let d = detect(&src.rgb, src.w, src.h, search);
        let ms = t.elapsed().as_millis();
        let near = dilate(&gtm, src.w, src.h, (src.h / 100).max(3));
        let gt_px = gtm.iter().filter(|&&v| v != 0).count().max(1);
        let hit = gtm
            .iter()
            .zip(&d.mask)
            .filter(|(a, b)| **a != 0 && **b != 0)
            .count();
        let det_px = d.mask.iter().filter(|&&v| v != 0).count();
        let good = near
            .iter()
            .zip(&d.mask)
            .filter(|(a, b)| **a != 0 && **b != 0)
            .count();
        let recall = hit as f64 / gt_px as f64;
        let precision = if det_px == 0 {
            1.0
        } else {
            good as f64 / det_px as f64
        };
        let kind = stem.rsplit('_').next().unwrap_or("");
        if kind != "F" {
            sum_r += recall;
            sum_p += precision;
            n += 1;
        }
        if let Ok(dbg) = std::env::var("WM_DEBUG_DIR") {
            // Detected mask in red, missed marks in green, on a dimmed copy of the picture.
            let mut ppm = format!("P6\n{} {}\n255\n", src.w, src.h).into_bytes();
            for (i, (&gv, &dv)) in gtm.iter().zip(&d.mask).enumerate() {
                let p = &src.rgb[i * 3..i * 3 + 3];
                let (g, dm) = (gv != 0, dv != 0);
                let px = match (g, dm) {
                    (true, true) => [255u8, 255, 0],
                    (false, true) => [255, 0, 0],
                    (true, false) => [0, 255, 0],
                    _ => [p[0] / 2 + 60, p[1] / 2 + 60, p[2] / 2 + 60],
                };
                ppm.extend_from_slice(&px);
            }
            let _ = std::fs::write(format!("{dbg}/{stem}.ppm"), ppm);
        }
        println!(
            "{stem}\t{recall:.2}\t{precision:.2}\t{:.2}\t{}\t{ms}",
            100.0 * det_px as f64 / (src.w * src.h) as f64,
            d.regions.len()
        );
    }
    println!(
        "mean over A to E\t{:.2}\t{:.2}",
        sum_r / n.max(1) as f64,
        sum_p / n.max(1) as f64
    );
    println!("-- clean pictures (every detected pixel is a false positive)");
    for c in files(&dir.join("clean"), "jpg") {
        let src = load_source(&io, &c, Animation::First, 24_000_000).unwrap();
        let d = detect(&src.rgb, src.w, src.h, search);
        let det_px = d.mask.iter().filter(|&&v| v != 0).count();
        println!(
            "{}\t{:.3}% detected, {} regions",
            c.file_stem().unwrap().to_string_lossy(),
            100.0 * det_px as f64 / (src.w * src.h) as f64,
            d.regions.len()
        );
    }
}
