//! One trait, one backend per platform, and a portable fallback.
//!
//! A backend reads one folder and reports every entry in it with its allocated size and the identity
//! needed to count hard links and clones once. It never recurses: the scanner owns the walk.

use std::io;
use std::path::Path;

use crate::tree::Kind;

pub(crate) mod portable;

#[cfg(unix)]
pub(crate) mod fds;
#[cfg(unix)]
pub(crate) use fds::DirFd;
/// Windows opens folders by path: there is no descriptor to hand to the children.
#[cfg(not(unix))]
pub(crate) struct DirFd;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(any(windows, test))]
pub(crate) mod windows;

/// Why a folder could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SkipReason {
    /// The account has no permission (EACCES, access denied).
    Permission = 0,
    /// macOS refused with EPERM: Full Disk Access (or a Files and Folders grant) would let it in.
    FullDiskAccess = 1,
    /// The folder vanished during the scan.
    Gone = 2,
    /// Any other error.
    Other = 3,
}

impl SkipReason {
    pub(crate) fn from_io(e: &io::Error) -> SkipReason {
        match e.kind() {
            io::ErrorKind::PermissionDenied => {
                // macOS reports a TCC refusal as EPERM (1); a plain permission error is EACCES (13).
                #[cfg(target_os = "macos")]
                if e.raw_os_error() == Some(1) {
                    return SkipReason::FullDiskAccess;
                }
                SkipReason::Permission
            }
            io::ErrorKind::NotFound => SkipReason::Gone,
            _ => SkipReason::Other,
        }
    }
}

/// One directory entry as the OS reported it.
pub(crate) struct Entry<'a> {
    pub name: &'a [u8],
    pub kind: Kind,
    /// Bytes allocated on disk (blocks times block size, or the OS's allocation size).
    pub alloc: u64,
    /// Length of the data, for spotting sparse and compressed files.
    pub apparent: u64,
    /// Device (volume) the entry lives on, 0 when the backend does not report it.
    pub dev: u64,
    /// File ID (inode), the low half of the file ID on Windows.
    pub id: u64,
    /// The high half of a 128-bit file ID (0 elsewhere).
    pub id_hi: u64,
    /// Hard link count, 1 when unknown.
    pub nlink: u32,
    /// APFS clone ID, 0 when none.
    pub clone_id: u64,
    /// Bytes of this file shared with at least one clone, 0 when none.
    pub clone_shared: u64,
    /// `tree::flags` bits the backend knows (dataless).
    pub flags: u8,
    /// A mount point or reparse point the backend recognised as another volume, if it can tell.
    pub is_mount_hint: bool,
}

/// Reusable per-thread buffer, handed out 8-byte aligned.
pub(crate) struct Scratch {
    buf: Vec<u8>,
}

impl Scratch {
    pub fn new(bytes: usize) -> Scratch {
        Scratch {
            buf: vec![0; bytes + 8],
        }
    }

    pub fn bytes(&mut self) -> &mut [u8] {
        let pad = (8 - (self.buf.as_ptr() as usize % 8)) % 8;
        let end = self.buf.len() - 8 + pad;
        &mut self.buf[pad..end]
    }
}

/// What the scanner needs to know about the root.
pub(crate) struct RootInfo {
    pub kind: Kind,
    pub dev: u64,
    pub alloc: u64,
}

pub(crate) trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn root_info(&self, path: &Path) -> io::Result<RootInfo>;
    /// Report every entry of `dir` to `sink`. `.` and `..` are never reported. `parent` is the open parent
    /// folder when the scanner still holds it: opening `dir` relative to it is one name lookup instead of a
    /// walk down the whole path. Returns the folder's own descriptor when the backend has one worth keeping
    /// for its children.
    fn read_dir(
        &self,
        dir: &Path,
        parent: Option<&DirFd>,
        scratch: &mut Scratch,
        sink: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<Option<DirFd>, SkipReason>;
}

/// Which backend to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendChoice {
    /// The fastest the platform has (default).
    #[default]
    Auto,
    /// `std::fs` readdir plus one `lstat` per entry, on every platform. Used to compare and as the
    /// fallback.
    Portable,
}

pub(crate) fn pick(choice: BackendChoice, clone_aware: bool) -> Box<dyn Backend> {
    #[cfg(not(target_os = "macos"))]
    let _ = clone_aware;
    if choice == BackendChoice::Auto {
        #[cfg(target_os = "macos")]
        return Box::new(macos::Bulk::new(clone_aware));
        #[cfg(target_os = "linux")]
        return Box::new(linux::Getdents);
        #[cfg(windows)]
        return Box::new(windows::Extd);
    }
    Box::new(portable::Portable)
}

/// Physical extents of a file as (device, ranges): how blocks shared through clones are told apart from
/// private ones. macOS only.
#[allow(unused_variables)]
pub(crate) fn file_extents(path: &Path, alloc: u64) -> Option<(u64, Vec<(u64, u64)>)> {
    #[cfg(target_os = "macos")]
    return macos::file_extents(path, alloc);
    #[cfg(not(target_os = "macos"))]
    None
}

/// (allocated, private) bytes of one file, where the OS can say. macOS only.
#[allow(unused_variables)]
pub(crate) fn private_size(path: &Path) -> Option<(u64, u64)> {
    #[cfg(target_os = "macos")]
    return macos::private_size(path);
    #[cfg(not(target_os = "macos"))]
    None
}

/// True when [`private_size`] works on this platform, so the scanner looks for clones among big files.
pub(crate) const PROBES_CLONES: bool = cfg!(target_os = "macos");
