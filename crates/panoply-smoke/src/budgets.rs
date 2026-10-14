//! The budgets a case is judged on, and the documented exceptions.
//!
//! FLOWS.md gives every module the same table. The matrix can measure two of its rows on a single
//! job: peak memory per job (under 300 MB for documents and images, constant for video and audio,
//! so the same 300 MB on the small corpus files) and the first preview of an instant transform
//! (under 100 ms for a typical file). A module may carry an exception, but only one that is
//! written down: in DECISIONS.md ("Exceptions to the budgets") and on the Budgets line of its spec.
//! Each exception below names where.

use panoply_core::budgets::{FIRST_PREVIEW_MS, PEAK_MEM_DOC_MB};

/// A files this size or smaller counts as a typical file for the first preview row, bytes.
pub const TYPICAL_BYTES: u64 = 10_000_000;

/// A documented budget exception.
#[derive(Debug, Clone, Copy)]
pub struct Exception {
    /// Module id.
    pub module: &'static str,
    /// Peak memory cap in MB, when the exception raises it.
    pub peak_mb: Option<u64>,
    /// True when the first preview row does not apply to the module.
    pub no_first_preview: bool,
    /// Which input types the exception covers (sniffed extensions), empty for all.
    pub only_for: &'static [&'static str],
    /// Where it is written down.
    pub source: &'static str,
}

/// Every exception the matrix honours. Add a row only with a pointer to the spec that grants it.
pub const EXCEPTIONS: &[Exception] = &[
    Exception {
        module: "image.remove_background",
        peak_mb: Some(2000),
        no_first_preview: true,
        only_for: &[],
        source: "id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP",
    },
    Exception {
        module: "image.erase",
        peak_mb: Some(600),
        no_first_preview: true,
        only_for: &[],
        source: "id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s",
    },
    Exception {
        module: "image.upscale",
        peak_mb: None,
        no_first_preview: true,
        only_for: &[],
        source: "id-and-image.md 3.11 Budgets: time and model size, preview tile in seconds",
    },
    Exception {
        module: "image.convert",
        peak_mb: Some(400),
        no_first_preview: false,
        only_for: &["dng", "raw", "cr2", "nef", "arw"],
        source: "id-and-image.md 3.1 Budgets: RAW development is about 8 bytes per pixel, 45 MP is about 360 MB",
    },
];

/// The exception for a module and input types, if there is one.
pub fn exception(module: &str, exts: &[String]) -> Option<&'static Exception> {
    EXCEPTIONS.iter().find(|e| {
        e.module == module
            && (e.only_for.is_empty() || exts.iter().any(|x| e.only_for.contains(&x.as_str())))
    })
}

/// The peak memory cap for a case in bytes, and the exception that raised it.
pub fn peak_cap(module: &str, exts: &[String]) -> (u64, Option<&'static Exception>) {
    match exception(module, exts) {
        Some(e) if e.peak_mb.is_some() => (e.peak_mb.unwrap_or(0) * 1_000_000, Some(e)),
        _ => (PEAK_MEM_DOC_MB * 1_000_000, None),
    }
}

/// True when the first preview row applies: an instant transform (pattern A) on a typical file,
/// with no exception.
pub fn first_preview_applies(module: &str, pattern: &str, bytes: u64, exts: &[String]) -> bool {
    pattern == "A"
        && bytes <= TYPICAL_BYTES
        && !exception(module, exts).is_some_and(|e| e.no_first_preview)
}

/// The first preview limit in milliseconds.
pub fn first_preview_ms() -> f64 {
    FIRST_PREVIEW_MS as f64
}
