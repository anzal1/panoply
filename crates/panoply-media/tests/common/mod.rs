//! Helpers shared by the integration tests: the corpus, a scratch folder, an engine, and the
//! checks that look at results with an independent reader (ffprobe and ffmpeg's decoder).

#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use panoply_media::engine::Engine;
use panoply_media::io::OpenIo;
use panoply_media::modules::{self, MediaModule};
use panoply_media::probe::{self, Profile};
use serde_json::Value;

/// A corpus file, or `None` (with a message) when it is not on disk. Fetch with
/// `python3 -I tests/corpus/fetch.py --kind video,audio`.
pub fn corpus(rel: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache")
        .join(rel);
    if p.is_file() {
        Some(p)
    } else {
        eprintln!("skipping: {} is not in the corpus cache", p.display());
        None
    }
}

/// Set up the helper engine from `PANOPLY_FFMPEG` or `PATH`. False (with a message) when there
/// is none, and the test should return.
pub fn engine() -> bool {
    let ok = Engine::use_path_programs();
    if !ok {
        eprintln!("skipping: no ffmpeg and ffprobe on PATH and PANOPLY_FFMPEG is not set");
    }
    ok
}

/// The engine in use.
pub fn the_engine() -> Arc<Engine> {
    Engine::get(&OpenIo::new()).expect("an engine is set up")
}

/// A temporary folder that is removed at the end.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new empty folder.
    pub fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!(
            "panoply-media-test-{tag}-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside it.
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// The module with this id.
pub fn module(id: &str) -> MediaModule {
    modules::modules()
        .into_iter()
        .find(|m| m.manifest.module.id == id)
        .unwrap_or_else(|| panic!("{id} is not a media module"))
}

/// Run a module on one file and return the files written and the report lines.
pub fn run(
    id: &str,
    input: &Path,
    options: Value,
    out_dir: &Path,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    run_many(id, &[input.to_path_buf()], options, out_dir)
}

/// Run a module on several files.
pub fn run_many(
    id: &str,
    inputs: &[PathBuf],
    options: Value,
    out_dir: &Path,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let io = OpenIo::new().with_out_dir(out_dir.to_path_buf());
    let out = module(id)
        .run(&io, inputs, &options)
        .map_err(|e| e.to_string())?;
    Ok((out, io.notes()))
}

/// Like [`run`], but the job must succeed.
pub fn run_ok(
    id: &str,
    input: &Path,
    options: Value,
    out_dir: &Path,
) -> (Vec<PathBuf>, Vec<String>) {
    run(id, input, options, out_dir).unwrap_or_else(|e| panic!("{id} failed: {e}"))
}

/// Probe a file with the engine's ffprobe.
pub fn probe(path: &Path) -> Profile {
    probe::probe(&the_engine(), &OpenIo::new(), path)
        .unwrap_or_else(|e| panic!("could not probe {}: {e}", path.display()))
}

/// Run ffmpeg and return its stderr and whether it succeeded.
pub fn ffmpeg(args: &[&str]) -> (bool, String) {
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner"])
        .args(args.iter().map(OsString::from))
        .output()
        .expect("ffmpeg starts");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Decode every stream of a file to nothing and return the error lines. An empty list means it
/// plays start to end.
pub fn decode_errors(path: &Path) -> Vec<String> {
    let (_, err) = ffmpeg(&[
        "-v",
        "error",
        "-i",
        &path.to_string_lossy(),
        "-f",
        "null",
        "-",
    ]);
    err.lines()
        .map(str::to_string)
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// A list of hashes that prints short when two lists differ (a failed comparison of a whole
/// stream would otherwise print thousands of hashes).
#[derive(Clone, PartialEq)]
pub struct Hashes(pub Vec<String>);

impl std::fmt::Debug for Hashes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head: Vec<&str> = self
            .0
            .iter()
            .take(3)
            .map(|h| &h[..h.len().min(8)])
            .collect();
        write!(f, "{} hashes starting {head:?}", self.0.len())
    }
}

impl std::ops::Deref for Hashes {
    type Target = Vec<String>;
    fn deref(&self) -> &Vec<String> {
        &self.0
    }
}

impl PartialEq<Vec<String>> for Hashes {
    fn eq(&self, other: &Vec<String>) -> bool {
        &self.0 == other
    }
}

/// The MD5 of every decoded frame of stream `v:0` or `a:0` (`spec`), one string per frame.
pub fn frame_hashes(path: &Path, spec: &str) -> Hashes {
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args(["-map", spec, "-f", "framemd5", "-"])
        .output()
        .expect("ffmpeg starts");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.rsplit(',').next().unwrap_or("").trim().to_string())
        .collect::<Vec<_>>()
        .into()
}

