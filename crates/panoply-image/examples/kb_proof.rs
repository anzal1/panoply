//! Proof that every exam preset of Resize to exact KB lands inside its published size range,
//! under both ways of counting a kilobyte, on 20 varied photos.
//!
//! For each preset and photo the picture is fitted to the preset's pixel size once, then solved
//! three times: with the preset's own reading of KB (or the size that holds under both readings
//! when the portal does not say), and forced to 1000 and to 1024 bytes per KB. Each result is
//! checked against the published numbers by plain arithmetic here, not by the module's own
//! window, so a wrong window cannot hide a wrong result.
//!
//! Usage: `cargo run --release -p panoply-image --example kb_proof [markdown-file]`
//! Exit status 1 when any case lands outside its window.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use panoply_image::modules::modules;
use panoply_image::ops::FsIo;
use panoply_image::ops::common::Animation;
use panoply_image::ops::kb;
use panoply_image::ops::options::Opts;
use panoply_image::presets::{self, Preset};
use serde_json::json;

const PHOTOS: [&str; 20] = [
    "exif-orientation/landscape-1.jpg",
    "exif-orientation/landscape-2.jpg",
    "exif-orientation/landscape-3.jpg",
    "exif-orientation/landscape-4.jpg",
    "exif-orientation/landscape-5.jpg",
    "exif-orientation/landscape-6.jpg",
    "exif-orientation/landscape-7.jpg",
    "exif-orientation/landscape-8.jpg",
    "libavif/seine-sdr-gainmap.jpg",
    "libvips/ultra-hdr.jpg",
    "libavif/apple-gainmap.jpg",
    "libavif/dog-extended-xmp.jpg",
    "libavif/paris-exif-orientation-5.jpg",
    "libjpeg-turbo/testorig.jpg",
    "generated/display-p3.jpg",
    "commons/an-teallach-panorama.jpg",
    "commons/day-earth-smiled.jpg",
    "generated/exif-gps-orientation6.jpg",
    "libavif/paris-icc-exif-xmp.png",
    "generated/cmyk-adobe.jpg",
];

/// The inclusive byte range a result must be in, from the published numbers.
fn range(p: &Preset, basis: &str) -> (u64, u64) {
    let (mn, mx) = (p.min_bytes_claimed, p.max_bytes_claimed);
    match basis {
        "1000" => ((mn * 1000.0) as u64, (mx * 1000.0) as u64),
        "1024" => ((mn * 1024.0) as u64, (mx * 1024.0) as u64),
        // Unknown: inside both readings at once.
        _ => ((mn * 1024.0) as u64, (mx * 1000.0) as u64),
    }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/cache/image");
    let manifest = modules()
        .into_iter()
        .find(|m| m.manifest.module.id == "image.resize_to_kb")
        .unwrap()
        .manifest;
    let io = FsIo::new();
    let photos: Vec<PathBuf> = PHOTOS
        .iter()
        .map(|p| root.join(p))
        .filter(|p| p.is_file())
        .collect();
    assert!(
        photos.len() >= 15,
        "only {} of the 20 photos are in the corpus; run tests/corpus/fetch.py and generate.py",
        photos.len()
    );
    let started = Instant::now();
    let mut md = String::new();
    let mut bad = 0usize;
    let mut total = 0usize;
    let mut padded = 0usize;
    let mut reduced = 0usize;
    let _ = writeln!(
        md,
        "| Preset | Status | Pixels | Published window | Reading | Photos in window | Smallest result | Largest result | Padded | Worst case |"
    );
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|---|---|");
    for p in presets::all().iter().filter(|p| p.has_size_rule()) {
        let readings: Vec<&str> = if p.kb_basis == "unknown" {
            vec!["both", "1000", "1024"]
        } else {
            vec!["preset", "1000", "1024"]
        };
        for reading in readings {
            let mut opts = json!({"preset": p.id});
            if reading == "1000" || reading == "1024" {
                opts["kb_basis"] = json!(reading);
            }
            let spec = kb::spec_from(&Opts::resolve(&manifest, &opts).unwrap()).unwrap();
            let check_basis = match reading {
                "1000" | "1024" => reading,
                _ if p.kb_basis == "1000" || p.kb_basis == "1024" => p.kb_basis.as_str(),
                _ => "both",
            };
            let (lo, hi) = range(p, check_basis);
            let (mut n_ok, mut smallest, mut largest, mut pads) = (0usize, u64::MAX, 0u64, 0usize);
            let mut worst = String::from("none");
            let mut fails: Vec<String> = Vec::new();
            for photo in &photos {
                total += 1;
                let name = photo.file_name().unwrap().to_string_lossy().into_owned();
                let res = kb::prepare(&io, photo, &spec, Animation::Ask)
                    .and_then(|(base, _)| kb::solve(&base, &spec, &io));
                match res {
                    Ok(sol) => {
                        let n = sol.bytes.len() as u64;
                        if sol.padded_by > 0 {
                            pads += 1;
                            padded += 1;
                        }
                        if sol.downscaled {
                            reduced += 1;
                        }
                        let pixels_ok = spec
                            .pixels
                            .is_none_or(|(w, h)| (sol.width, sol.height) == (w, h))
                            || sol.downscaled;
                        if n >= lo && n <= hi && pixels_ok {
                            n_ok += 1;
                            smallest = smallest.min(n);
                            largest = largest.max(n);
                        } else {
                            bad += 1;
                            fails.push(format!(
                                "{name}: {n} bytes, {}x{} (window {lo} to {hi})",
                                sol.width, sol.height
                            ));
                        }
                    }
                    Err(e) => {
                        bad += 1;
                        fails.push(format!("{name}: {e}"));
                    }
                }
            }
            if let Some(f) = fails.first() {
                worst = f.clone();
            }
            let px = spec
                .pixels
                .map(|(w, h)| format!("{w} x {h}"))
                .unwrap_or_else(|| "free".into());
            let window = if p.min_bytes_claimed > 0.0 {
                format!("{} to {} KB", p.min_bytes_claimed, p.max_bytes_claimed)
            } else {
                format!("up to {} KB", p.max_bytes_claimed)
            };
            let reading_label = match reading {
                "both" => "both at once (20,480 style: min x 1024 to max x 1000)".to_string(),
                "preset" => format!("preset says {} bytes per KB", p.kb_basis),
                r => format!("{r} bytes per KB"),
            };
            let _ = writeln!(
                md,
                "| {} | {} | {} | {} | {} | {}/{} | {} | {} | {} | {} |",
                p.id,
                p.status,
                px,
                window,
                reading_label,
                n_ok,
                photos.len(),
                if smallest == u64::MAX {
                    "-".into()
                } else {
                    smallest.to_string()
                },
                if largest == 0 {
                    "-".into()
                } else {
                    largest.to_string()
                },
                pads,
                worst.replace('|', "/")
            );
            for f in fails {
                eprintln!("FAIL {} [{reading}] {f}", p.id);
            }
        }
    }
    println!("{md}");
    println!(
        "{} cases ({} photos), {} outside the window, {} needed padding, {} reduced in pixels, {:.1} s",
        total,
        photos.len(),
        bad,
        padded,
        reduced,
        started.elapsed().as_secs_f64()
    );
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, md).unwrap();
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
