//! The models and engines Panoply may download, gathered from every crate that has some.
//!
//! This is the list behind `panoply models` and the app's Modules page: what each item is, how
//! big it is, whether it is on this computer, whether the person allowed it, and a way to remove
//! it. The consent gate itself is in [`panoply_core::assets`]; every download site calls it.

use std::path::PathBuf;

use panoply_core::assets::{Entry, merge_entries};

pub use panoply_core::assets::{AssetInfo, AssetKind, Status};

/// Every downloadable item on this platform, in a stable order. Items that several crates cache
/// (the ONNX Runtime) appear once.
pub fn catalog() -> Vec<Entry> {
    merge_entries(
        std::iter::once(panoply_pdf::pdfium::entry())
            .chain(panoply_media::engine::assets())
            .chain(panoply_vision::catalog::assets())
            .chain(panoply_ocr::models::assets())
            .chain(panoply_id::runtime::assets()),
    )
}

/// One item by id. `runtime` is accepted for `onnxruntime`, the name older versions used.
pub fn find(id: &str) -> Option<Entry> {
    let id = if id == "runtime" { "onnxruntime" } else { id };
    catalog().into_iter().find(|e| e.info.id == id)
}

/// Download one item now, so a later job starts at once. The consent gate runs first, exactly
/// as it does inside a job. Returns where the item is.
pub fn fetch(id: &str, notice: &mut dyn FnMut(&str)) -> Result<PathBuf, String> {
    let id = if id == "runtime" { "onnxruntime" } else { id };
    let plain = |e: &dyn std::fmt::Display| e.to_string();
    match id {
        "pdfium" => panoply_pdf::pdfium::ensure_library().map_err(|e| plain(&e)),
        "ffmpeg" => {
            let io = panoply_media::io::OpenIo::new();
            panoply_media::engine::Engine::get(&io)
                .map(|e| e.ffmpeg.clone())
                .map_err(|e| plain(&e))
        }
        "onnxruntime" | "isnet" | "migan" | "esrgan" => {
            let io = panoply_vision::ConsoleIo::new();
            let vid = if id == "onnxruntime" { "runtime" } else { id };
            panoply_vision::catalog::fetch(&io, vid).map_err(|e| plain(&e))
        }
        "yunet" => panoply_id::runtime::ensure_model().map_err(|e| plain(&e)),
        _ => {
            let asset = [
                &panoply_ocr::models::DET,
                &panoply_ocr::models::REC_EN,
                &panoply_ocr::models::REC_LATIN,
                &panoply_ocr::models::REC_DEVANAGARI,
                &panoply_ocr::models::TESSDATA_BEN,
                &panoply_ocr::models::TESSDATA_ENG,
                &panoply_ocr::models::TESSDATA_HIN,
            ]
            .into_iter()
            .find(|a| panoply_ocr::models::info(a).id == id)
            .ok_or_else(|| {
                let ids: Vec<String> = catalog().iter().map(|e| e.info.id.clone()).collect();
                format!(
                    "\"{id}\" is not something Panoply downloads. Try one of: {}.",
                    ids.join(", ")
                )
            })?;
            let mut fetch = panoply_ocr::Fetch::new(notice);
            panoply_ocr::models::ensure(asset, &mut fetch).map_err(|e| plain(&e))
        }
    }
}
