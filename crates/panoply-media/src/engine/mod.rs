//! The helper engine: an LGPL build of ffmpeg and ffprobe run as a separate process.
//!
//! Why a helper process and not a linked library (ADR 0002, spec section 0.1):
//!
//! - The license line is a process boundary. Panoply is MIT or Apache-2.0 and the workspace
//!   forbids `unsafe`, so nothing here links or loads libav*. The helper is a program the user
//!   runs through us, fetched on first use and checksum pinned, never committed.
//! - A damaged file can crash a demuxer. In a helper it takes down only the helper, and the
//!   host shows a plain error (ARCHITECTURE.md section 10, hostile files never crash the app).
//! - Cancel is a kill, memory is the helper's own streaming (flat for any duration), and the
//!   idle cost is zero because nothing is loaded between jobs.
//! - Hardware encoders are ffmpeg's wrappers over the OS APIs (VideoToolbox, Media Foundation,
//!   VA-API, plus NVENC, QSV and AMF), so the OS engine is used first without us writing FFI.
//!
//! Lookup order: `PANOPLY_FFMPEG` (a path, with ffprobe beside it or in `PANOPLY_FFPROBE`),
//! then the pinned build in the engines folder, fetched once when missing, then (only if the
//! fetch is impossible) an ffmpeg and ffprobe found on `PATH`.

mod fetch;
pub mod pins;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::{MediaError, Result};
use crate::io::Io;
use crate::proc;

/// Where the engine in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `PANOPLY_FFMPEG`.
    Env,
    /// The pinned LGPL build in the engines folder.
    Pinned,
    /// An ffmpeg found on `PATH`.
    System,
}

/// What `ffmpeg -version` says about the build.
#[derive(Debug, Clone, Default)]
pub struct BuildInfo {
    /// For example `8.0` or `N-117512-gabc`.
    pub version: String,
    /// The `configuration:` line.
    pub configuration: String,
}

impl BuildInfo {
    /// True when the build was configured with GPL or nonfree parts. Panoply never fetches such
    /// a build, and the code never asks one for x264, x265 or fdk-aac.
    pub fn is_gpl(&self) -> bool {
        self.configuration.contains("--enable-gpl")
            || self.configuration.contains("--enable-nonfree")
    }
}

/// The helper programs, and what they can do on this machine.
pub struct Engine {
    /// The ffmpeg program.
    pub ffmpeg: PathBuf,
    /// The ffprobe program.
    pub ffprobe: PathBuf,
    /// Where it came from.
    pub source: Source,
    /// A folder of shared libraries the helper needs (OpenH264), added to its search path.
    lib_dir: Option<PathBuf>,
    build: OnceLock<BuildInfo>,
    encoders: OnceLock<HashSet<String>>,
    filters: OnceLock<HashSet<String>>,
    hw: Mutex<Vec<(String, bool)>>,
}

static SHARED: Mutex<Option<Arc<Engine>>> = Mutex::new(None);

/// The folder engines are cached in: `PANOPLY_ENGINES_DIR`, else the user's cache folder.
pub fn engines_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PANOPLY_ENGINES_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home.unwrap_or_default()
            .join("Library/Caches/Panoply/engines")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Panoply/cache/engines")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".cache")))
            .unwrap_or_default()
            .join("panoply/engines")
    }
}

/// The downloadable parts of the video engine for this platform: empty where the engine is an
/// ffmpeg found on `PATH` (macOS today), one entry where a pinned build exists.
pub fn assets() -> Vec<panoply_core::assets::Entry> {
    fetch::entry().into_iter().collect()
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(exe(name)))
        .find(|p| p.is_file())
}

impl Engine {
    /// An engine around these two programs (tests, and the `PANOPLY_FFMPEG` override).
    pub fn from_paths(ffmpeg: PathBuf, ffprobe: PathBuf, source: Source) -> Self {
        Engine {
            ffmpeg,
            ffprobe,
            source,
            lib_dir: None,
            build: OnceLock::new(),
            encoders: OnceLock::new(),
            filters: OnceLock::new(),
            hw: Mutex::new(Vec::new()),
        }
    }

