//! Panoply module manifest, version 0.
//!
//! One `module.toml` per module declares its inputs, options, outputs, capabilities and the
//! surfaces it appears on. Every platform renders the options natively, and the CLI and MCP
//! tools are generated from the same file, so a module never ships UI code.
//!
//! ```toml
//! accepts = ["application/pdf"]
//! outputs = ["application/pdf"]
//!
//! [module]
//! id = "pdf.compress"
//! name = "Compress PDF"
//! version = "0.1.0"
//! pattern = "B"
//! summary = "Make this PDF small enough to email or upload."
//!
//! [options.grayscale]
//! type = "bool"
//! default = false
//! ```

mod pattern;
mod schema;
mod validate;

pub use pattern::Pattern;
pub use schema::{
    Capabilities, FilesAccess, Manifest, ModelSpec, ModuleInfo, OptionSpec, OptionType, Surfaces,
};
pub use validate::ALLOWED_MODEL_LICENSES;

/// Errors from reading a manifest.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The file could not be read.
    #[error("Could not read {path}: {source}")]
    Io {
        /// File that failed to open.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The text is not a valid manifest.
    #[error("This module.toml is not valid: {0}")]
    Parse(String),
}
