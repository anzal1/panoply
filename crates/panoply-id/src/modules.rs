//! The built-in ID modules: manifests plus the operation each one runs.

use std::path::PathBuf;

use panoply_image::error::Result;
use panoply_image::ops::Io;
use panoply_image::ops::options::Opts;
use panoply_manifest::Manifest;
use serde_json::Value;

use crate::{passport, sheet, signature};

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `id.passport_photo`
    PassportPhoto,
    /// `id.print_sheet`
    PrintSheet,
    /// `id.signature_cleaner`
    SignatureCleaner,
}

/// A module's manifest and operation.
pub struct IdModule {
    /// What it runs.
    pub op: Op,
    /// Its manifest, from `modules/builtin/id-*/module.toml`.
    pub manifest: Manifest,
}

const MANIFESTS: [(Op, &str); 3] = [
    (
        Op::PassportPhoto,
        include_str!("../../../modules/builtin/id-passport-photo/module.toml"),
    ),
    (
        Op::PrintSheet,
        include_str!("../../../modules/builtin/id-print-sheet/module.toml"),
    ),
    (
        Op::SignatureCleaner,
        include_str!("../../../modules/builtin/id-signature-cleaner/module.toml"),
    ),
];

/// All three modules.
pub fn modules() -> Vec<IdModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| IdModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in ID manifest parses"),
        })
        .collect()
}

impl IdModule {
    /// Run the operation on `inputs` with these options (defaults from the manifest fill in).
    pub fn run(&self, io: &dyn Io, inputs: &[PathBuf], options: &Value) -> Result<Vec<PathBuf>> {
        let opts = Opts::resolve(&self.manifest, options)?;
        match self.op {
            Op::PassportPhoto => passport::run(io, inputs, &opts),
            Op::PrintSheet => sheet::run(io, inputs, &opts),
            Op::SignatureCleaner => signature::run(io, inputs, &opts),
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
            assert!(m.manifest.module.id.starts_with("id."));
        }
        assert_eq!(modules().len(), 3);
    }

    #[test]
    fn the_passport_manifest_says_what_it_downloads() {
        let m = modules()
            .into_iter()
            .find(|m| m.op == Op::PassportPhoto)
            .unwrap();
        assert!(m.manifest.capabilities.network);
        assert_eq!(m.manifest.capabilities.models.len(), 1);
        assert_eq!(m.manifest.capabilities.models[0].license, "MIT");
    }
}
