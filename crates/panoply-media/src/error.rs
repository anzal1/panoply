//! Errors. Every message is a plain sentence that says what happened and what to do next,
//! because the UI shows it as is (FLOWS.md, shared capability 10). Nothing here ever carries
//! a stack trace or the helper program's raw log.

use std::fmt;

/// What went wrong in the media engine.
#[derive(Debug)]
pub enum MediaError {
    /// The user cancelled.
    Cancelled,
    /// An option has a value the operation cannot use.
    BadOption(String),
    /// The file or the request needs something this build or this machine does not have.
    Unsupported(String),
    /// The file is damaged or not what its name says. The text may suggest Repair.
    Corrupt(String),
    /// The helper engine (ffmpeg) is missing and could not be fetched.
    Engine(String),
    /// Reading or writing a file failed.
    Io(String),
    /// A job ran but could not reach what was asked (a size target, for example).
    Failed(String),
    /// A file the module may not read.
    NotAllowed(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Cancelled. Nothing was changed."),
            Self::BadOption(m)
            | Self::Unsupported(m)
            | Self::Corrupt(m)
            | Self::Engine(m)
            | Self::Io(m)
            | Self::Failed(m)
            | Self::NotAllowed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for MediaError {}

/// Result alias for the engine.
pub type Result<T> = std::result::Result<T, MediaError>;

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(format!(
            "Could not read or write the file ({e}). Check the file and the free space on the disk, then try again."
        ))
    }
}

/// A file that cannot be read as media, with the standard next step.
pub fn unreadable(what: impl fmt::Display) -> MediaError {
    MediaError::Corrupt(format!(
        "This file could not be read as video or audio ({what}). If it should play, it may be cut off: try Repair video."
    ))
}
