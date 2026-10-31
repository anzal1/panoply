//! Errors of the PDF engine. Every variant reads as a sentence a person can act on, because the
//! host shows `Display` text directly (FLOWS.md, shared capability 10: plain errors).

use std::path::PathBuf;

/// Why a PDF job stopped.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// The bytes are not a PDF at all.
    #[error("This file is not a PDF, or it is empty.")]
    NotPdf,
    /// The file is encrypted. Phase 1 never decrypts, it says so.
    #[error("This PDF is password-protected. Enter the password or try Unlock PDF.")]
    Encrypted,
    /// The file is damaged beyond what the engine can repair.
    #[error(
        "This PDF is damaged and could not be repaired ({0}). Open it in a viewer and print it to a new PDF, then try again."
    )]
    Damaged(String),
    /// The file is damaged and also too big to repair in memory.
    #[error(
        "This PDF is damaged and too large to repair here. Repair it with another tool first, then try again."
    )]
    DamagedTooLarge,
    /// A feature this build does not do yet. The text says which and what to do instead.
    #[error("{0}")]
    Unsupported(String),
    /// The user's options or page ranges do not make sense.
    #[error("{0}")]
    BadInput(String),
    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file involved, empty when unknown.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The rendering engine (pdfium) could not be loaded or failed.
    #[error("{0}")]
    Engine(String),
}

impl From<std::io::Error> for PdfError {
    fn from(source: std::io::Error) -> Self {
        PdfError::Io {
            path: PathBuf::new(),
            source,
        }
    }
}

/// Result alias for the engine.
pub type Result<T> = std::result::Result<T, PdfError>;

impl PdfError {
    /// Shorthand for a damaged-file error with a short technical reason.
    pub fn damaged(why: impl Into<String>) -> Self {
        PdfError::Damaged(why.into())
    }
}