impl From<Vec<String>> for Hashes {
    fn from(v: Vec<String>) -> Self {
        Hashes(v)
    }
}

/// The MD5 of the data of every packet of a stream (bit-identical copies have identical hashes).
/// `spec` is like `0:v:0`; read with ffprobe so only the packet bytes count, not timestamps or
/// side data.
pub fn packet_hashes(path: &Path, spec: &str) -> Hashes {
    let e = the_engine();
    let sel = spec.strip_prefix("0:").unwrap_or(spec);
    let out = Command::new(&e.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            sel,
            "-show_entries",
            "packet=data_hash",
            "-show_data_hash",
            "MD5",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("ffprobe starts");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().trim_end_matches(',').to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .into()
}

/// Files in a folder, sorted, as names.
pub fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// True when the engine has this encoder (fixtures are made with libx264 and libx265 where the
/// engine has them; a test that needs one skips otherwise).
pub fn has_encoder(name: &str) -> bool {
    let ok = the_engine().has_encoder(name);
    if !ok {
        eprintln!("skipping: the engine has no {name} encoder");
    }
    ok
}

/// True when a hardware encoder for this codec works here.
pub fn has_hw(codec: panoply_media::encoders::VCodec) -> bool {
    let ok = panoply_media::encoders::pick(&the_engine(), codec, true).is_some();
    if !ok {
        eprintln!("skipping: no hardware {} encoder here", codec.plain());
    }
    ok
}

/// Make a short test clip with a moving picture and a tone. `video_args` are the encoder
/// arguments. Returns the path.
pub fn make_clip(dir: &Path, name: &str, seconds: u32, fps: u32, video_args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("testsrc2=size=640x360:rate={fps}:duration={seconds}"),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("sine=frequency=440:sample_rate=48000:duration={seconds}"),
    ];
    args.extend(video_args.iter().map(|s| s.to_string()));
    args.extend(["-c:a", "aac", "-b:a", "96k", "-shortest"].map(String::from));
    args.push(out.to_string_lossy().into_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (ok, err) = ffmpeg(&refs);
    assert!(ok, "could not make the test clip: {err}");
    out
}

/// Mean SSIM of `out` against the same stretch of `src` (starting at `start` seconds),
/// frame for frame, and the lowest per-frame value.
pub fn ssim_against(src: &Path, start: f64, out: &Path) -> (f64, f64) {
    let e = the_engine();
    let o = Command::new(&e.ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-v",
            "info",
            "-ss",
            &format!("{start:.6}"),
            "-i",
        ])
        .arg(src)
        .arg("-i")
        .arg(out)
        .args([
            "-lavfi",
            "[0:v][1:v]scale2ref[a][b];[a][b]ssim=stats_file=-:shortest=1:repeatlast=0",
            "-f",
            "null",
            "-",
        ])
        .output()
        .expect("ffmpeg starts");
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    let mut vals: Vec<f64> = Vec::new();
    for l in text.lines() {
        if let Some(i) = l.find("All:")
            && let Some(v) = l[i + 4..].split_whitespace().next()
            && let Ok(x) = v.parse()
        {
            vals.push(x);
        }
    }
    assert!(
        !vals.is_empty(),
        "no SSIM lines: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    (mean, vals.iter().cloned().fold(1.0, f64::min))
}
