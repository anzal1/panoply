//! Panoply's image AI modules: remove background, erase, remove watermark and upscale.
//!
//! The crate has three layers.
//!
//! * A small **model manager** ([`models`]): each model is a pinned file (URL, SHA-256, size,
//!   license). The first job that needs one downloads it from the official source into the
//!   per-user cache, says how big it is before the download starts, checks the digest and only
//!   then uses it. Nothing is downloaded at build time and nothing ships in the binary.
//! * **Inference** ([`runtime`]): ONNX Runtime through the `ort` crate, loaded at run time from a
//!   pinned official release, with the CoreML provider on macOS, DirectML on Windows where the
//!   runtime build has it, and the CPU everywhere else. A model is loaded for a job and dropped
//!   after 60 seconds without use (FLOWS.md budgets).
//! * The **modules** ([`modules`]): `image.remove_background`, `image.erase`,
//!   `image.remove_watermark` and `image.upscale`, registered the same way as the `panoply-image`
//!   modules. On macOS, background removal uses the operating system's subject finder (Apple
//!   Vision) first and falls back to the IS-Net model; every other job runs a small model.
//!
//! Pixel work (mask refinement, tiling, blending) is in [`px`], [`matte`] and [`tile`]; every
//! large buffer is processed in tiles or strips so a job stays under the 300 MB image budget.
#![warn(missing_docs)]
// Pixel loops walk runtime-sized rows in fixed-size groups (3, 4 and 2 samples); the plain
// `chunks_exact` form reads the same and is kept in hot loops and test code.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod cache;
pub mod catalog;
pub mod cutout;
pub mod faces;
pub mod fetch;
pub mod inpaint;
pub mod matte;
pub mod models;
pub mod modules;
pub mod px;
pub mod runtime;
pub mod tile;
pub mod upscale;
pub mod watermark;
pub mod work;

use panoply_image::ops::{FsIo, Io};

/// What a vision job needs from its host on top of the image [`Io`]: whether the network may be
/// used (the host grants it from the manifest and the user's approval).
pub trait VisionIo: Io {
    /// True when a model or the runtime may be downloaded for this job.
    fn network_allowed(&self) -> bool;
}

impl VisionIo for FsIo {
    /// A plain file system host allows downloads unless `PANOPLY_OFFLINE` is set.
    fn network_allowed(&self) -> bool {
        std::env::var_os("PANOPLY_OFFLINE").is_none()
    }
}

/// A console host for downloads started by hand (`panoply models fetch`): progress goes to
/// standard error, card lines to standard output.
pub struct ConsoleIo {
    inner: FsIo,
    last: std::sync::Mutex<Option<(String, u32)>>,
}

impl ConsoleIo {
    /// A new console host.
    pub fn new() -> Self {
        Self {
            inner: FsIo::new(),
            last: std::sync::Mutex::new(None),
        }
    }
}

impl Default for ConsoleIo {
    fn default() -> Self {
        Self::new()
    }
}

impl Io for ConsoleIo {
    fn open_read(&self, path: &std::path::Path) -> panoply_image::error::Result<std::fs::File> {
        self.inner.open_read(path)
    }

    fn create_output(
        &self,
        input: &std::path::Path,
        name: &panoply_image::ops::NameSpec,
        ext: &str,
    ) -> panoply_image::error::Result<(std::path::PathBuf, std::fs::File)> {
        self.inner.create_output(input, name, ext)
    }

    fn cancel(&self) -> &panoply_core::CancelToken {
        self.inner.cancel()
    }

    fn progress(&self, fraction: f32, label: &str) {
        let pct = (fraction * 100.0) as u32;
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        // Print when the label changes and then every 10 percent.
        let show = match last.as_ref() {
            Some((l, p)) => l != label || pct >= p + 10,
            None => true,
        };
        if show {
            eprintln!("{pct:>3}%  {label}");
            *last = Some((label.to_string(), pct));
        }
    }

    fn note(&self, line: String) {
        println!("{line}");
    }
}

impl VisionIo for ConsoleIo {
    fn network_allowed(&self) -> bool {
        true
    }
}

/// Wraps any [`Io`] and says the network is not allowed: the host of a job whose user has not
/// approved downloads. A model that is already on disk still loads; one that is missing fails
/// with a plain sentence that names the download.
pub struct NoNetwork<T: Io>(pub T);

impl<T: Io> Io for NoNetwork<T> {
    fn open_read(&self, path: &std::path::Path) -> panoply_image::error::Result<std::fs::File> {
        self.0.open_read(path)
    }

    fn create_output(
        &self,
        input: &std::path::Path,
        name: &panoply_image::ops::NameSpec,
        ext: &str,
    ) -> panoply_image::error::Result<(std::path::PathBuf, std::fs::File)> {
        self.0.create_output(input, name, ext)
    }

    fn cancel(&self) -> &panoply_core::CancelToken {
        self.0.cancel()
    }

    fn progress(&self, fraction: f32, label: &str) {
        self.0.progress(fraction, label);
    }

    fn note(&self, line: String) {
        self.0.note(line);
    }
}

impl<T: Io> VisionIo for NoNetwork<T> {
    fn network_allowed(&self) -> bool {
        false
    }
}
