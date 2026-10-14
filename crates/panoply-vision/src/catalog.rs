//! A listing of what the image AI modules download, for `panoply models` and the app's "what is
//! stored on this computer" screen: size shown before anything is fetched (FLOWS.md budgets).

use std::path::PathBuf;

use panoply_image::error::{ImageError, Result};

use crate::VisionIo;
use crate::fetch::mb;
use crate::models::{self, Model};
use crate::runtime;

/// One downloadable item.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The id used by `panoply models fetch`.
    pub id: &'static str,
    /// What it is.
    pub name: &'static str,
    /// Download size in bytes.
    pub size: u64,
    /// SPDX license of the item.
    pub license: &'static str,
    /// Where it comes from.
    pub source: String,
    /// The modules that use it.
    pub used_by: &'static [&'static str],
    /// Where it is on disk, when it is.
    pub cached: Option<PathBuf>,
}

impl Entry {
    /// One line for a listing: id, size, license, state.
    pub fn line(&self) -> String {
        let state = match &self.cached {
            Some(p) => format!("on this computer ({})", p.display()),
            None => format!("not downloaded, {} when needed", mb(self.size)),
        };
        format!(
            "{:<22} {:>8}  {:<12} {}\n{:<22} {}\n{:<22} used by {}; from {}",
            self.id,
            mb(self.size),
            self.license,
            state,
            "",
            self.name,
            "",
            self.used_by.join(", "),
            self.source
        )
    }
}

fn model_entry(m: &'static Model, used_by: &'static [&'static str], short: &'static str) -> Entry {
    Entry {
        id: short,
        name: m.name,
        size: m.size,
        license: m.license,
        source: m.source.to_string(),
        used_by,
        cached: models::cached_path(m),
    }
}

/// Everything the modules may download: the ONNX Runtime (once, shared) and the three models.
pub fn entries() -> Vec<Entry> {
    let mut v = vec![Entry {
        id: "runtime",
        name: "the ONNX Runtime library",
        size: runtime::pinned_size().unwrap_or(0),
        license: "MIT",
        source: format!(
            "github.com/microsoft/onnxruntime release {}",
            runtime::ORT_VERSION
        ),
        used_by: &[
            "image.remove_background",
            "image.erase",
            "image.remove_watermark",
            "image.upscale",
        ],
        cached: runtime::runtime_path(),
    }];
    v.push(model_entry(
        &models::ISNET,
        &["image.remove_background"],
        "isnet",
    ));
    v.push(model_entry(
        &models::MIGAN,
        &["image.erase", "image.remove_watermark"],
        "migan",
    ));
    v.push(model_entry(&models::ESRGAN, &["image.upscale"], "esrgan"));
    v
}

/// Downloads one item now (`runtime`, `isnet`, `migan`, `esrgan`), so a later job starts at once.
pub fn fetch(io: &dyn VisionIo, id: &str) -> Result<PathBuf> {
    match id {
        "runtime" => runtime::ensure_runtime(io),
        "isnet" => models::ensure(io, &models::ISNET),
        "migan" => models::ensure(io, &models::MIGAN),
        "esrgan" => models::ensure(io, &models::ESRGAN),
        other => Err(ImageError::BadOption(format!(
            "\"{other}\" is not something Panoply downloads. Try one of: runtime, isnet, migan, esrgan."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_has_the_runtime_and_three_models_with_sizes() {
        let e = entries();
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].id, "runtime");
        for x in &e[1..] {
            assert!(x.size > 1_000_000);
            assert!(x.line().contains("MB"));
        }
        assert!(
            fetch(&panoply_image::ops::FsIo::new(), "nonsense")
                .unwrap_err()
                .to_string()
                .contains("runtime, isnet")
        );
    }
}
