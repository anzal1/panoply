//! Probe: for real JPEG photos, the smallest output at SSIM >= 0.97 for each table set.
use panoply_pdf::jpeg::{self, EncodeOptions, Tables};
use panoply_pdf::{resample, ssim};

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        let info = jpeg::parse_info(&bytes).unwrap();
        let Some(r) = jpeg::decode(&bytes, &info) else {
            println!("{path}: not decodable");
            continue;
        };
        // Shrink to about 1.5 MP like a 150 dpi page image.
        let s = (1_500_000.0 / (r.w * r.h) as f64).sqrt().min(1.0);
        let (w, h) = resample::scaled_size(r.w, r.h, s);
        let px = resample::shrink(&r.data, r.w, r.h, r.ch, w, h);
        let reference = ssim::luma(&px, r.ch);
        println!(
            "{path}: {}x{} -> {w}x{h}, original jpeg {} bytes",
            r.w,
            r.h,
            bytes.len()
        );
        for tables in [
            Tables::Standard,
            Tables::ImageMagick,
            Tables::MsSsim,
            Tables::PsnrHvs,
        ] {
            for (prog, sub) in [(true, true), (true, false)] {
                let ok = |q: u8| -> Option<(usize, f64)> {
                    let j = jpeg::encode(
                        &px,
                        w,
                        h,
                        r.ch,
                        EncodeOptions {
                            tables,
                            quality: q,
                            progressive: prog,
                            subsample: sub,
                        },
                    )?;
                    let i = jpeg::parse_info(&j)?;
                    let b = jpeg::decode(&j, &i)?;
                    Some((
                        j.len(),
                        ssim::ssim_gray(&reference, &ssim::luma(&b.data, r.ch), w, h),
                    ))
                };
                let (mut lo, mut hi) = (20u8, 98u8);
                if ok(hi).is_none_or(|x| x.1 < 0.97) {
                    println!("  {tables:?} prog={prog} sub={sub}: cannot reach 0.97");
                    continue;
                }
                while lo < hi {
                    let m = (lo + hi) / 2;
                    if ok(m).is_some_and(|x| x.1 >= 0.97) {
                        hi = m
                    } else {
                        lo = m + 1
                    }
                }
                let (len, s) = ok(hi).unwrap();
                println!("  {tables:?} prog={prog} sub={sub}: q{hi} {len} bytes ssim {s:.4}");
            }
        }
    }
}
