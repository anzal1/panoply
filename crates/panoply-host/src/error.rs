use std::path::PathBuf;

use panoply_core::JobEvent;

/// Why a module run stopped. Every variant has a plain message and a next step, because the
/// UI never shows a stack trace (FLOWS.md, shared capability 10).
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The module tried to touch something it was not given.
    #[error("access denied: {}", .0.display())]
    NotAllowed(PathBuf),
    /// The module asked for the network without having been granted it.
    #[error("network access denied")]
    NetworkDenied,
    /// A file operation failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// An output file already exists. Panoply never overwrites.
    #[error("output already exists: {}", .0.display())]
    OutputExists(PathBuf),
    /// An option had a value the module cannot use.
    #[error("bad option: {0}")]
    BadOption(String),
    /// No module has this id.
    #[error("unknown module: {0}")]
    UnknownModule(String),
    /// A module with this id is already registered.
    #[error("module already registered: {0}")]
    Duplicate(String),
}

impl ModuleError {
    /// What went wrong, in words a person would use.
    pub fn plain_message(&self) -> String {
        match self {
            Self::Cancelled => "Cancelled. Nothing was changed.".into(),
            Self::NotAllowed(p) => format!(
                "This module is not allowed to use {}. It can only use the files you chose.",
                p.display()
            ),
            Self::NetworkDenied => {
                "This module tried to use the network, which it was not allowed to.".into()
            }
            Self::Io { path, source } => match source.kind() {
                std::io::ErrorKind::NotFound => format!("Could not find {}.", path.display()),
                std::io::ErrorKind::PermissionDenied => {
                    format!(
                        "Panoply does not have permission to open {}.",
                        path.display()
                    )
                }
                _ => format!("Could not read or write {}.", path.display()),
            },
            Self::OutputExists(p) => format!("{} already exists.", p.display()),
            Self::BadOption(m) => format!("One of the options is not right: {m}."),
            Self::UnknownModule(id) => format!("There is no module called {id}."),
            Self::Duplicate(id) => format!("A module called {id} is already installed."),
        }
    }

    /// What the person can do next.
    pub fn next_step(&self) -> String {
        match self {
            Self::Cancelled => "Run it again when you are ready.".into(),
            Self::NotAllowed(_) => {
                "Choose the file again so Panoply can hand it to the module.".into()
            }
            Self::NetworkDenied => "Report this to the module author, or remove the module.".into(),
            Self::Io { source, .. } => match source.kind() {
                std::io::ErrorKind::NotFound => {
                    "Check that the file was not moved or deleted.".into()
                }
                std::io::ErrorKind::PermissionDenied => {
                    "Allow access to the folder in your system settings, then try again.".into()
                }
                _ => "Check the file and the free space on the disk, then try again.".into(),
            },
            Self::OutputExists(_) => "Choose another output folder or name.".into(),
            Self::BadOption(_) => "Change the option and run again.".into(),
            Self::UnknownModule(_) => "Run `panoply modules` to see what is installed.".into(),
            Self::Duplicate(_) => "Remove one of the two modules.".into(),
        }
    }

    /// The failure event the UI renders.
    pub fn to_event(&self) -> JobEvent {
        JobEvent::Failed {
            plain_message: self.plain_message(),
            next_step: self.next_step(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_plain_message_and_next_step() {
        let e = ModuleError::Io {
            path: "/x/a.pdf".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };
        match e.to_event() {
            JobEvent::Failed {
                plain_message,
                next_step,
            } => {
                assert!(plain_message.contains("Could not find /x/a.pdf"));
                assert!(!next_step.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
