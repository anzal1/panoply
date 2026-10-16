//! The budgets a case is judged on, and the documented exceptions.
//!
//! FLOWS.md gives every module the same table. The matrix can measure two of its rows on a single
//! job: peak memory per job (under 300 MB for documents and images, constant for video and audio,
//! so the same 300 MB on the small corpus files) and the first preview of an instant transform
//! (under 100 ms for a typical file). A module may carry an exception, but only one that is
//! written down: in DECISIONS.md ("Exceptions to the budgets", and "Exceptions found by the beta
//! smoke matrix" for the ones measured on real files) and, for the first group, on the Budgets
//! line of its spec. Each exception below names where.

use panoply_core::budgets::{FIRST_PREVIEW_MS, PEAK_MEM_DOC_MB};

/// A files this size or smaller counts as a typical file for the first preview row, bytes.
pub const TYPICAL_BYTES: u64 = 10_000_000;

/// A documented budget exception.
#[derive(Debug, Clone, Copy)]
pub struct Exception {
    /// Module id, or `*` for every module.
    pub module: &'static str,
    /// Peak memory cap in MB, when the exception raises it.
    pub peak_mb: Option<u64>,
    /// True when the first preview row does not apply to the module.
    pub no_first_preview: bool,
    /// Which input types the exception covers (sniffed extensions), empty for all.
    pub only_for: &'static [&'static str],
    /// Which corpus files the exception covers, empty for all. A file the system decoder is
    /// expensive on is named here, so the same module on an ordinary file is still held to the cap.
    pub only_ids: &'static [&'static str],
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
        only_ids: &[],
        source: "id-and-image.md 3.8 Budgets: 2 GB cap on CPU for the fallback model, result under 3 s for 12 MP",
    },
    Exception {
        module: "image.erase",
        peak_mb: Some(600),
        no_first_preview: true,
        only_for: &[],
        only_ids: &[],
        source: "id-and-image.md 3.14 Budgets: segmenter plus inpainter, preview under 2 s",
    },
    Exception {
        module: "image.upscale",
        peak_mb: None,
        no_first_preview: true,
        only_for: &[],
        only_ids: &[],
        source: "id-and-image.md 3.11 Budgets: time and model size, preview tile in seconds",
    },
    Exception {
        module: "image.convert",
        peak_mb: Some(400),
        no_first_preview: false,
        only_for: &["dng", "raw", "cr2", "nef", "arw"],
        only_ids: &[],
        source: "id-and-image.md 3.1 Budgets: RAW development is about 8 bytes per pixel, 45 MP is about 360 MB",
    },
    // Measured on the corpus by the beta smoke matrix (2026-10-14). Written down in DECISIONS.md,
    // "Exceptions found by the beta smoke matrix".
    Exception {
        module: "image.erase",
        peak_mb: Some(1250),
        no_first_preview: true,
        only_for: &[],
        only_ids: &[],
        source: "DECISIONS.md, smoke matrix exceptions: the fill model is about 575 MB resident and the picture adds about 7 bytes a pixel up to the 24 MP working size, 1025 MB at 31 MP, 1156 MB for a batch",
    },
    Exception {
        module: "image.remove_watermark",
        peak_mb: Some(950),
        no_first_preview: true,
        only_for: &[],
        only_ids: &[],
        source: "DECISIONS.md, smoke matrix exceptions: the same fill model as Erase, 786 MB on a camera RAW and 912 MB with a 31 MP picture in the batch",
    },
    Exception {
        module: "id.passport_photo",
        peak_mb: Some(450),
        no_first_preview: false,
        only_for: &[],
        only_ids: &[],
        source: "id-and-image.md 2.1 Budgets (the cutout carries the 3.8 exception) and DECISIONS.md, smoke matrix exceptions: Apple Vision's foreground mask, 348 MB measured",
    },
    Exception {
        module: "pdf.ocr",
        peak_mb: Some(650),
        no_first_preview: false,
        only_for: &[],
        only_ids: &[],
        source: "DECISIONS.md, smoke matrix exceptions: Apple Vision is 156 MB before the page is read, and a page with next to no text also starts the Hindi and Bengali fallback (ONNX Runtime and the PP-OCR models), 535 MB measured",
    },
    Exception {
        module: "image.upscale",
        peak_mb: Some(400),
        no_first_preview: true,
        only_for: &[],
        only_ids: &[],
        source: "DECISIONS.md, smoke matrix exceptions: the upscale model plus the allocator keeping its high-water mark between the pictures of a batch, 313 MB measured",
    },
    Exception {
        module: "*",
        peak_mb: Some(1800),
        no_first_preview: false,
        only_for: &[],
        only_ids: &["img-heic-rotated-cw90"],
        source: "DECISIONS.md, smoke matrix exceptions: a 33 MP 10-bit HEIF stored in 3520 x 1600 tiles, about 800 MB inside ImageIO whatever the module does (image-compress-results.md section 3)",
    },
    Exception {
        module: "*",
        peak_mb: Some(1000),
        no_first_preview: false,
        only_for: &[],
        only_ids: &["img-raw-canon-7d-sraw", "img-raw-pixel-7-pro-dng"],
        source: "DECISIONS.md, smoke matrix exceptions: camera RAW through ImageIO, which has no tiled RAW path (image-compress-results.md section 3; 3.1 already carries a RAW exception)",
    },
    Exception {
        module: "*",
        peak_mb: Some(450),
        no_first_preview: false,
        only_for: &[],
        only_ids: &["img-avif-gainmap-avif"],
        source: "DECISIONS.md, smoke matrix exceptions: an HDR AVIF costs ImageIO about 170 MB before any module touches it, even at 400 x 300",
    },
];

