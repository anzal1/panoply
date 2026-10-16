use std::fmt;

/// A plain error. The message is a short sentence safe to show to a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    #[cfg(windows)]
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Error(message.into())
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[cfg(windows)]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}
