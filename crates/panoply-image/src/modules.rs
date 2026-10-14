//! The built-in image modules: manifests plus the operation each one runs.

use std::path::PathBuf;

use panoply_manifest::Manifest;
use serde_json::Value;

use crate::error::Result;
use crate::ops::options::Opts;
use crate::ops::{self, Io};

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `image.convert`
    Convert,
    /// `image.compress`
    Compress,
    /// `image.resize`
    Resize,
    /// `image.resize_to_kb`
    ResizeToKb,
    /// `image.strip_metadata`
    StripMetadata,
}

/// A module's manifest and operation.
pub struct ImageModule {
    /// What it runs.
    pub op: Op,
    /// Its manifest, from `modules/builtin/image-*/module.toml`.
    pub manifest: Manifest,
}

const MANIFESTS: [(Op, &str); 5] = [
    (
        Op::Convert,
        include_str!("../../../modules/builtin/image-convert/module.toml"),
    ),
    (
        Op::Compress,
        include_str!("../../../modules/builtin/image-compress/module.toml"),
    ),
    (
        Op::Resize,
        include_str!("../../../modules/builtin/image-resize/module.toml"),
    ),
    (
        Op::ResizeToKb,
        include_str!("../../../modules/builtin/image-resize-to-kb/module.toml"),
    ),
    (
        Op::StripMetadata,
        include_str!("../../../modules/builtin/image-strip-metadata/module.toml"),
    ),
];

/// All five modules.
pub fn modules() -> Vec<ImageModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| ImageModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in image manifest parses"),
        })
        .collect()
}

impl ImageModule {
    /// Run the operation on `inputs` with these options (defaults from the manifest fill in).
    pub fn run(&self, io: &dyn Io, inputs: &[PathBuf], options: &Value) -> Result<Vec<PathBuf>> {
        let opts = Opts::resolve(&self.manifest, options)?;
        match self.op {
            Op::Convert => ops::convert::run(io, inputs, &opts),
            Op::Compress => ops::compress::run(io, inputs, &opts),
            Op::Resize => ops::resize::run(io, inputs, &opts),
            Op::ResizeToKb => ops::kb::run(io, inputs, &opts),
            Op::StripMetadata => ops::strip::run(io, inputs, &opts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_are_valid() {
        for m in modules() {
            m.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", m.manifest.module.id));
            assert!(m.manifest.module.id.starts_with("image."));
        }
        assert_eq!(modules().len(), 5);
    }
}
