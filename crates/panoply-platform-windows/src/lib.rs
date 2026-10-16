//! Panoply Windows platform crate: file change events for the instant search index, behind a safe API.
//!
//! # Unsafe policy
//!
//! This is the only crate in the workspace that is allowed to use `unsafe` on Windows (its `Cargo.toml`
//! sets `unsafe_code = "allow"` instead of inheriting the workspace `forbid`). All of it is in `sys.rs`,
//! a thin layer over `windows-sys` that owns handles, overlapped I/O and the buffers they write into.
//! Every block carries a `// SAFETY:` comment. Callers never see a handle or a raw pointer.
//!
//! The system calls are compiled on Windows only. The byte decoders ([`usn`], [`notify`], [`paths`])
//! have no dependencies and build and test on every platform, so the workspace builds everywhere.
//!
//! # How changes are found
//!
//! **The USN journal (off by default).** It is reviewed but has never run against a real Windows volume
//! (the code was written and checked on a Mac), and it only works for an elevated process, so it is opt in:
//! [`Options::usn_journal`], or the environment variable `PANOPLY_FIND_USN=1` through
//! [`Options::from_env`]. The default is `ReadDirectoryChangesW`. When it is on: on an NTFS volume the file system keeps a change journal: one record per change,
//! with the file reference number of the file and of its parent folder. One reader per volume blocks in
//! `FSCTL_READ_USN_JOURNAL` (with `BytesToWaitFor` set, so the kernel wakes it when a record exists), turns
//! each record into a path by opening the parent folder by its reference number, and reports the path if it
//! lies below a watched root. Idle cost is zero: no timer, no polling. A journal that wrapped past our
//! position (`ERROR_JOURNAL_ENTRY_DELETED`) is reported as a rescan of the roots.
//!
//! **`ReadDirectoryChangesW`.** The default. Also used, with the journal on, when the volume is not NTFS, the journal is not enabled, the process
//! lacks the privilege to open the volume, or a root has no drive letter. One worker per root waits on an
//! overlapped read with an infinite timeout, recursive. A buffer overflow is reported as a rescan.
//!
//! Both wait on the I/O event and a stop event, so dropping the [`Watcher`] ends the threads at once.
//!
//! # Known gaps
//!
//! `ReadDirectoryChangesW` has a short window between one read completing and the next being issued;
//! the 64 KiB buffer covers bursts, and a loss shows up as an overflow only when the kernel notices it.
//! The USN position is not saved across runs yet.

mod error;
pub mod notify;
pub mod paths;
pub mod usn;

pub use error::Error;

use std::path::PathBuf;

/// One report from a watcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Something at this path changed (created, deleted, renamed, written).
    Changed(PathBuf),
    /// Changes below this folder were lost or coalesced: read it again.
    Rescan(PathBuf),
}

/// Choices for [`start_with`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Try the NTFS USN journal first. Off by default: see the crate docs.
    pub usn_journal: bool,
}

impl Options {
    /// The defaults, with the USN journal on when `PANOPLY_FIND_USN` is `1`.
    pub fn from_env() -> Self {
        Options {
            usn_journal: std::env::var_os("PANOPLY_FIND_USN").is_some_and(|v| v == "1"),
        }
    }
}

/// What reports changes for a watched root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The NTFS USN change journal of the volume.
    UsnJournal,
    /// `ReadDirectoryChangesW` on the root.
    DirectoryChanges,
}

#[cfg(windows)]
mod sys;
#[cfg(windows)]
mod watcher;

#[cfg(windows)]
pub use watcher::{Watcher, start, start_with};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_is_off_by_default() {
        assert!(!Options::default().usn_journal);
    }
}
