//! Where the vision files live: the engines folder (`PANOPLY_ENGINES_DIR`, else the user's cache
//! folder), the same one the PDF, media, OCR and ID engines use, so a runtime fetched by one is
//! found by the others.

use std::path::PathBuf;

/// The folder engines are cached in.
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

/// The folder the models are cached in, one sub-folder per model and pinned version.
pub fn models_dir() -> PathBuf {
    engines_dir().join("models")
}

/// Scratch space for compiled-model caches (CoreML), safe to delete at any time.
pub fn scratch_dir() -> PathBuf {
    engines_dir().join("vision-cache")
}
