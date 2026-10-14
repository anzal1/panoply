//! The error type shared by every part of the crate.

use std::io;

/// Everything that can go wrong while sharing.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file system or socket error.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// A JSON body that does not match the protocol.
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
    /// Certificate or TLS configuration problems.
    #[error("tls: {0}")]
    Tls(String),
    /// The device presented a certificate that is not the one it announced.
    #[error("the device certificate does not match its announced fingerprint")]
    FingerprintMismatch,
    /// The other side speaks a protocol major version we do not.
    #[error("incompatible protocol version {0}")]
    IncompatibleVersion(String),
    /// The receiver declined (HTTP 403).
    #[error("the receiver declined the transfer")]
    Rejected,
    /// The receiver is busy with another session (HTTP 409).
    #[error("the receiver is busy with another transfer")]
    Busy,
    /// Too many requests or wrong PINs (HTTP 429).
    #[error("too many attempts, try again later")]
    TooManyRequests,
    /// The receiver wants a PIN and none (or a wrong one) was given (HTTP 401).
    #[error("a PIN is required")]
    PinRequired,
    /// The transfer was cancelled by the user or by the other side.
    #[error("cancelled")]
    Cancelled,
    /// The other side answered with a status we do not map to anything more specific.
    #[error("the device answered {status}: {message}")]
    Remote {
        /// HTTP status code.
        status: u16,
        /// Plain message from the body, when there was one.
        message: String,
    },
    /// No device matched the name, fingerprint or address.
    #[error("no device matches {0}")]
    NoSuchDevice(String),
    /// Nothing to send, a missing file, or other bad input.
    #[error("{0}")]
    Input(String),
    /// A listener could not start.
    #[error("{0}")]
    Bind(String),
    /// The file hash did not match the one the sender declared (HTTP 422).
    #[error("the file hash did not match")]
    HashMismatch,
    /// A connection or request ran out of time.
    #[error("timed out")]
    Timeout,
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Error::Io(io::Error::other(e))
    }
}

impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Self {
        Error::Tls(e.to_string())
    }
}

impl From<tokio::time::error::Elapsed> for Error {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Error::Timeout
    }
}
