//! macOS: `getattrlistbulk`, which returns the name, type, file ID, link count and allocated size of many
//! entries in one system call, with no `stat` per file.
//!
//! On APFS it also asks for the private size and the clone ID (extended common attributes), which is how
//! a clone that shares its blocks with another file is counted once. A folder the call cannot read (a
//! filesystem without bulk support) falls back to the portable reader for that folder only.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use libc::{c_int, c_void};

use super::{Backend, DirFd, Entry, RootInfo, Scratch, SkipReason, portable};
use crate::tree::{Kind, flags};

const ATTR_BIT_MAP_COUNT: u16 = 5;
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_DEVID: u32 = 0x0000_0002;
const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
const ATTR_CMN_FLAGS: u32 = 0x0004_0000;
const ATTR_CMN_FILEID: u32 = 0x0200_0000;
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
const ATTR_FILE_LINKCOUNT: u32 = 0x0000_0001;
const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;
const ATTR_FILE_DATALENGTH: u32 = 0x0000_0200;
const ATTR_CMNEXT_PRIVATESIZE: u32 = 0x0000_0008;
const ATTR_CMNEXT_CLONEID: u32 = 0x0000_0100;
const ATTR_CMNEXT_CLONE_REFCNT: u32 = 0x0000_1000;
const FSOPT_ATTR_CMN_EXTENDED: u64 = 0x20;

const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;
const SF_DATALESS: u32 = 0x4000_0000;

/// 256 KiB: one call covers a few thousand entries of a big folder.
pub(crate) const BUF: usize = 256 * 1024;

pub(crate) struct Bulk {
    clones: bool,
    portable: portable::Portable,
}

impl Bulk {
    pub fn new(clones: bool) -> Bulk {
        Bulk {
            clones,
            portable: portable::Portable,
        }
    }
}

fn rd<T: Copy>(buf: &[u8], off: usize) -> Option<T> {
    if off + std::mem::size_of::<T>() > buf.len() {
        return None;
    }
    // SAFETY: bounds checked above; read_unaligned has no alignment requirement and every T used here
    // is a plain integer.
    Some(unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast::<T>()) })
}

