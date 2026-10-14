//! Huge pictures stay inside the 300 MB cap (FLOWS.md budgets, IPC-7): a 400 megapixel PNG and
//! a 120 megapixel JPEG panorama go through every module that accepts them, in strips.
//!
//! These run one at a time in their own test binary, so the resident set size read here is the
//! cost of the job and not of other tests. Peak is the process high-water mark: on Linux
//! `VmHWM`, on macOS the largest `ps` reading taken every few milliseconds while the job runs.

mod common;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use panoply_image::encode::jpeg::{JpegOpts, JpegSink};
use panoply_image::pipeline::RowSink;
use panoply_image::types::{Depth, Layout, Rows};
use serde_json::json;

const CAP_MB: u64 = 300;
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[cfg(target_os = "linux")]
fn rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    out.ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn hwm_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Run `f` and return its result with the peak resident size in MB seen meanwhile.
fn with_peak<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let sampler = std::thread::spawn(move || {
        let mut peak = 0;
        while !s2.load(Ordering::Relaxed) {
            peak = peak.max(rss_kb());
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        peak
    });
    let r = f();
    stop.store(true, Ordering::Relaxed);
    #[allow(unused_mut)]
    let mut peak = sampler.join().unwrap().max(rss_kb());
    #[cfg(target_os = "linux")]
    {
        peak = peak.max(hwm_kb());
    }
    (r, peak / 1024)
}

/// A 24000 x 5000 (120 megapixel) JPEG panorama, written row by row so making it is cheap too.
fn make_panorama(path: &std::path::Path) {
    let (w, h) = (24000u32, 5000u32);
    let file = std::fs::File::create(path).unwrap();
    let mut sink = JpegSink::new(
        std::io::BufWriter::new(file),
        JpegOpts {
            quality: 80.0,
            progressive: false,
            optimize: false,
            ..Default::default()
        },
    );
    sink.begin(w, h, Layout::Rgb, Depth::U8, None).unwrap();
    let mut row = vec![0u8; w as usize * 3];
    for y in 0..h {
        for x in 0..w as usize {
            let v = ((x / 6) as u32 + y / 3) as u8;
            row[x * 3] = v;
            row[x * 3 + 1] = v.wrapping_mul(3).wrapping_add((x / 97) as u8);
            row[x * 3 + 2] = 255 - v;
        }
        sink.push(Rows::U8(&row)).unwrap();
    }
    sink.finish().unwrap();
}

#[test]
fn the_400_megapixel_png_goes_through_every_module_under_the_cap() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(src) = corpus("generated/huge-20000x20000.png") else {
        return;
    };
    let s = Scratch::new("mem-png");
    let jobs: Vec<(&str, serde_json::Value)> = vec![
        ("image.convert", json!({"to": "jpg"})),
        ("image.convert", json!({"to": "png"})),
        ("image.resize", json!({"longest": 2000})),
        ("image.resize", json!({"mode": "percent", "percent": 5})),
        (
            "image.resize_to_kb",
            json!({"width": 200, "height": 230, "min_kb": 20, "max_kb": 50}),
        ),
        ("image.strip_metadata", json!({})),
    ];
    for (id, o) in jobs {
        let (r, peak) = with_peak(|| run(id, &src, o.clone(), s.path()));
        let r = r.unwrap_or_else(|e| panic!("{id} {o}: {e}"));
        assert_eq!(r.outputs.len(), 1);
        eprintln!("{id} {o}: peak {peak} MB");
        assert!(
            peak < CAP_MB,
            "{id} {o}: peak {peak} MB is over the {CAP_MB} MB cap"
        );
    }
    // Compress handles a picture this big too, within the same cap.
    let (r, peak) = with_peak(|| run("image.compress", &src, json!({}), s.path()));
    let r = r.unwrap_or_else(|e| panic!("image.compress on 400 megapixels: {e}"));
    assert_eq!(r.outputs.len(), 1);
    eprintln!("image.compress {{}}: peak {peak} MB");
    assert!(
        peak < CAP_MB,
        "image.compress: peak {peak} MB is over the {CAP_MB} MB cap"
    );
}

#[test]
fn a_120_megapixel_jpeg_panorama_goes_through_in_strips() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let s = Scratch::new("mem-pano");
    let src = s.path().join("pano.jpg");
    make_panorama(&src);
    let out = Scratch::new("mem-pano-out");
    let jobs: Vec<(&str, serde_json::Value)> = vec![
        ("image.convert", json!({"to": "png"})),
        ("image.convert", json!({"to": "jpg"})),
        ("image.resize", json!({"longest": 3000})),
        (
            "image.resize_to_kb",
            json!({"width": 600, "height": 130, "max_kb": 40}),
        ),
        ("image.strip_metadata", json!({"remove": "everything"})),
    ];
    for (id, o) in jobs {
        let (r, peak) = with_peak(|| run(id, &src, o.clone(), out.path()));
        let r = r.unwrap_or_else(|e| panic!("{id} {o}: {e}"));
        eprintln!("{id} {o}: peak {peak} MB");
        assert!(
            peak < CAP_MB,
            "{id} {o}: peak {peak} MB is over the {CAP_MB} MB cap"
        );
        // The picture came out the right size.
        let f = &r.outputs[0];
        if id == "image.resize" {
            let img = decode_srgb(f);
            assert_eq!(img.width, 3000);
            assert_eq!(img.height, 625);
        }
    }
}

#[test]
fn a_format_that_cannot_hold_the_size_is_named_before_encoding() {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // 120 megapixels at 24000 px wide: WebP stops at 16383 per side (IPC-7), JPEG at 65535.
    let s = Scratch::new("mem-limits");
    let src = s.path().join("pano.jpg");
    make_panorama(&src);
    let out = Scratch::new("mem-limits-out");
    let e = run("image.convert", &src, json!({"to": "webp"}), out.path())
        .err()
        .expect("WebP cannot hold 24000 px");
    assert!(e.contains("JPG or PNG") || e.contains("WebP"), "{e}");
    assert!(
        out.files().is_empty(),
        "a refused job left files: {:?}",
        out.files()
    );
}
