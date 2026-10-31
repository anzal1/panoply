//! Errors, each with a sentence a person can act on.

/// Why reading a picture or a page failed.
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,
    /// No engine on this machine can read the requested languages.
    #[error("{0}")]
    Unsupported(String),
    /// A model or the ONNX Runtime could not be found, downloaded or loaded.
    #[error("{0}")]
    Engine(String),
    /// The input is not a picture or page this can read.
    #[error("{0}")]
    BadInput(String),
    /// A file could not be read or written.
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for OcrError {
    fn from(e: std::io::Error) -> Self {
        OcrError::Io(format!("A file could not be read or written ({e})."))
    }
}

/// Shorthand.
pub type Result<T> = std::result::Result<T, OcrError>;