/// Parse one packed entry. Returns false when it has no usable name or reported an error for itself.
fn parse_entry(e: &[u8], clones: bool, sink: &mut dyn FnMut(&Entry<'_>)) -> bool {
    // length (4), returned attribute set (5 x u32), then, in this order: the entry's own error if it has
    // one, then the attributes in bit order (common, volume, directory, file, fork group).
    let (Some(rc), Some(rfile), Some(rfork)) =
        (rd::<u32>(e, 4), rd::<u32>(e, 16), rd::<u32>(e, 20))
    else {
        return false;
    };
    let mut p = 24;
    if rc & ATTR_CMN_ERROR != 0 {
        let err: u32 = match rd(e, p) {
            Some(v) => v,
            None => return false,
        };
        p += 4;
        if err != 0 {
            return false;
        }
    }
    let mut name: &[u8] = &[];
    if rc & ATTR_CMN_NAME != 0 {
        let at = p;
        let (Some(off), Some(len)) = (rd::<i32>(e, p), rd::<u32>(e, p + 4)) else {
            return false;
        };
        p += 8;
        let start = (at as i64 + off as i64) as usize;
        let end = start + (len as usize).saturating_sub(1); // drop the NUL
        if end > e.len() || start > end {
            return false;
        }
        name = &e[start..end];
    }
    let mut dev = 0u64;
    if rc & ATTR_CMN_DEVID != 0 {
        dev = rd::<u32>(e, p).unwrap_or(0) as u64;
        p += 4;
    }
    let mut objtype = 0u32;
    if rc & ATTR_CMN_OBJTYPE != 0 {
        objtype = rd::<u32>(e, p).unwrap_or(0);
        p += 4;
    }
    let mut st_flags = 0u32;
    if rc & ATTR_CMN_FLAGS != 0 {
        st_flags = rd::<u32>(e, p).unwrap_or(0);
        p += 4;
    }
    let mut id = 0u64;
    if rc & ATTR_CMN_FILEID != 0 {
        id = rd::<u64>(e, p).unwrap_or(0);
        p += 8;
    }
    let mut nlink = 1u32;
    if rfile & ATTR_FILE_LINKCOUNT != 0 {
        nlink = rd::<u32>(e, p).unwrap_or(1);
        p += 4;
    }
    let mut alloc = 0u64;
    if rfile & ATTR_FILE_ALLOCSIZE != 0 {
        alloc = rd::<i64>(e, p).unwrap_or(0).max(0) as u64;
        p += 8;
    }
    let mut apparent = 0u64;
    if rfile & ATTR_FILE_DATALENGTH != 0 {
        apparent = rd::<i64>(e, p).unwrap_or(0).max(0) as u64;
        p += 8;
    }
    let mut clone_id = 0u64;
    if clones && rfork & ATTR_CMNEXT_CLONEID != 0 {
        clone_id = rd::<u64>(e, p).unwrap_or(0);
        p += 8;
    }
    let mut refcnt = 1u32;
    if clones && rfork & ATTR_CMNEXT_CLONE_REFCNT != 0 {
        refcnt = rd::<u32>(e, p).unwrap_or(1);
    }
    if name.is_empty() {
        return false;
    }
    let kind = match objtype {
        VREG => Kind::File,
        VDIR => Kind::Dir,
        VLNK => Kind::Symlink,
        _ => Kind::Other,
    };
    let mut fl = 0u8;
    if st_flags & SF_DATALESS != 0 {
        fl |= flags::DATALESS;
    }
    // A file whose clone count is above 1 still has partners that hold its blocks. Which blocks, and how many,
    // the scanner settles later from the physical extents; `alloc` is the upper bound.
    let clone_shared = if kind == Kind::File && refcnt > 1 && clone_id != 0 {
        alloc
    } else {
        0
    };
    sink(&Entry {
        name,
        kind,
        alloc: if kind == Kind::Dir { 0 } else { alloc },
        apparent,
        dev,
        id,
        id_hi: 0,
        nlink,
        // The ID only groups files that still know each other (count above 1).
        clone_id: if clone_shared > 0 && refcnt > 1 {
            clone_id
        } else {
            0
        },
        clone_shared,
        flags: fl,
        is_mount_hint: false,
    });
    true
}

impl Backend for Bulk {
    fn name(&self) -> &'static str {
        if self.clones {
            "getattrlistbulk (with APFS clone accounting)"
        } else {
            "getattrlistbulk"
        }
    }

    fn root_info(&self, path: &Path) -> io::Result<RootInfo> {
        self.portable.root_info(path)
    }

    fn read_dir(
        &self,
        dir: &Path,
        parent: Option<&DirFd>,
        scratch: &mut Scratch,
        sink: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<Option<DirFd>, SkipReason> {
        let fd = DirFd::open(dir, parent).map_err(|e| SkipReason::from_io(&e))?;
        match self.bulk(fd.raw(), scratch, sink) {
            Ok(()) => Ok(fd.keep()),
            Err(Fallback) => self.portable.read_dir(dir, None, scratch, sink),
        }
    }
}

struct Fallback;

impl Bulk {
    fn bulk(
        &self,
        fd: c_int,
        scratch: &mut Scratch,
        sink: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<(), Fallback> {
        let mut al = libc::attrlist {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: ATTR_CMN_RETURNED_ATTRS
                | ATTR_CMN_ERROR
                | ATTR_CMN_NAME
                | ATTR_CMN_DEVID
                | ATTR_CMN_OBJTYPE
                | ATTR_CMN_FLAGS
                | ATTR_CMN_FILEID,
            volattr: 0,
            dirattr: 0,
            fileattr: ATTR_FILE_LINKCOUNT | ATTR_FILE_ALLOCSIZE | ATTR_FILE_DATALENGTH,
            // Clone ID and count are cheap. The private size is not: asking for it for every file doubled a
            // scan of a million files (6 s to 13 s), so it is read afterwards for the big files only.
            forkattr: if self.clones {
                ATTR_CMNEXT_CLONEID | ATTR_CMNEXT_CLONE_REFCNT
            } else {
                0
            },
        };
        // No FSOPT_PACK_INVAL_ATTRS: every entry says in its returned set which attributes it holds.
        let mut options = 0u64;
        if self.clones {
            options |= FSOPT_ATTR_CMN_EXTENDED;
        }
        let buf = scratch.bytes();
        let mut first = true;
        loop {
            // SAFETY: `al` is a valid attrlist, `buf` is writable for buf.len() bytes, fd is open.
            let n = unsafe {
                libc::getattrlistbulk(
                    fd,
                    (&mut al as *mut libc::attrlist).cast::<c_void>(),
                    buf.as_mut_ptr().cast::<c_void>(),
                    buf.len(),
                    options,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                // Not supported on this filesystem (or by this combination): read the folder portably.
                // Only safe before any entry was reported.
                if first {
                    return Err(Fallback);
                }
                return Ok(());
            }
            if n == 0 {
                return Ok(());
            }
            first = false;
            let mut off = 0usize;
            for _ in 0..n {
                let Some(len) = rd::<u32>(buf, off) else {
                    break;
                };
                let len = len as usize;
                if len < 24 || off + len > buf.len() {
                    break;
                }
                parse_entry(&buf[off..off + len], self.clones, sink);
                off += len;
            }
        }
    }
}

/// The physical byte ranges (device offset, length) a file occupies, for finding blocks shared with other
/// files. `None` when the file cannot be opened or the filesystem will not say.
pub(crate) fn file_extents(path: &Path, alloc: u64) -> Option<(u64, Vec<(u64, u64)>)> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    let f = std::fs::File::open(path).ok()?;
    let md = f.metadata().ok()?;
    let dev = md.dev();
    // Ask up to the allocation, which is the length rounded up to whole blocks.
    let len = md.len().max(alloc);
    let fd = f.as_raw_fd();
    let mut out = Vec::new();
    let mut pos = 0u64;
    let mut guard = 0u32;
    while pos < len {
        guard += 1;
        if guard > 200_000 {
            return None;
        }
        let mut l2p = libc::log2phys {
            l2p_flags: 0,
            l2p_contigbytes: (len - pos) as libc::off_t,
            l2p_devoffset: pos as libc::off_t,
        };
        // SAFETY: fd is open for the life of `f`; `l2p` is a valid in/out struct for F_LOG2PHYS_EXT.
        let r = unsafe { libc::fcntl(fd, libc::F_LOG2PHYS_EXT, &mut l2p as *mut libc::log2phys) };
        if r == 0 && l2p.l2p_contigbytes > 0 {
            out.push((l2p.l2p_devoffset as u64, l2p.l2p_contigbytes as u64));
            pos += l2p.l2p_contigbytes as u64;
        } else {
            // A hole: jump to the next data, or finish when there is none.
            // SAFETY: plain lseek on an open fd.
            let next = unsafe { libc::lseek(fd, (pos + 1) as libc::off_t, libc::SEEK_DATA) };
            if next < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::ENXIO) {
                    break;
                }
                return None;
            }
            pos = next as u64;
        }
    }
    Some((dev, out))
}

