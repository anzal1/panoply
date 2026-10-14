//! Compress results against other encoders at the same picture quality.
//!
//! For each corpus picture the pixels are decoded once, and "similar enough" means the same
//! thing for every contender: SSIM of luma and of chroma, the worse of the two at least 0.97
//! (the Panoply default, `panoply_image::ssim`). Each contender's quality knob is searched for
//! the smallest file that still scores 0.97, and also run at its own defaults for context.
//!
//! Contenders: Panoply `image.compress` (the module itself), MozJPEG at its defaults (the
//! `mozjpeg` crate, which is what `cjpeg` from MozJPEG runs), libjpeg-turbo `cjpeg`, `cwebp`,
//! and oxipng at its defaults for PNG. Tools that are not installed are left out and the report
//! says so.
//!
//! Usage: `cargo run --release -p panoply-image --example compress_report [markdown-file]`

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use panoply_image::color::OutSpace;
use panoply_image::decode::{self, Input, OpenOpts};
use panoply_image::encode::jpeg::{JpegOpts, encode as jpeg_encode};
use panoply_image::modules::modules;
use panoply_image::ops::FsIo;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image, quiet};
use panoply_image::ssim::{Score, score};
use panoply_image::types::{Image, Samples};
use serde_json::json;

const TARGET: f64 = 0.97;

fn decode_file(path: &Path) -> Option<Image> {
    let f = std::fs::File::open(path).ok()?;
    let ext = path.extension().and_then(|e| e.to_str());
    let mut r = decode::open(
        Input::File(f),
        ext,
        &OpenOpts {
            first_frame: true,
            ..Default::default()
        },
    )
    .ok()?;
    let (c, p) = quiet();
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        out_depth: Some(panoply_image::types::Depth::U8),
        ..Default::default()
    };
    Some(
        decode_to_image(
            &mut *r,
            &plan,
            &Ctl {
                cancel: &c,
                progress: &p,
            },
        )
        .ok()?
        .0,
    )
}

fn flatten(img: &Image) -> Image {
    let a = img.to_u8();
    if !a.layout.has_alpha() {
        return a;
    }
    let ch = a.layout.channels();
    let Samples::U8(v) = &a.samples else {
        unreachable!()
    };
    let mut out = Vec::new();
    for p in v.chunks_exact(ch) {
        let al = p[ch - 1] as u32;
        let mix = |c: u8| ((c as u32 * al + 255 * (255 - al) + 127) / 255) as u8;
        if ch == 2 {
            out.push(mix(p[0]))
        } else {
            out.extend_from_slice(&[mix(p[0]), mix(p[1]), mix(p[2])])
        }
    }
    Image {
        layout: a.layout.with_alpha(false),
        samples: Samples::U8(out),
        ..a
    }
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
        || Command::new(tool).arg("--version").output().is_ok()
}

fn write_ppm(img: &Image, path: &Path) {
    let rgb = img.to_rgb_layout();
    let Samples::U8(v) = &rgb.samples else {
        unreachable!()
    };
    let mut out = format!("P6\n{} {}\n255\n", rgb.width, rgb.height).into_bytes();
    out.extend_from_slice(v);
    std::fs::write(path, out).unwrap();
}

fn write_png(img: &Image, path: &Path) {
    let rgb = img.to_rgb_layout();
    let Samples::U8(v) = &rgb.samples else {
        unreachable!()
    };
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), rgb.width, rgb.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().unwrap().write_image_data(v).unwrap();
}

/// Smallest `q` in 1..=100 (integer) whose result scores at least the target.
fn search(mut try_q: impl FnMut(u32) -> Option<(u64, Score)>) -> Option<(u32, u64, Score)> {
    let (mut lo, mut hi) = (1u32, 100u32);
    let mut best: Option<(u32, u64, Score)> = None;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        match try_q(mid) {
            Some((n, s)) if s.worst() >= TARGET => {
                best = Some((mid, n, s));
                if mid == 1 {
                    break;
                }
                hi = mid - 1;
            }
            Some(_) => lo = mid + 1,
            None => return None,
        }
    }
    best
}

#[derive(Default, Clone)]
struct Cell {
    size: Option<u64>,
    score: Option<f64>,
    q: Option<u32>,
}