    /// The engine for this run: the one already found, or the first that can be found, fetched
    /// or (last) taken from `PATH`. `io` decides whether the one-time download is allowed.
    pub fn get(io: &dyn Io) -> Result<Arc<Engine>> {
        if let Some(e) = SHARED.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            return Ok(e);
        }
        let engine = Arc::new(Self::locate(io)?);
        *SHARED.lock().unwrap_or_else(|p| p.into_inner()) = Some(engine.clone());
        Ok(engine)
    }

    /// Use these programs for the rest of the process (tests, and tools that already know where
    /// ffmpeg is). Returns the engine now in use.
    pub fn use_programs(ffmpeg: PathBuf, ffprobe: PathBuf, source: Source) -> Arc<Engine> {
        let e = Arc::new(Engine::from_paths(ffmpeg, ffprobe, source));
        *SHARED.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.clone());
        e
    }

    /// Use the ffmpeg and ffprobe found on `PATH` when no engine is set up yet. Returns false
    /// when there is none.
    pub fn use_path_programs() -> bool {
        if SHARED.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
            return true;
        }
        let env = std::env::var_os("PANOPLY_FFMPEG").map(PathBuf::from);
        let (f, p) = match env {
            Some(f) => {
                let p = f
                    .parent()
                    .map(|d| d.join(exe("ffprobe")))
                    .unwrap_or_default();
                (Some(f), Some(p))
            }
            None => (find_on_path("ffmpeg"), find_on_path("ffprobe")),
        };
        match (f, p) {
            (Some(f), Some(p)) if f.is_file() && p.is_file() => {
                Engine::use_programs(f, p, Source::System);
                true
            }
            _ => false,
        }
    }

    fn locate(io: &dyn Io) -> Result<Engine> {
        if let Some(p) = std::env::var_os("PANOPLY_FFMPEG") {
            let ffmpeg = PathBuf::from(p);
            if !ffmpeg.is_file() {
                return Err(MediaError::Engine(format!(
                    "PANOPLY_FFMPEG points at {}, which is not a file. Fix the path or remove the setting.",
                    ffmpeg.display()
                )));
            }
            let ffprobe = std::env::var_os("PANOPLY_FFPROBE")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    ffmpeg
                        .parent()
                        .map(|d| d.join(exe("ffprobe")))
                        .unwrap_or_else(|| PathBuf::from(exe("ffprobe")))
                });
            if !ffprobe.is_file() {
                return Err(MediaError::Engine(format!(
                    "ffprobe was not found at {}. Put it next to ffmpeg or set PANOPLY_FFPROBE.",
                    ffprobe.display()
                )));
            }
            return Ok(Engine::from_paths(ffmpeg, ffprobe, Source::Env));
        }
        match fetch::ensure_pinned(io) {
            Ok(Some((ffmpeg, ffprobe, lib_dir))) => {
                let mut e = Engine::from_paths(ffmpeg, ffprobe, Source::Pinned);
                e.lib_dir = lib_dir;
                return Ok(e);
            }
            Ok(None) => {}
            Err(MediaError::Cancelled) => return Err(MediaError::Cancelled),
            Err(fetch_err) => {
                if let (Some(f), Some(p)) = (find_on_path("ffmpeg"), find_on_path("ffprobe")) {
                    return Ok(Engine::from_paths(f, p, Source::System));
                }
                return Err(fetch_err);
            }
        }
        // No pinned build exists for this platform: use one on PATH if there is one.
        if let (Some(f), Some(p)) = (find_on_path("ffmpeg"), find_on_path("ffprobe")) {
            return Ok(Engine::from_paths(f, p, Source::System));
        }
        Err(MediaError::Engine(format!(
            "Panoply has no prebuilt video engine for {} on {}, and no ffmpeg was found. Install ffmpeg, or set PANOPLY_FFMPEG to it.",
            std::env::consts::OS,
            std::env::consts::ARCH
        )))
    }

    /// Set the helper's environment: the folder of extra libraries, and nothing else leaks in.
    pub fn configure(&self, cmd: &mut Command) {
        if let Some(dir) = &self.lib_dir {
            let var = if cfg!(windows) {
                "PATH"
            } else if cfg!(target_os = "macos") {
                "DYLD_LIBRARY_PATH"
            } else {
                "LD_LIBRARY_PATH"
            };
            let mut paths = vec![dir.clone()];
            if let Some(old) = std::env::var_os(var) {
                paths.extend(std::env::split_paths(&old));
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env(var, joined);
            }
        }
    }

    /// Version and configuration of the build.
    pub fn build(&self) -> &BuildInfo {
        self.build.get_or_init(|| {
            let out = proc::capture(self, None, &self.ffmpeg, &[OsString::from("-version")]);
            let text = out
                .ok()
                .map(|f| String::from_utf8_lossy(&f.stdout).into_owned())
                .unwrap_or_default();
            let mut info = BuildInfo::default();
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("ffmpeg version ") {
                    info.version = v.split_whitespace().next().unwrap_or("").to_string();
                } else if let Some(c) = line.strip_prefix("configuration:") {
                    info.configuration = c.trim().to_string();
                }
            }
            info
        })
    }

    fn listing(&self, flag: &str) -> HashSet<String> {
        let out = proc::capture(
            self,
            None,
            &self.ffmpeg,
            &[OsString::from("-hide_banner"), OsString::from(flag)],
        );
        let text = out
            .ok()
            .map(|f| String::from_utf8_lossy(&f.stdout).into_owned())
            .unwrap_or_default();
        // Lines look like " V....D h264_videotoolbox    VideoToolbox H.264 Encoder" for encoders
        // and " TS lut3d    V->V    Adjust colors using a 3D LUT." for filters.
        text.lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let flags = it.next()?;
                let name = it.next()?;
                (flags.len() >= 2
                    && flags
                        .chars()
                        .all(|c| c.is_ascii_alphabetic() || c == '.' || c == '='))
                .then(|| name.to_string())
            })
            .collect()
    }

    /// True when the build has this encoder.
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders
            .get_or_init(|| self.listing("-encoders"))
            .contains(name)
    }

    /// True when the build has this filter.
    pub fn has_filter(&self, name: &str) -> bool {
        self.filters
            .get_or_init(|| self.listing("-filters"))
            .contains(name)
    }

    /// Run a tiny real encode with `encoder` to see that it works on this machine (a hardware
    /// encoder can be listed and still have no device). Cached for the life of the process.
    pub fn encoder_works(&self, encoder: &str, pre: &[&str], extra: &[&str]) -> bool {
        let key = format!("{encoder} {} {}", pre.join(" "), extra.join(" "));
        if let Some((_, ok)) = self
            .hw
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|(k, _)| *k == key)
        {
            return *ok;
        }
        let mut args: Vec<OsString> = ["-nostdin", "-hide_banner", "-loglevel", "error"]
            .iter()
            .map(OsString::from)
            .collect();
        args.extend(pre.iter().map(OsString::from));
        args.extend(
            ["-f", "lavfi", "-i", "color=c=gray:s=640x360:r=25:d=0.2"]
                .iter()
                .map(OsString::from),
        );
        args.extend(extra.iter().map(OsString::from));
        args.extend(["-c:v", encoder, "-frames:v", "2", "-f", "null", "-"].map(OsString::from));
        let ok = proc::capture(self, None, &self.ffmpeg, &args)
            .map(|f| f.ok)
            .unwrap_or(false);
        self.hw
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((key, ok));
        ok
    }

    /// One line for the result card about the engine in use.
    pub fn describe(&self) -> String {
        let b = self.build();
        let kind = match self.source {
            Source::Pinned => "Panoply's LGPL build",
            Source::Env => "the program set in PANOPLY_FFMPEG",
            Source::System => "the ffmpeg found on this computer",
        };
        format!("Engine: ffmpeg {} ({kind}).", b.version)
    }

    /// Where the pinned library folder is, when one is in use.
    pub fn lib_dir(&self) -> Option<&Path> {
        self.lib_dir.as_deref()
    }
}
