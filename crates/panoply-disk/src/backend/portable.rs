//! The fallback: `std::fs::read_dir` plus one `lstat` per entry. Correct everywhere, about 2 to 4 times
//! slower than the platform backends on a big tree.
//!
//! Allocated size is `st_blocks * 512` on Unix. Where the OS gives no block count the length rounded up to
//! 4 KiB stands in.

use std::io;
use std::path::Path;

use super::{Backend, DirFd, Entry, RootInfo, Scratch, SkipReason};
use crate::tree::Kind;

pub(crate) struct Portable;

#[cfg(unix)]
fn facts(md: &std::fs::Metadata) -> (u64, u64, u64, u64, u32) {
    use std::os::unix::fs::MetadataExt;
    (
        md.blocks() * 512,
        md.len(),
        md.dev(),
        md.ino(),
        md.nlink() as u32,
    )
}

#[cfg(not(unix))]
fn facts(md: &std::fs::Metadata) -> (u64, u64, u64, u64, u32) {
    let len = md.len();
    (len.div_ceil(4096) * 4096, len, 0, 0, 1)
}

fn kind_of(ft: std::fs::FileType) -> Kind {
    if ft.is_dir() {
        Kind::Dir
    } else if ft.is_symlink() {
        Kind::Symlink
    } else if ft.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

#[cfg(unix)]
fn name_bytes(n: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(n.as_bytes())
}

#[cfg(not(unix))]
fn name_bytes(n: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(n.to_string_lossy().into_owned().into_bytes())
}

impl Backend for Portable {
    fn name(&self) -> &'static str {
        "portable (readdir + lstat)"
    }

    fn root_info(&self, path: &Path) -> io::Result<RootInfo> {
        let md = std::fs::metadata(path)?;
        let (alloc, _, dev, _, _) = facts(&md);
        Ok(RootInfo {
            kind: kind_of(md.file_type()),
            dev,
            alloc,
        })
    }

    fn read_dir(
        &self,
        dir: &Path,
        _parent: Option<&DirFd>,
        _scratch: &mut Scratch,
        sink: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<Option<DirFd>, SkipReason> {
        let rd = std::fs::read_dir(dir).map_err(|e| SkipReason::from_io(&e))?;
        for ent in rd {
            let Ok(ent) = ent else { continue };
            // On Unix this is fstatat on the open directory, not a path walk.
            let Ok(md) = ent.metadata() else { continue };
            let (alloc, apparent, dev, id, nlink) = facts(&md);
            let name = ent.file_name();
            let nb = name_bytes(&name);
            sink(&Entry {
                name: &nb,
                kind: kind_of(md.file_type()),
                alloc,
                apparent,
                dev,
                id,
                id_hi: 0,
                nlink,
                clone_id: 0,
                clone_shared: 0,
                flags: 0,
                is_mount_hint: false,
            });
        }
        Ok(None)
    }
}