/// Every exception that applies to a module, its input types and its files.
pub fn exceptions<'a>(
    module: &'a str,
    exts: &'a [String],
    ids: &'a [String],
) -> impl Iterator<Item = &'static Exception> + 'a {
    EXCEPTIONS.iter().filter(move |e| {
        (e.module == "*" || e.module == module)
            && (e.only_for.is_empty() || exts.iter().any(|x| e.only_for.contains(&x.as_str())))
            && (e.only_ids.is_empty() || ids.iter().any(|i| e.only_ids.contains(&i.as_str())))
    })
}

/// The peak memory cap for a case in bytes, and the exception that raised it (the one that
/// allows the most).
pub fn peak_cap(
    module: &str,
    exts: &[String],
    ids: &[String],
) -> (u64, Option<&'static Exception>) {
    let best = exceptions(module, exts, ids)
        .filter(|e| e.peak_mb.is_some())
        .max_by_key(|e| e.peak_mb.unwrap_or(0));
    match best {
        Some(e) => (e.peak_mb.unwrap_or(0) * 1_000_000, Some(e)),
        None => (PEAK_MEM_DOC_MB * 1_000_000, None),
    }
}

/// True when the first preview row applies: an instant transform (pattern A) on a typical file,
/// with no exception.
pub fn first_preview_applies(
    module: &str,
    pattern: &str,
    bytes: u64,
    exts: &[String],
    ids: &[String],
) -> bool {
    pattern == "A"
        && bytes <= TYPICAL_BYTES
        && !exceptions(module, exts, ids).any(|e| e.no_first_preview)
}

/// The first preview limit in milliseconds.
pub fn first_preview_ms() -> f64 {
    FIRST_PREVIEW_MS as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_file_named_in_an_exception_is_the_only_file_it_covers() {
        let heif = v(&["heif"]);
        let (cap, why) = peak_cap("image.compress", &heif, &v(&["img-heic-rotated-cw90"]));
        assert_eq!(cap, 1_800_000_000);
        assert!(why.is_some());
        // The same module on another HEIC photo is held to the 300 MB budget.
        let (cap, why) = peak_cap("image.compress", &heif, &v(&["img-heic-other"]));
        assert_eq!(cap, PEAK_MEM_DOC_MB * 1_000_000);
        assert!(why.is_none());
    }

    #[test]
    fn the_largest_applicable_cap_wins() {
        let (cap, _) = peak_cap("image.erase", &v(&["heif"]), &v(&["img-heic-rotated-cw90"]));
        assert_eq!(cap, 1_800_000_000);
        let (cap, _) = peak_cap("image.erase", &v(&["jpg"]), &v(&["img-commons-wide-nasa"]));
        assert_eq!(cap, 1_250_000_000);
    }

    #[test]
    fn every_exception_says_where_it_is_written() {
        for e in EXCEPTIONS {
            assert!(
                e.source.contains("DECISIONS.md") || e.source.contains(".md"),
                "{}: {}",
                e.module,
                e.source
            );
        }
    }
}
