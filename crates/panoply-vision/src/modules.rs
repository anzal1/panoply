//! The four image AI modules: manifests plus the operation each one runs, registered the same way
//! as the `panoply-image` modules.

use std::path::PathBuf;

use panoply_image::error::Result;
use panoply_image::ops::options::Opts;
use panoply_manifest::Manifest;
use serde_json::Value;

use crate::VisionIo;
use crate::{cutout, inpaint, upscale, watermark};

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `image.remove_background`
    RemoveBackground,
    /// `image.erase`
    Erase,
    /// `image.remove_watermark`
    RemoveWatermark,
    /// `image.upscale`
    Upscale,
}

/// A module's manifest and operation.
pub struct VisionModule {
    /// What it runs.
    pub op: Op,
    /// Its manifest, from `modules/builtin/image-ai-*/module.toml`.
    pub manifest: Manifest,
}

const MANIFESTS: [(Op, &str); 4] = [
    (
        Op::RemoveBackground,
        include_str!("../../../modules/builtin/image-ai-remove-background/module.toml"),
    ),
    (
        Op::Erase,
        include_str!("../../../modules/builtin/image-ai-erase/module.toml"),
    ),
    (
        Op::RemoveWatermark,
        include_str!("../../../modules/builtin/image-ai-remove-watermark/module.toml"),
    ),
    (
        Op::Upscale,
        include_str!("../../../modules/builtin/image-ai-upscale/module.toml"),
    ),
];

/// All four modules.
pub fn modules() -> Vec<VisionModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| VisionModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in vision manifest parses"),
        })
        .collect()
}

impl VisionModule {
    /// Run the operation on `inputs` with these options (defaults from the manifest fill in).
    pub fn run(
        &self,
        io: &dyn VisionIo,
        inputs: &[PathBuf],
        options: &Value,
    ) -> Result<Vec<PathBuf>> {
        let opts = Opts::resolve(&self.manifest, options)?;
        match self.op {
            Op::RemoveBackground => cutout::run(io, inputs, &opts),
            Op::Erase => inpaint::run(io, inputs, &opts),
            Op::RemoveWatermark => watermark::run(io, inputs, &opts),
            Op::Upscale => upscale::run(io, inputs, &opts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_are_valid() {
        let all = modules();
        assert_eq!(all.len(), 4);
        for m in &all {
            m.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", m.manifest.module.id));
            assert!(m.manifest.module.id.starts_with("image."));
            // They download models, so they ask for the network and say why.
            assert!(m.manifest.capabilities.network);
            assert!(m.manifest.capabilities.network_reason.is_some());
            assert!(!m.manifest.capabilities.models.is_empty());
        }
        let ids: Vec<&str> = all.iter().map(|m| m.manifest.module.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "image.remove_background",
                "image.erase",
                "image.remove_watermark",
                "image.upscale"
            ]
        );
    }

    #[test]
    fn model_sizes_in_manifests_match_the_pins() {
        for m in modules() {
            for spec in &m.manifest.capabilities.models {
                let pin = crate::models::all()
                    .into_iter()
                    .find(|p| p.id == spec.id)
                    .unwrap_or_else(|| panic!("{} is not a pinned model", spec.id));
                let mb = (pin.size as f64 / 1e6).round() as i64;
                assert!(
                    (i64::from(spec.size_mb) - mb).abs() <= 1,
                    "{}: manifest says {} MB, pin is {} MB",
                    spec.id,
                    spec.size_mb,
                    mb
                );
                assert_eq!(spec.license, pin.license);
            }
        }
    }

    #[test]
    fn watermark_needs_the_ownership_confirmation() {
        let wm = modules()
            .into_iter()
            .find(|m| m.op == Op::RemoveWatermark)
            .unwrap();
        let io = panoply_image::ops::FsIo::new();
        let err = wm
            .run(&io, &[PathBuf::from("nothing.jpg")], &Value::Null)
            .unwrap_err()
            .to_string();
        assert!(err.contains("own_content"), "{err}");
        assert!(err.contains("own"), "{err}");
    }
}
