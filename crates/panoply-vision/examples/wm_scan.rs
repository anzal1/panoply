//! Lists what watermark detection finds on the pictures it is given (one line a region).
//!
//! Usage: wm_scan FILE...   Every file is read the way the module reads it (same size cap).
//! A clean picture that prints a region is a false positive.
#![allow(clippy::chunks_exact_to_as_chunks)]

use std::path::PathBuf;

use panoply_image::ops::FsIo;
use panoply_image::ops::common::Animation;
use panoply_vision::watermark::{Search, detect_with, place_words};
use panoply_vision::work::{MAX_WORK_PIXELS, load_source};

fn main() {
    let io = FsIo::new();
    for p in std::env::args().skip(1).map(PathBuf::from) {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let src = match load_source(&io, &p, Animation::First, MAX_WORK_PIXELS) {
            Ok(s) => s,
            Err(e) => {
                println!("{name}\tunreadable: {e}");
                continue;
            }
        };
        let faces = panoply_vision::faces::find(&src.rgb, src.w, src.h);
        let d = detect_with(&src.rgb, src.w, src.h, Search::Anywhere, &faces);
        if d.regions.is_empty() {
            println!("{name}\tnone ({} spared, {} faces)", d.spared, faces.len());
        }
        for r in &d.regions {
            println!(
                "{name}\t{:?} score {:.2} at {} rect {},{} {}x{} of {}x{}",
                r.kind,
                r.score,
                place_words(&r.rect, src.w, src.h),
                r.rect.x,
                r.rect.y,
                r.rect.w,
                r.rect.h,
                src.w,
                src.h
            );
        }
    }
}
