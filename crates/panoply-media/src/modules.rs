//! The wave 1 media modules: manifests plus the operation each one runs.
//!
//! Manifests live in `modules/builtin/media-*/module.toml` and are embedded here. The host
//! adapter (`panoply-host`, `builtin/media.rs`) wraps [`MediaModule`] behind its `Module` trait.

use std::path::PathBuf;

use panoply_manifest::Manifest;
use serde_json::Value;

use crate::error::Result;
use crate::io::Io;
use crate::ops;
use crate::opts::Opts;

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `media.convert`
    Convert,
    /// `media.extract_audio`
    ExtractAudio,
    /// `media.trim`
    Trim,
    /// `media.info`
    Info,
    /// `media.normalize`
    Normalize,
    /// `media.to_mono`
    ToMono,
    /// `media.hdr_to_sdr`
    HdrToSdr,
    /// `media.streams`
    Streams,
    /// `media.compress`
    Compress,
    /// `media.repair`
    Repair,
    /// `media.denoise`
    Denoise,
}

/// A module's manifest and operation.
pub struct MediaModule {
    /// What it runs.
    pub op: Op,
    /// Its manifest.
    pub manifest: Manifest,
}

const MANIFESTS: &[(Op, &str)] = &[
    (
        Op::Convert,
        include_str!("../../../modules/builtin/media-convert/module.toml"),
    ),
    (
        Op::ExtractAudio,
        include_str!("../../../modules/builtin/media-extract-audio/module.toml"),
    ),
    (
        Op::Trim,
        include_str!("../../../modules/builtin/media-trim/module.toml"),
    ),
    (
        Op::Info,
        include_str!("../../../modules/builtin/media-info/module.toml"),
    ),
    (
        Op::Normalize,
        include_str!("../../../modules/builtin/media-normalize/module.toml"),
    ),
    (
        Op::ToMono,
        include_str!("../../../modules/builtin/media-to-mono/module.toml"),
    ),
    (
        Op::HdrToSdr,
        include_str!("../../../modules/builtin/media-hdr-to-sdr/module.toml"),
    ),
    (
        Op::Streams,
        include_str!("../../../modules/builtin/media-streams/module.toml"),
    ),
    (
        Op::Compress,
        include_str!("../../../modules/builtin/media-compress/module.toml"),
    ),
    (
        Op::Repair,
        include_str!("../../../modules/builtin/media-repair/module.toml"),
    ),
    (
        Op::Denoise,
        include_str!("../../../modules/builtin/media-denoise/module.toml"),
    ),
];

/// Every media module.
pub fn modules() -> Vec<MediaModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| MediaModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in media manifest parses"),
        })
        .collect()
}

impl MediaModule {
    /// Run the operation on `inputs` with these options (defaults from the manifest fill in).
    pub fn run(&self, io: &dyn Io, inputs: &[PathBuf], options: &Value) -> Result<Vec<PathBuf>> {
        let opts = Opts::resolve(&self.manifest, options)?;
        if inputs.is_empty() {
            return Err(crate::error::MediaError::BadOption(
                "Choose at least one file first".into(),
            ));
        }
        match self.op {
            Op::Convert => ops::convert::run(io, inputs, &opts),
            Op::ExtractAudio => ops::extract_audio::run(io, inputs, &opts),
            Op::Trim => ops::trim::run(io, inputs, &opts),
            Op::Info => ops::info::run(io, inputs, &opts),
            Op::Normalize => ops::normalize::run(io, inputs, &opts),
            Op::ToMono => ops::to_mono::run(io, inputs, &opts),
            Op::HdrToSdr => ops::hdr_to_sdr::run(io, inputs, &opts),
            Op::Streams => ops::streams::run(io, inputs, &opts),
            Op::Compress => ops::compress::run(io, inputs, &opts),
            Op::Repair => ops::repair::run(io, inputs, &opts),
            Op::Denoise => ops::denoise::run(io, inputs, &opts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_are_valid_and_ids_match_the_folder_names() {
        for m in modules() {
            m.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", m.manifest.module.id));
            assert!(m.manifest.module.id.starts_with("media."));
            assert!(
                m.manifest.capabilities.network,
                "engine download is declared"
            );
        }
    }
}
