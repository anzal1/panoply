//! Everything that differs per operating system, behind one small surface.
//!
//! macOS has the full set (the bar is a non-activating panel). Windows and Linux compile the same
//! surface with the portable GPUI calls; they are verified by `cargo check` and the Linux build,
//! not yet by hand (see README.md, "Windows and Linux").
#![allow(dead_code, unsafe_code)]

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
pub(crate) use other::*;

pub(crate) mod ico;
#[cfg(windows)]
pub(crate) mod tray;

#[cfg(all(unix, feature = "snapshot"))]
pub(crate) mod proc;

/// Milliseconds since the epoch, for latency probes.
pub(crate) fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

static TRASH_OVERRIDE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Send trashed files to this folder instead of the system Trash (headless runs and tests).
pub(crate) fn set_trash_dir(dir: std::path::PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = TRASH_OVERRIDE.set(dir);
}

/// Move a file to the Trash, or to the override folder when one is set.
pub(crate) fn trash(path: &std::path::Path) -> std::io::Result<()> {
    match TRASH_OVERRIDE.get() {
        Some(dir) => {
            let name = path
                .file_name()
                .ok_or_else(|| std::io::Error::other("no file name"))?;
            std::fs::rename(path, dir.join(name))
        }
        None => os_trash(path),
    }
}
