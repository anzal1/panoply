//! Reading a whole folder with `getattrlistbulk`.
//!
//! `readdir` plus one `lstat` per entry costs a system call per file. `getattrlistbulk` returns the
//! name, the object type and the modified time of many entries per call (a 64 KB buffer holds a few
//! hundred), which is what makes a first scan of a million files take seconds. The records are parsed
//! from the byte buffer with ordinary slice reads; the only `unsafe` is the call itself.
//!
//! Record layout (see `man 2 getattrlistbulk`): a `u32` length, the set of attributes actually returned
//! (five `u32`), then each returned attribute in the order of its bit: name (an offset and a length,
//! relative to the start of that field, pointing at a NUL terminated string inside the record), device
//! id (`u32`), object type (`u32`), modified time (`timespec`, two `i64`).

use std::cell::RefCell;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use crate::error::Error;

/// What kind of object an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A folder.
    Dir,
    /// A symbolic link (never followed).
    Symlink,
    /// A socket, device, fifo or anything else.
    Other,
}

/// One entry of a folder.
#[derive(Clone, Copy, Debug)]
pub struct BulkEntry<'a> {
    /// The name, without a path, as the file system stores it.
    pub name: &'a [u8],
    /// The object type.
    pub kind: EntryKind,
    /// Modified time in seconds since the Unix epoch (0 when the file system did not report one).
    pub mtime: i64,
    /// Device id of the volume the entry lives on (0 when not reported).
    pub dev: u32,
}

const BUF_BYTES: usize = 128 * 1024;
const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

thread_local! {
    static BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; BUF_BYTES]);
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn i64_at(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_ne_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Reads the folder `path` and calls `f` for every entry (never `.` or `..`).
///
/// Returns an error when the folder cannot be opened or the file system does not support bulk reads
/// (the caller falls back to `readdir`); entries already delivered stay delivered.
pub fn read_dir_bulk(path: &Path, f: &mut dyn FnMut(BulkEntry<'_>)) -> Result<(), Error> {
    let dir = File::open(path).map_err(|e| Error::new(format!("cannot open folder: {e}")))?;
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS
            | libc::ATTR_CMN_NAME
            | libc::ATTR_CMN_DEVID
            | libc::ATTR_CMN_OBJTYPE
            | libc::ATTR_CMN_MODTIME,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        loop {
            // SAFETY: `dir` is an open file descriptor for the whole call; `list` is a fully initialised
            // attrlist that outlives the call; `buf` is a writable allocation of exactly the length passed.
            // The kernel writes only inside it and returns the number of records written.
            let n = unsafe {
                libc::getattrlistbulk(
                    dir.as_raw_fd(),
                    (&raw mut list).cast(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    u64::from(libc::FSOPT_PACK_INVAL_ATTRS),
                )
            };
            if n < 0 {
                return Err(Error::new(format!(
                    "bulk read failed: {}",
                    io::Error::last_os_error()
                )));
            }
            if n == 0 {
                return Ok(());
            }
            let mut at = 0usize;
            for _ in 0..n {
                let Some(len) = u32_at(&buf, at).map(|l| l as usize) else {
                    return Err(Error::new("short bulk record"));
                };
                if len < 24 || at + len > buf.len() {
                    return Err(Error::new("bad bulk record length"));
                }
                let rec = &buf[at..at + len];
                at += len;
                parse(rec, f);
            }
        }
    })
}

/// Parses one record and delivers it. A record with no usable name is skipped.
fn parse(rec: &[u8], f: &mut dyn FnMut(BulkEntry<'_>)) {
    // rec[0..4] is the length; rec[4..24] the returned attribute set (commonattr first).
    let Some(common) = u32_at(rec, 4) else { return };
    let mut p = 24usize;
    let mut name: Option<&[u8]> = None;
    let (mut dev, mut objtype, mut mtime) = (0u32, 0u32, 0i64);
    if common & libc::ATTR_CMN_NAME != 0 {
        let field = p;
        let (Some(off), Some(len)) = (u32_at(rec, p).map(|v| v as i32), u32_at(rec, p + 4)) else {
            return;
        };
        p += 8;
        let start = field as i64 + i64::from(off);
        if start < 0 {
            return;
        }
        let start = start as usize;
        let Some(raw) = rec.get(start..start + len as usize) else {
            return;
        };
        // The length counts the terminating NUL.
        name = Some(raw.strip_suffix(&[0]).unwrap_or(raw));
    }
    if common & libc::ATTR_CMN_DEVID != 0 {
        dev = u32_at(rec, p).unwrap_or(0);
        p += 4;
    }
    if common & libc::ATTR_CMN_OBJTYPE != 0 {
        objtype = u32_at(rec, p).unwrap_or(0);
        p += 4;
    }
    if common & libc::ATTR_CMN_MODTIME != 0 {
        mtime = i64_at(rec, p).unwrap_or(0);
    }
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return;
    };
    let kind = match objtype {
        VREG => EntryKind::File,
        VDIR => EntryKind::Dir,
        VLNK => EntryKind::Symlink,
        _ => EntryKind::Other,
    };
    f(BulkEntry {
        name,
        kind,
        mtime,
        dev,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn matches_std_read_dir() {
        let root = std::env::temp_dir().join(format!("panoply-bulk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        for i in 0..700 {
            std::fs::write(root.join(format!("file {i} \u{e9}.txt")), b"x").unwrap();
        }
        std::os::unix::fs::symlink("sub", root.join("link")).unwrap();
        let mut got: BTreeMap<Vec<u8>, (EntryKind, i64, u32)> = BTreeMap::new();
        read_dir_bulk(&root, &mut |e| {
            got.insert(e.name.to_vec(), (e.kind, e.mtime, e.dev));
        })
        .unwrap();
        let mut want = 0;
        for ent in std::fs::read_dir(&root).unwrap() {
            let ent = ent.unwrap();
            let md = ent.metadata().unwrap();
            let key = std::os::unix::ffi::OsStrExt::as_bytes(ent.file_name().as_os_str()).to_vec();
            let (kind, mtime, _dev) = *got.get(&key).expect("entry missing from bulk read");
            let ft = md.file_type();
            let expect = if ft.is_dir() {
                EntryKind::Dir
            } else if ft.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::File
            };
            assert_eq!(kind, expect);
            assert_eq!(mtime, md.mtime());
            want += 1;
        }
        assert_eq!(got.len(), want);
        assert!(got.len() > 700);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_folder_is_an_error() {
        assert!(read_dir_bulk(Path::new("/definitely/not/here"), &mut |_| {}).is_err());
    }
}