fn fmt_cell(c: &Cell) -> String {
    match (c.size, c.score) {
        (Some(n), Some(s)) => format!(
            "{n} ({s:.3}{})",
            c.q.map(|q| format!(", q{q}")).unwrap_or_default()
        ),
        (Some(n), None) => format!("{n}"),
        _ => "-".into(),
    }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/cache/image");
    let tmp = std::env::temp_dir().join(format!("panoply-compress-report-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let cjpeg = have("cjpeg");
    let cwebp = have("cwebp");
    let compress = modules()
        .into_iter()
        .find(|m| m.manifest.module.id == "image.compress")
        .unwrap();

    let jpegs = [
        "exif-orientation/landscape-1.jpg",
        "exif-orientation/landscape-3.jpg",
        "exif-orientation/landscape-6.jpg",
        "libavif/seine-sdr-gainmap.jpg",
        "libavif/apple-gainmap.jpg",
        "libavif/dog-extended-xmp.jpg",
        "libavif/paris-exif-orientation-5.jpg",
        "libvips/ultra-hdr.jpg",
        "libjpeg-turbo/testorig.jpg",
        "generated/display-p3.jpg",
        "generated/exif-gps-orientation6.jpg",
        "generated/cmyk-adobe.jpg",
        "commons/an-teallach-panorama.jpg",
    ];
    let pngs = [
        "libavif/paris-icc-exif-xmp.png",
        "generated/display-p3.png",
        "generated/rgba-16bit.png",
        "pngsuite/basn6a16.png",
        "pngsuite/basn2c16.png",
        "pngsuite/s05n3p02.png",
    ];

    let mut md = String::new();
    let mut totals = [0u64; 6]; // orig, ours, mozjpeg default, mozjpeg equal, cjpeg equal, cwebp equal
    let mut wins_vs_default = 0;
    let mut n_jpeg = 0;
    let _ = writeln!(md, "### JPEG pictures, bytes (similarity, quality)\n");
    let _ = writeln!(
        md,
        "| Picture | Pixels | Original | Panoply compress | MozJPEG defaults (q75) | MozJPEG at 0.97 | libjpeg-turbo cjpeg at 0.97 | cwebp at 0.97 |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|");
    for rel in jpegs {
        let path = root.join(rel);
        if !path.is_file() {
            continue;
        }
        let Some(src) = decode_file(&path) else {
            continue;
        };
        let src = flatten(&src);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let orig = std::fs::metadata(&path).unwrap().len();
        // Panoply.
        let out_dir = tmp.join("ours");
        let _ = std::fs::remove_dir_all(&out_dir);
        std::fs::create_dir_all(&out_dir).unwrap();
        let io = FsIo::in_dir(&out_dir);
        let ours = match compress.run(&io, std::slice::from_ref(&path), &json!({"ssim": TARGET})) {
            Ok(o) if !o.is_empty() => {
                let img = flatten(&decode_file(&o[0]).unwrap());
                Cell {
                    size: Some(std::fs::metadata(&o[0]).unwrap().len()),
                    score: Some(score(&src, &img).worst()),
                    q: None,
                }
            }
            Ok(_) => Cell {
                size: Some(orig),
                score: Some(1.0),
                q: None,
            },
            Err(e) => {
                eprintln!("{name}: {e}");
                continue;
            }
        };
        // MozJPEG defaults (progressive, trellis, optimized tables) at q75.
        let moz = |q: u32| -> Option<(u64, Score)> {
            let b = jpeg_encode(
                &src,
                &JpegOpts {
                    quality: q as f32,
                    ..Default::default()
                },
            )
            .ok()?;
            let p = tmp.join("m.jpg");
            std::fs::write(&p, &b).ok()?;
            Some((b.len() as u64, score(&src, &flatten(&decode_file(&p)?))))
        };
        let moz_default = moz(75)
            .map(|(n, s)| Cell {
                size: Some(n),
                score: Some(s.worst()),
                q: Some(75),
            })
            .unwrap_or_default();
        let moz_eq = search(moz)
            .map(|(q, n, s)| Cell {
                size: Some(n),
                score: Some(s.worst()),
                q: Some(q),
            })
            .unwrap_or_default();
        // libjpeg-turbo cjpeg.
        let mut cj_eq = Cell::default();
        if cjpeg {
            let ppm = tmp.join("in.ppm");
            write_ppm(&src, &ppm);
            let run = |q: u32| -> Option<(u64, Score)> {
                let out = tmp.join("c.jpg");
                Command::new("cjpeg")
                    .args(["-quality", &q.to_string(), "-outfile"])
                    .arg(&out)
                    .arg(&ppm)
                    .output()
                    .ok()?;
                Some((
                    std::fs::metadata(&out).ok()?.len(),
                    score(&src, &flatten(&decode_file(&out)?)),
                ))
            };
            cj_eq = search(run)
                .map(|(q, n, s)| Cell {
                    size: Some(n),
                    score: Some(s.worst()),
                    q: Some(q),
                })
                .unwrap_or_default();
        }
        let mut cw_eq = Cell::default();
        if cwebp {
            let pngp = tmp.join("in.png");
            write_png(&src, &pngp);
            let run = |q: u32| -> Option<(u64, Score)> {
                let out = tmp.join("w.webp");
                Command::new("cwebp")
                    .args(["-quiet", "-q", &q.to_string()])
                    .arg(&pngp)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .ok()?;
                Some((
                    std::fs::metadata(&out).ok()?.len(),
                    score(&src, &flatten(&decode_file(&out)?)),
                ))
            };
            cw_eq = search(run)
                .map(|(q, n, s)| Cell {
                    size: Some(n),
                    score: Some(s.worst()),
                    q: Some(q),
                })
                .unwrap_or_default();
        }
        let _ = writeln!(
            md,
            "| {name} | {}x{} | {orig} | {} | {} | {} | {} | {} |",
            src.width,
            src.height,
            fmt_cell(&ours),
            fmt_cell(&moz_default),
            fmt_cell(&moz_eq),
            fmt_cell(&cj_eq),
            fmt_cell(&cw_eq)
        );
        n_jpeg += 1;
        totals[0] += orig;
        totals[1] += ours.size.unwrap_or(orig);
        totals[2] += moz_default.size.unwrap_or(0);
        totals[3] += moz_eq.size.unwrap_or(0);
        totals[4] += cj_eq.size.unwrap_or(0);
        totals[5] += cw_eq.size.unwrap_or(0);
        if ours.size.unwrap_or(u64::MAX) <= moz_default.size.unwrap_or(0) {
            wins_vs_default += 1;
        }
    }
    let pct = |a: u64, b: u64| {
        if b == 0 {
            "-".to_string()
        } else {
            format!("{:+.1}%", 100.0 * (a as f64 - b as f64) / b as f64)
        }
    };
    let _ = writeln!(
        md,
        "\nTotals over {n_jpeg} pictures: original {} bytes, Panoply {} ({} vs original), MozJPEG defaults {} (Panoply is {} against it), MozJPEG at 0.97 {} (Panoply {}), cjpeg at 0.97 {} (Panoply {}), cwebp at 0.97 {} (Panoply {}). Panoply is no bigger than MozJPEG defaults on {wins_vs_default} of {n_jpeg}.",
        totals[0],
        totals[1],
        pct(totals[1], totals[0]),
        totals[2],
        pct(totals[1], totals[2]),
        totals[3],
        pct(totals[1], totals[3]),
        totals[4],
        pct(totals[1], totals[4]),
        totals[5],
        pct(totals[1], totals[5])
    );

    // Under a size: how close below the target the result lands (the bar is within 2 percent).
    let _ = writeln!(md, "\n### Under a size: how close below the target\n");
    let _ = writeln!(
        md,
        "| Picture | Target (bytes) | Result (bytes) | Below the target by |"
    );
    let _ = writeln!(md, "|---|---|---|---|");
    let (mut within, mut cases, mut worst_gap) = (0, 0, 0.0f64);
    for rel in jpegs.iter().take(12) {
        let path = root.join(rel);
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let out_dir = tmp.join("under");
        for frac in [0.4f64, 0.6, 0.8] {
            let _ = std::fs::remove_dir_all(&out_dir);
            std::fs::create_dir_all(&out_dir).unwrap();
            let io = FsIo::in_dir(&out_dir);
            // The reference size: quality 92 of this picture, so every target is reachable.
            let Ok(r) = compress.run(
                &io,
                std::slice::from_ref(&path),
                &json!({"target": "quality", "quality": 92}),
            ) else {
                continue;
            };
            let Some(first) = r.first() else { continue };
            let reference = std::fs::metadata(first).unwrap().len();
            let target = (reference as f64 * frac) as u64;
            let _ = std::fs::remove_file(first);
            let Ok(r) = compress.run(
                &io,
                std::slice::from_ref(&path),
                &json!({"target": "under_size", "max_size": target}),
            ) else {
                continue;
            };
            let Some(first) = r.first() else { continue };
            let n = std::fs::metadata(first).unwrap().len();
            let gap = 100.0 * (target as f64 - n as f64) / target as f64;
            cases += 1;
            if n <= target && gap <= 2.0 {
                within += 1;
            }
            worst_gap = worst_gap.max(gap);
            let _ = writeln!(md, "| {name} | {target} | {n} | {gap:.2}% |");
        }
    }
    let _ = writeln!(
        md,
        "\n{within} of {cases} results are under the target and within 2% of it (largest gap {worst_gap:.2}%)."
    );

    // Pictures that make PNG files people actually compress: a photograph saved as PNG, a flat
    // chart, a gradient logo. Written once into the scratch folder.
    let mut extra_pngs: Vec<PathBuf> = Vec::new();
    if let Some(photo) = decode_file(&root.join("exif-orientation/landscape-1.jpg")) {
        let p = tmp.join("photo-as-png.png");
        write_png(&flatten(&photo), &p);
        extra_pngs.push(p);
    }
    {
        let (w, h) = (800u32, 500u32);
        let mut chart = Image::new(
            w,
            h,
            panoply_image::types::Layout::Rgb,
            panoply_image::types::Depth::U8,
        );
        let mut logo = chart.clone();
        if let (Samples::U8(c), Samples::U8(l)) = (&mut chart.samples, &mut logo.samples) {
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let i = (y * w as usize + x) * 3;
                    let bar = x / 100 % 2 == 0 && y > 480 - (x / 100 + 1) * 40;
                    let grid = x % 100 == 0 || y % 100 == 0;
                    let v: [u8; 3] = if bar {
                        [40, 110, 200]
                    } else if grid {
                        [210, 210, 210]
                    } else {
                        [250, 250, 250]
                    };
                    c[i..i + 3].copy_from_slice(&v);
                    let d = (((x as f32 - 400.0).powi(2) + (y as f32 - 250.0).powi(2)).sqrt()
                        / 280.0)
                        .min(1.0);
                    let g = [
                        (255.0 * (1.0 - d)) as u8,
                        (120.0 + 80.0 * d) as u8,
                        (200.0 - 120.0 * d) as u8,
                    ];
                    l[i..i + 3].copy_from_slice(&g);
                }
            }
        }
        let (pc, pl) = (tmp.join("flat-chart.png"), tmp.join("gradient-logo.png"));
        write_png(&chart, &pc);
        write_png(&logo, &pl);
        extra_pngs.push(pc);
        extra_pngs.push(pl);
    }
    let _ = writeln!(md, "\n### PNG pictures, bytes (similarity)\n");
    let _ = writeln!(
        md,
        "| Picture | Pixels | Original | oxipng defaults (lossless) | Panoply, lossless only | Panoply, default (reduces colors on flat art) |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|");
    let all_pngs: Vec<PathBuf> = pngs
        .iter()
        .map(|r| root.join(r))
        .chain(extra_pngs)
        .collect();
    for path in all_pngs {
        if !path.is_file() {
            continue;
        }
        let Some(src) = decode_file(&path) else {
            continue;
        };
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let orig = std::fs::metadata(&path).unwrap().len();
        // oxipng at its defaults on the file itself. Safe stripping keeps the color profile, which
        // Panoply keeps too, and drops what does not change how the picture looks.
        let ox = {
            let raw = std::fs::read(&path).unwrap();
            let o = oxipng::Options {
                strip: oxipng::StripChunks::Safe,
                ..Default::default()
            };
            oxipng::optimize_from_memory(&raw, &o)
                .map(|b| b.len() as u64)
                .ok()
        };
        let run = |lossy: &str| -> Cell {
            let d = tmp.join("png-out");
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            let io = FsIo::in_dir(&d);
            match compress.run(
                &io,
                std::slice::from_ref(&path),
                &json!({"ssim": TARGET, "lossy_png": lossy}),
            ) {
                Ok(o) if !o.is_empty() => {
                    let img = decode_file(&o[0]).unwrap();
                    Cell {
                        size: Some(std::fs::metadata(&o[0]).unwrap().len()),
                        score: Some(score(&flatten(&src), &flatten(&img)).worst()),
                        q: None,
                    }
                }
                Ok(_) => Cell {
                    size: Some(orig),
                    score: Some(1.0),
                    q: None,
                },
                Err(_) => Cell::default(),
            }
        };
        let lossless = run("off");
        let auto = run("auto");
        let _ = writeln!(
            md,
            "| {name} | {}x{} | {orig} | {} | {} | {} |",
            src.width,
            src.height,
            ox.map(|n| n.to_string()).unwrap_or("-".into()),
            fmt_cell(&lossless),
            fmt_cell(&auto)
        );
    }
    let _ = writeln!(
        md,
        "\nTools: cjpeg {}, cwebp {}.",
        if cjpeg {
            "libjpeg-turbo (installed)"
        } else {
            "not installed"
        },
        if cwebp { "installed" } else { "not installed" }
    );
    println!("{md}");
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(p, &md).unwrap();
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
