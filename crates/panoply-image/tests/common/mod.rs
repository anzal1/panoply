//! Shared helpers for the corpus tests: find a corpus file, run a module into a scratch folder,
//! decode what came out.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use panoply_image::decode::{self, Input, OpenOpts};
use panoply_image::modules::{ImageModule, modules};
use panoply_image::ops::FsIo;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image, quiet};
use panoply_image::types::{Image, Samples};
use serde_json::{Value, json};

pub const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/cache/image"
);

/// The corpus file `rel`, or `None` (the test then skips, with a line saying so) when the corpus
/// has not been fetched (set `PANOPLY_REQUIRE_CORPUS=1`, as CI does, to fail instead). Fetch with `python3 -I tests/corpus/fetch.py --kind image`, then
/// `python3 tests/corpus/generate.py --kind image`.
pub fn corpus(rel: &str) -> Option<PathBuf> {
    let p = Path::new(CORPUS).join(rel);
    if p.is_file() {
        Some(p)
    } else if std::env::var_os("PANOPLY_REQUIRE_CORPUS").is_some() {
        panic!(
            "{rel} is not in the corpus and PANOPLY_REQUIRE_CORPUS is set (run tests/corpus/fetch.py and generate.py)"
        );
    } else {
        eprintln!(
            "skipping: {rel} is not in the corpus (run tests/corpus/fetch.py and generate.py)"
        );
        None
    }
}

/// A fresh scratch folder that is removed when dropped.
pub struct Scratch(pub PathBuf);

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!(
            "panoply-image-{tag}-{}-{}",
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn module(id: &str) -> ImageModule {
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

/// Run module `id` on `inputs`, writing into `out`.
pub fn run_many(id: &str, inputs: &[PathBuf], options: Value, out: &Path) -> Result<Ran, String> {
    let io = FsIo::in_dir(out);
    let outputs = module(id)
        .run(&io, inputs, &options)
        .map_err(|e| e.to_string())?;
    Ok(Ran {
        outputs,
        notes: io.take_notes(),
    })
}

pub fn run(id: &str, input: &Path, options: Value, out: &Path) -> Result<Ran, String> {
    run_many(id, &[input.to_path_buf()], options, out)
}

/// Run and expect success with exactly one output.
pub fn run_ok(id: &str, input: &Path, options: Value, out: &Path) -> (PathBuf, Ran) {
    let r =
        run(id, input, options, out).unwrap_or_else(|e| panic!("{id} on {}: {e}", input.display()));
    assert_eq!(
        r.outputs.len(),
        1,
        "{id} on {}: {:?}",
        input.display(),
        r.notes
    );
    (r.outputs[0].clone(), r)
}

/// Decode a file the way the pipeline does for a lossless job: upright, 16 bit kept, color kept.
pub fn decode_keep(path: &Path) -> Image {
    decode_with(path, panoply_image::color::OutSpace::Keep)
}

/// Decode to sRGB.
pub fn decode_srgb(path: &Path) -> Image {
    decode_with(path, panoply_image::color::OutSpace::Srgb)
}

fn decode_with(path: &Path, space: panoply_image::color::OutSpace) -> Image {
    let f = std::fs::File::open(path).unwrap();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_string);
    let opts = OpenOpts {
        first_frame: true,
        keep_color: space == panoply_image::color::OutSpace::Keep,
        ..Default::default()
    };
    let mut r = decode::open(Input::File(f), ext.as_deref(), &opts)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    decode_to_image(
        &mut *r,
        &ImagePlan {
            out_space: space,
            ..Default::default()
        },
        &ctl,
    )
    .unwrap()
    .0
}

/// The 8 bit RGB value at `(x, y)`.
pub fn px(img: &Image, x: u32, y: u32) -> [u8; 3] {
    let ch = img.layout.channels();
    let i = (y as usize * img.width as usize + x as usize) * ch;
    let at = |k: usize| i + if ch >= 3 { k } else { 0 };
    let get = |k: usize| match &img.samples {
        Samples::U8(v) => v[at(k)],
        Samples::U16(v) => ((v[at(k)] as u32 * 255 + 32767) / 65535) as u8,
    };
    [get(0), get(1), get(2)]
}

pub fn file_bytes(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

/// Options object from `json!`.
pub fn opts(v: Value) -> Value {
    v
}

pub fn none() -> Value {
    json!({})
}

/// Write a PNG built from `rgba` (8 bit RGBA) for tests that need a picture with known pixels.
pub fn write_rgba_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}
