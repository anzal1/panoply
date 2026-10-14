//! Tries Apple Vision's text recognizer as a proposer of watermark text on the synthetic set.
#![allow(clippy::chunks_exact_to_as_chunks)]
#[cfg(target_os = "macos")]
fn main() {
    use panoply_image::ops::FsIo;
    use panoply_image::ops::common::Animation;
    use panoply_platform_macos::ocr::{Request, recognize};
    use panoply_vision::work::load_source;
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("DIR"));
    let io = FsIo::new();
    let mut files: Vec<_> = std::fs::read_dir(dir.join("marked"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let (mut sr, mut n) = (0.0, 0);
    for m in files {
        let stem = m.file_stem().unwrap().to_string_lossy().into_owned();
        let src = load_source(&io, &m, Animation::First, 24_000_000).unwrap();
        let gt = load_source(
            &io,
            &dir.join("gt").join(format!("{stem}.png")),
            Animation::First,
            24_000_000,
        )
        .unwrap();
        let t = std::time::Instant::now();
        let obs = recognize(
            src.w,
            src.h,
            3,
            &src.rgb,
            &Request {
                languages: &[],
                auto_detect: false,
                accurate: true,
                language_correction: false,
            },
        );
        let ms = t.elapsed().as_millis();
        let obs = match obs {
            Ok(o) => o,
            Err(e) => {
                println!("{stem}: error {e}");
                continue;
            }
        };
        let mut covered = vec![false; src.w * src.h];
        let mut texts = Vec::new();
        for o in &obs {
            texts.push(format!("{:?}@{:.2}", o.text, o.confidence));
            for wd in &o.words {
                let x0 = (wd.x * src.w as f32) as usize;
                let x1 = (((wd.x + wd.w) * src.w as f32) as usize).min(src.w);
                let y1 = ((1.0 - wd.y) * src.h as f32) as usize;
                let y0 = (((1.0 - wd.y - wd.h) * src.h as f32).max(0.0)) as usize;
                for y in y0..y1.min(src.h) {
                    for x in x0..x1 {
                        covered[y * src.w + x] = true;
                    }
                }
            }
        }
        let gt_px = gt.rgb.chunks_exact(3).filter(|p| p[0] > 127).count().max(1);
        let hit = gt
            .rgb
            .chunks_exact(3)
            .zip(&covered)
            .filter(|(p, c)| p[0] > 127 && **c)
            .count();
        let kind = stem.rsplit('_').next().unwrap();
        if kind != "F" {
            sr += hit as f64 / gt_px as f64;
            n += 1;
        }
        println!(
            "{stem}\trecall {:.2}\t{} lines\t{ms} ms\t{}",
            hit as f64 / gt_px as f64,
            obs.len(),
            texts.join(" | ")
        );
    }
    println!("mean recall A-E {:.2}", sr / n as f64);
}
#[cfg(not(target_os = "macos"))]
fn main() {}