/// Allocated and private size of one file (what deleting it would free). Costly per file, so the scanner asks
/// only for big files. `None` when the file cannot be read or the volume has no private size.
pub(crate) fn private_size(path: &Path) -> Option<(u64, u64)> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut al = libc::attrlist {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: 0,
        dirattr: 0,
        fileattr: ATTR_FILE_ALLOCSIZE,
        forkattr: ATTR_CMNEXT_PRIVATESIZE,
    };
    let mut buf = [0u8; 64];
    // SAFETY: valid path and attrlist; the buffer is writable for its whole length.
    let r = unsafe {
        libc::getattrlist(
            c.as_ptr(),
            (&mut al as *mut libc::attrlist).cast::<c_void>(),
            buf.as_mut_ptr().cast::<c_void>(),
            buf.len(),
            libc::FSOPT_NOFOLLOW | FSOPT_ATTR_CMN_EXTENDED as u32,
        )
    };
    if r != 0 {
        return None;
    }
    // length (4), then the file attribute, then the fork attribute, both packed on 4 bytes.
    let len: u32 = rd(&buf, 0)?;
    if len < 20 {
        return None;
    }
    let alloc: i64 = rd(&buf, 4)?;
    let private: i64 = rd(&buf, 12)?;
    Some((alloc.max(0) as u64, private.max(0) as u64))
}
