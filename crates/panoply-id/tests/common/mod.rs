//! Shared helpers for the corpus tests: find a corpus file, run a module into a scratch folder,
//! read a picture back.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use panoply_id::modules::{IdModule, Op, modules};
use panoply_image::ops::FsIo;
use panoply_image::types::Image;
use serde_json::Value;

pub const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/cache/image"
);

/// The portraits in the corpus (see tests/corpus/image.toml, tag `face`).
pub const PORTRAITS: [&str; 7] = [
    "antonelli",
    "borman",
    "fisher",
    "hoshide",
    "jemison",
    "sunita",
    "thomas",
];

/// The corpus file `rel`, or `None` (the test then skips, with a line saying so) when the corpus
/// has not been fetched (set `PANOPLY_REQUIRE_CORPUS=1`, as CI does, to fail instead). Fetch with
/// `python3 -I tests/corpus/fetch.py --kind image`.
pub fn corpus(rel: &str) -> Option<PathBuf> {
    let p = Path::new(CORPUS).join(rel);
    if p.is_file() {
        Some(p)
    } else if std::env::var_os("PANOPLY_REQUIRE_CORPUS").is_some() {
        panic!(
            "{rel} is not in the corpus and PANOPLY_REQUIRE_CORPUS is set (run tests/corpus/fetch.py)"
        );
    } else {
        eprintln!("skipping: {rel} is not in the corpus (run tests/corpus/fetch.py --kind image)");
        None
    }
}

pub fn portrait(name: &str) -> Option<PathBuf> {
    corpus(&format!("faces/{name}.jpg"))
}

/// True when a face engine can run here: Apple Vision on macOS, else YuNet if its runtime and
/// model can be fetched (set `PANOPLY_REQUIRE_ENGINES=1` to fail instead of skipping).
pub fn engines_available() -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    let ok = panoply_id::runtime::ensure_onnxruntime().is_ok()
        && panoply_id::runtime::ensure_model().is_ok();
    if !ok && std::env::var_os("PANOPLY_REQUIRE_ENGINES").is_some() {
        panic!("the face engine could not be fetched and PANOPLY_REQUIRE_ENGINES is set");
    }
    if !ok {
        eprintln!("skipping: the YuNet engine is not available here");
    }
    ok
}

/// A fresh scratch folder that is removed when dropped.
pub struct Scratch(pub PathBuf);

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!(
            "panoply-id-{tag}-{}-{}",
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

pub fn module(op: Op) -> IdModule {
    modules().into_iter().find(|m| m.op == op).unwrap()
}

/// Run a module on `inputs` into `out` and return the outputs and the card lines.
pub fn run(
    op: Op,
    inputs: &[PathBuf],
    options: Value,
    out: &Path,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let io = FsIo::in_dir(out);
    let r = module(op)
        .run(&io, inputs, &options)
        .map_err(|e| e.to_string())?;
    Ok((r, io.take_notes()))
}

/// Decode a file to an upright 8 bit RGB picture.
pub fn read(path: &Path) -> Image {
    let io = FsIo::new();
    panoply_id::imageio::load_upright(&io, path, 100_000, Some([255, 255, 255]))
        .unwrap()
        .image
}
