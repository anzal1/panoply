use std::path::PathBuf;

use panoply_manifest::Manifest;

use crate::{ModuleError, RunContext};

/// One module. Built-in modules implement this directly. Community modules sit behind the same
/// trait through `panoply_wasm::WasmModule`.
pub trait Module: Send + Sync {
    /// The module's manifest.
    fn manifest(&self) -> &Manifest;

    /// Run the job. Read files only through `ctx.guard`, poll `ctx.cancel` between chunks,
    /// and return the files written (empty for read-only modules).
    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError>;
}
