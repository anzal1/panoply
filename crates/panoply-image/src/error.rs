//! Errors. Every message is a plain sentence that says what happened and what to do next,
//! because the UI shows it as is (FLOWS.md, shared capability 10).

use std::fmt;

/// What went wrong in the image pipeline.
#[derive(Debug)]
pub enum ImageError {
    /// The user cancelled.
    Cancelled,
    /// An option has a value the operation cannot use.
    BadOption(String),
    /// The file or the request needs something this build does not have.
    Unsupported(String),
    /// The file is damaged or not what its name says.
    Corrupt(String),
    /// The image is too large to process inside the memory cap.
    TooLarge(String),
    /// Reading or writing a file failed.
    Io(String),
    /// A job ran fine but could not reach what was asked (for example a size target).
    Failed(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Cancelled. Nothing was changed."),
            Self::BadOption(m)
            | Self::Unsupported(m)
            | Self::Corrupt(m)
            | Self::TooLarge(m)
            | Self::Io(m)
            | Self::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ImageError {}

/// Result alias for the pipeline.
pub type Result<T> = std::result::Result<T, ImageError>;

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(format!(
            "Could not read or write the file ({e}). Check the file and the free space on the disk, then try again."
        ))
    }
}

/// A damaged-file error with the standard next step.
pub fn corrupt(what: impl fmt::Display) -> ImageError {
    ImageError::Corrupt(format!(
        "This image looks damaged ({what}). Try opening it in another app, or export it again from the original."
    ))
}
