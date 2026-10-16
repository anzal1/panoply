//! Linux: `getdents64` for the names and types, then `statx` relative to the open folder for the block
//! count, link count and file ID. Allocated size is `stx_blocks * 512`, so a sparse file costs what it
//! occupies. Btrfs and XFS reflinks are not detected (no cheap call reports them); the report says so.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Backend, DirFd, Entry, RootInfo, Scratch, SkipReason};
use crate::tree::Kind;

pub(crate) const BUF: usize = 128 * 1024;

pub(crate) struct Getdents;

const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;

fn dev_of(maj: u32, min: u32) -> u64 {
    ((maj as u64) << 32) | min as u64
}

fn kind_of(mode: u32) -> Kind {
    match mode & S_IFMT {
        S_IFDIR => Kind::Dir,
        S_IFREG => Kind::File,
        S_IFLNK => Kind::Symlink,
        _ => Kind::Other,
    }
}

fn statx(dirfd: libc::c_int, name: *const libc::c_char) -> Option<libc::statx> {
    // SAFETY: zeroed statx is a valid out-buffer; `name` is NUL-terminated and outlives the call.
    unsafe {
        let mut sx: libc::statx = std::mem::zeroed();
        let r = libc::statx(
            dirfd,
            name,
            libc::AT_SYMLINK_NOFOLLOW | libc::AT_NO_AUTOMOUNT | libc::AT_STATX_DONT_SYNC,
            libc::STATX_TYPE
                | libc::STATX_SIZE
                | libc::STATX_BLOCKS
                | libc::STATX_INO
                | libc::STATX_NLINK,
            &mut sx,
        );
        if r == 0 { Some(sx) } else { None }
    }
}

impl Backend for Getdents {
    fn name(&self) -> &'static str {
        "getdents64 + statx"
    }

    fn root_info(&self, path: &Path) -> io::Result<RootInfo> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // Follows a symlink on the root, as the user asked for the place it points to.
        // SAFETY: valid path, zeroed out-buffer.
        let sx = unsafe {
            let mut sx: libc::statx = std::mem::zeroed();
            if libc::statx(
                libc::AT_FDCWD,
                c.as_ptr(),
                libc::AT_STATX_DONT_SYNC,
                libc::STATX_TYPE | libc::STATX_BLOCKS | libc::STATX_INO,
                &mut sx,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
            sx
        };
        Ok(RootInfo {
            kind: kind_of(sx.stx_mode as u32),
            dev: dev_of(sx.stx_dev_major, sx.stx_dev_minor),
            alloc: sx.stx_blocks * 512,
        })
    }

    fn read_dir(
        &self,
        dir: &Path,
        parent: Option<&DirFd>,
        scratch: &mut Scratch,
        sink: &mut dyn FnMut(&Entry<'_>),
    ) -> Result<Option<DirFd>, SkipReason> {
        let dfd = DirFd::open(dir, parent).map_err(|e| SkipReason::from_io(&e))?;
        let fd = dfd.raw();
        let buf = scratch.bytes();
        let res = loop {
            // SAFETY: fd is open and `buf` is writable for its whole length.
            let n = unsafe { libc::syscall(libc::SYS_getdents64, fd, buf.as_mut_ptr(), buf.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break Err(SkipReason::from_io(&e));
            }
            if n == 0 {
                break Ok(());
            }
            let mut off = 0usize;
            let n = n as usize;
            while off + 19 <= n {
                // struct linux_dirent64 { u64 ino; i64 off; u16 reclen; u8 type; char name[]; }
                let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
                if reclen < 20 || off + reclen > n {
                    break;
                }
                let name_start = off + 19;
                let name_end = buf[name_start..off + reclen]
                    .iter()
                    .position(|b| *b == 0)
                    .map_or(off + reclen, |p| name_start + p);
                let name = &buf[name_start..name_end];
                let skip = name == b"." || name == b"..";
                if !skip {
                    // The name is NUL-terminated inside the record: no copy needed for statx.
                    let ptr = buf[name_start..].as_ptr().cast::<libc::c_char>();
                    if let Some(sx) = statx(fd, ptr) {
                        let mode = sx.stx_mode as u32;
                        let kind = kind_of(mode);
                        sink(&Entry {
                            name,
                            kind,
                            alloc: sx.stx_blocks * 512,
                            apparent: sx.stx_size,
                            dev: dev_of(sx.stx_dev_major, sx.stx_dev_minor),
                            id: sx.stx_ino,
                            id_hi: 0,
                            nlink: if kind == Kind::File { sx.stx_nlink } else { 1 },
                            clone_id: 0,
                            clone_shared: 0,
                            flags: 0,
                            is_mount_hint: false,
                        });
                    }
                }
                off += reclen;
            }
        };
        res.map(|()| dfd.keep())
    }
}
