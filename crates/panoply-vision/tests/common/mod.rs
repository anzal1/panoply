//! Shared helpers for the vision tests: corpus files, scratch folders, running a module,
//! decoding what came out, and synthetic pictures.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use panoply_image::format::Format;
use panoply_image::ops::FsIo;
use panoply_image::ops::common::{Animation, EncodeSpec, encode_bytes};
use panoply_vision::models::{self, Model};
use panoply_vision::modules::{VisionModule, modules};
use panoply_vision::work::{image_from, load_source};
use serde_json::Value;

pub const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/cache/image"
);

/// The corpus file `rel`, or `None` (the test then skips, with a line saying so) when the corpus
/// has not been fetched. `PANOPLY_REQUIRE_CORPUS=1` (as CI sets it) turns that into a failure.
pub fn corpus(rel: &str) -> Option<PathBuf> {
    let p = Path::new(CORPUS).join(rel);
    if p.is_file() {
        Some(p)
    } else if std::env::var_os("PANOPLY_REQUIRE_CORPUS").is_some() {
        panic!("{rel} is not in the corpus and PANOPLY_REQUIRE_CORPUS is set");
    } else {
        eprintln!("skipping: {rel} is not in the corpus (run tests/corpus/fetch.py)");
        None
    }
}

/// A HEIC corpus file, or `None` where this system cannot decode HEIC. HEIC decodes through the
/// operating system and only macOS has that wired; elsewhere every module refuses it with a plain
/// message (covered in the panoply-image corpus tests), so a vision test that needs the pixels of
/// a HEIC has nothing to check there.
pub fn heic_corpus(rel: &str) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        corpus(rel)
    } else {
        eprintln!("skipping: {rel} needs HEIC decoding, which is macOS only for now");
        None
    }
}

/// True when this model can be used now: it is on disk, or the network may fetch it. A run
/// with `PANOPLY_OFFLINE=1` and no cached model skips the test instead of failing it.
pub fn have(model: &'static Model) -> bool {
    if models::cached_path(model).is_some() || std::env::var_os("PANOPLY_OFFLINE").is_none() {
        true
    } else {
        eprintln!(
            "skipping: {} is not downloaded and PANOPLY_OFFLINE is set",
            model.id
        );
        false
    }
}

/// A fresh scratch folder that is removed when dropped.
pub struct Scratch(pub PathBuf);

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!(
            "panoply-vision-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Files in the folder, sorted.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.0)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    }

    /// Copy a file in and return the copy.
    pub fn copy_in(&self, from: &Path) -> PathBuf {
        let to = self.0.join(from.file_name().unwrap());
        std::fs::copy(from, &to).unwrap();
        to
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn module(id: &str) -> VisionModule {
    modules()
        .into_iter()
        .find(|m| m.manifest.module.id == id)
        .unwrap_or_else(|| panic!("no module {id}"))
}

/// What a run produced.
pub struct Ran {
    pub outputs: Vec<PathBuf>,
    pub notes: Vec<String>,
}

impl Ran {
    pub fn notes_text(&self) -> String {
        self.notes.join("\n")
    }
}

/// Run module `id` on `inputs`, writing next to them.
pub fn run_many(id: &str, inputs: &[PathBuf], options: Value) -> Result<Ran, String> {
    let io = FsIo::new();
    let outputs = module(id)
        .run(&io, inputs, &options)
        .map_err(|e| e.to_string())?;
    Ok(Ran {
        outputs,
        notes: io.take_notes(),
    })
}

/// Run and expect success with exactly one output.
pub fn run_ok(id: &str, input: &Path, options: Value) -> (PathBuf, Ran) {
    let r = run_many(id, &[input.to_path_buf()], options)
        .unwrap_or_else(|e| panic!("{id} on {}: {e}", input.display()));
    assert_eq!(r.outputs.len(), 1, "{id}: {:?}", r.outputs);
    (r.outputs[0].clone(), r)
}

/// A decoded picture: upright 8 bit RGB and the alpha plane when it has one.
pub struct Pic {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

impl Pic {
    pub fn px(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    pub fn a(&self, x: usize, y: usize) -> u8 {
        self.alpha.as_ref().map_or(255, |a| a[y * self.w + x])
    }
}

pub fn decode(path: &Path) -> Pic {
    let s = load_source(&FsIo::new(), path, Animation::First, usize::MAX / 8)
        .unwrap_or_else(|e| panic!("decoding {}: {e}", path.display()));
    Pic {
        w: s.w,
        h: s.h,
        rgb: s.rgb,
        alpha: s.alpha,
    }
}

/// A smooth gradient picture with a little deterministic texture, as a PNG file.
pub fn gradient_png(path: &Path, w: usize, h: usize) -> Pic {
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            let n = ((x * 7 + y * 13) % 11) as f32;
            rgb.extend_from_slice(&[
                (60.0 + 90.0 * t + n) as u8,
                (90.0 + 60.0 * t + n) as u8,
                (120.0 - 40.0 * t + n) as u8,
            ]);
        }
    }
    let img = image_from(w, h, &rgb, None);
    let bytes = encode_bytes(&img, &EncodeSpec::new(Format::Png)).unwrap();
    std::fs::write(path, bytes).unwrap();
    Pic {
        w,
        h,
        rgb,
        alpha: None,
    }
}

/// Writes RGB (and alpha) pixels as a PNG.
pub fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8], alpha: Option<&[u8]>) {
    let img = image_from(w, h, rgb, alpha);
    std::fs::write(
        path,
        encode_bytes(&img, &EncodeSpec::new(Format::Png)).unwrap(),
    )
    .unwrap();
}

/// Paints a white translucent block-letter stamp: `n` letters, 8 x 14 pixels each, 13 apart.
pub fn stamp(rgb: &mut [u8], w: usize, x0: usize, y0: usize, n: usize, alpha: f32) {
    for k in 0..n {
        for y in 0..14 {
            for x in 0..8 {
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
