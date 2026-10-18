//! Errors, each with a sentence a person can act on.

/// Why a speech job failed.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,
    /// This system has no speech engine.
    #[error("{0}")]
    Unsupported(String),
    /// The engine failed or the chosen voice is missing.
    #[error("{0}")]
    Engine(String),
    /// The text or file cannot be read aloud.
    #[error("{0}")]
    BadInput(String),
    /// A file could not be read or written.
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for TtsError {
    fn from(e: std::io::Error) -> Self {
        TtsError::Io(format!("A file could not be read or written ({e})."))
    }
}

/// Shorthand.
pub type Result<T> = std::result::Result<T, TtsError>;
