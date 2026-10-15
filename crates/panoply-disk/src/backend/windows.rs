//! Windows: `GetFileInformationByHandleEx` with `FileIdExtdDirectoryInfo`, which is the documented front
//! of the `NtQueryDirectoryFile` call. One system call returns the name, attributes, length, allocation
//! size and 128-bit file ID of dozens of entries, with no handle per file.
//!
//! Junctions, symbolic links and mount points are reported as links and never entered. Compressed system
//! files that the OS marks with the WOF reparse tag (CompactOS) are real files and are counted by their
//! allocation size. Cloud placeholders (OneDrive) are files that cost nothing on this disk. The entry
//! parser is plain byte reading and runs in the unit tests on every platform.

#![cfg_attr(not(windows), allow(dead_code))]

use super::Entry;
use crate::tree::{Kind, flags};

const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_REPARSE_POINT: u32 = 0x400;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_OPEN: u32 = 0x4_0000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

const TAG_MOUNT_POINT: u32 = 0xA000_0003;
const TAG_WOF: u32 = 0x8000_0017;

/// 256 KiB per call.
pub(crate) const BUF: usize = 256 * 1024;

const HEADER: usize = 88;

fn is_cloud_tag(tag: u32) -> bool {
    tag & 0xFFFF_0FFF == 0x9000_001A
}

/// Walk one buffer of `FILE_ID_EXTD_DIR_INFO` records.
pub(crate) fn parse_extd(buf: &[u8], sink: &mut dyn FnMut(&Entry<'_>)) {
    let mut off = 0usize;
    let mut name_utf8: Vec<u8> = Vec::with_capacity(512);
    loop {
        if off + HEADER > buf.len() {
            return;
        }
        let r = &buf[off..];
        let u32_at = |o: usize| u32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
        let i64_at = |o: usize| {
            i64::from_le_bytes([
                r[o],
                r[o + 1],
                r[o + 2],
                r[o + 3],
                r[o + 4],
                r[o + 5],
                r[o + 6],
                r[o + 7],
            ])
        };
        let next = u32_at(0) as usize;
        let apparent = i64_at(40).max(0) as u64;
        let alloc = i64_at(48).max(0) as u64;
        let attrs = u32_at(56);
        let name_len = u32_at(60) as usize;
        let tag = u32_at(68);
        let id_lo = i64_at(72) as u64;
        let id_hi = i64_at(80) as u64;
        if HEADER + name_len <= r.len() {
            name_utf8.clear();
            let units = r[HEADER..HEADER + name_len]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]));
            for ch in char::decode_utf16(units) {
                let mut tmp = [0u8; 4];
                name_utf8
                    .extend_from_slice(ch.unwrap_or('\u{FFFD}').encode_utf8(&mut tmp).as_bytes());
            }
            let dot = name_utf8 == b"." || name_utf8 == b"..";
            if !dot && !name_utf8.is_empty() {
                let reparse = attrs & ATTR_REPARSE_POINT != 0;
                let is_dir_attr = attrs & ATTR_DIRECTORY != 0;
                let mut fl = 0u8;
                let mut mount = false;
                let kind = if reparse && tag == TAG_WOF {
                    Kind::File
                } else if reparse && is_cloud_tag(tag) {
                    if !is_dir_attr {
                        fl |= flags::DATALESS;
                    }
                    if is_dir_attr { Kind::Dir } else { Kind::File }
                } else if reparse {
                    mount = tag == TAG_MOUNT_POINT;
                    Kind::Symlink
                } else if is_dir_attr {
                    Kind::Dir
                } else {
                    Kind::File
                };
                if attrs & (ATTR_OFFLINE | ATTR_RECALL_ON_OPEN | ATTR_RECALL_ON_DATA_ACCESS) != 0
                    && kind == Kind::File
                {
                    fl |= flags::DATALESS;
                }
                sink(&Entry {
                    name: &name_utf8,
                    kind,
                    alloc: if kind == Kind::Dir { 0 } else { alloc },
                    apparent,
                    dev: 0,
                    id: id_lo,
                    id_hi,
                    // The directory record carries no link count: 0 means "unknown, remember the ID".
                    nlink: if kind == Kind::File { 0 } else { 1 },
                    clone_id: 0,
                    clone_shared: 0,
                    flags: fl,
                    is_mount_hint: mount,
                });
            }
        }
        if next == 0 {
            return;
        }
        off += next;
    }
}

#[cfg(windows)]
pub(crate) use imp::Extd;

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdExtdDirectoryInfo, FileIdExtdDirectoryRestartInfo,
        GetFileInformationByHandleEx, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    use super::parse_extd;
    use crate::backend::{Backend, DirFd, Entry, RootInfo, Scratch, SkipReason, portable};

    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PATH_NOT_FOUND: i32 = 3;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_NO_MORE_FILES: i32 = 18;

    pub(crate) struct Extd;

    fn reason(code: i32) -> SkipReason {
        match code {
            ERROR_ACCESS_DENIED => SkipReason::Permission,
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => SkipReason::Gone,
            _ => SkipReason::Other,
        }
    }

    fn code(e: &windows::core::Error) -> i32 {
        (e.code().0 & 0xFFFF) as i32
    }

    /// `\\?\` form so paths past 260 characters open.
    fn wide(path: &Path) -> Vec<u16> {
        let mut v: Vec<u16> = path.as_os_str().encode_wide().collect();
        let prefixed = v.starts_with(&[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]);
        let unc = v.starts_with(&[b'\\' as u16, b'\\' as u16]);
        if !prefixed && !unc {
            let mut p: Vec<u16> = r"\\?\".encode_utf16().collect();
            p.append(&mut v);
            v = p;
        }
        v.push(0);
        v
    }

    impl Backend for Extd {
        fn name(&self) -> &'static str {
            "GetFileInformationByHandleEx (FileIdExtdDirectoryInfo)"
        }

        fn root_info(&self, path: &Path) -> io::Result<RootInfo> {
            portable::Portable.root_info(path)
        }

        fn read_dir(
            &self,
            dir: &Path,
            _parent: Option<&DirFd>,
            scratch: &mut Scratch,
            sink: &mut dyn FnMut(&Entry<'_>),
        ) -> Result<Option<DirFd>, SkipReason> {
            let w = wide(dir);
            // SAFETY: `w` is a NUL-terminated wide string that outlives the call.
            let h: HANDLE = unsafe {
                CreateFileW(
                    PCWSTR(w.as_ptr()),
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    None,
                )
            }
            .map_err(|e| reason(code(&e)))?;
            let buf = scratch.bytes();
            let mut class = FileIdExtdDirectoryRestartInfo;
            let res = loop {
                // SAFETY: `h` is an open directory handle and `buf` is writable for buf.len() bytes
                // (8-byte aligned by Scratch).
                let r = unsafe {
                    GetFileInformationByHandleEx(
                        h,
                        class,
                        buf.as_mut_ptr().cast(),
                        buf.len() as u32,
                    )
                };
                class = FileIdExtdDirectoryInfo;
                match r {
                    Ok(()) => parse_extd(buf, sink),
                    Err(e) if code(&e) == ERROR_NO_MORE_FILES => break Ok(()),
                    Err(e) => break Err(reason(code(&e))),
                }
            };
            // SAFETY: `h` was opened above and is closed once.
            let _ = unsafe { CloseHandle(h) };
            res.map(|()| None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        name: &str,
        attrs: u32,
        tag: u32,
        eof: i64,
        alloc: i64,
        id: (u64, u64),
        last: bool,
    ) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let name_bytes = units.len() * 2;
        let mut len = HEADER + name_bytes;
        len = len.div_ceil(8) * 8;
        let mut r = vec![0u8; len];
        let next = if last { 0 } else { len as u32 };
        r[0..4].copy_from_slice(&next.to_le_bytes());
        r[40..48].copy_from_slice(&eof.to_le_bytes());
        r[48..56].copy_from_slice(&alloc.to_le_bytes());
        r[56..60].copy_from_slice(&attrs.to_le_bytes());
        r[60..64].copy_from_slice(&(name_bytes as u32).to_le_bytes());
        r[68..72].copy_from_slice(&tag.to_le_bytes());
        r[72..80].copy_from_slice(&id.0.to_le_bytes());
        r[80..88].copy_from_slice(&id.1.to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            r[HEADER + i * 2..HEADER + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    #[test]
    fn parses_files_dirs_links_and_skips_dots() {
        let mut buf = Vec::new();
        buf.extend(record(".", 0x10, 0, 0, 0, (1, 0), false));
        buf.extend(record("..", 0x10, 0, 0, 0, (2, 0), false));
        buf.extend(record("Straße.txt", 0x20, 0, 1_000, 4096, (7, 1), false));
        buf.extend(record("docs", 0x10, 0, 0, 0, (8, 0), false));
        buf.extend(record(
            "junction",
            0x410,
            TAG_MOUNT_POINT,
            0,
            0,
            (9, 0),
            false,
        ));
        buf.extend(record(
            "system.dll",
            0x420,
            TAG_WOF,
            9_000_000,
            3_000_000,
            (10, 0),
            false,
        ));
        buf.extend(record(
            "cloud.bin",
            0x420 | ATTR_RECALL_ON_DATA_ACCESS,
            0x9000_601A,
            5_000_000,
            0,
            (11, 0),
            true,
        ));
        let mut seen: Vec<(String, Kind, u64, u8, bool, u32)> = Vec::new();
        parse_extd(&buf, &mut |e| {
            seen.push((
                String::from_utf8(e.name.to_vec()).unwrap(),
                e.kind,
                e.alloc,
                e.flags,
                e.is_mount_hint,
                e.nlink,
            ));
        });
        assert_eq!(seen.len(), 5);
        assert_eq!(
            seen[0],
            ("Straße.txt".into(), Kind::File, 4096, 0, false, 0)
        );
        assert_eq!(seen[1].1, Kind::Dir);
        assert_eq!(seen[2].1, Kind::Symlink);
        assert!(
            seen[2].4,
            "a mount point is a hint that it is another volume"
        );
        assert_eq!(
            seen[3],
            ("system.dll".into(), Kind::File, 3_000_000, 0, false, 0)
        );
        assert_eq!(seen[4].1, Kind::File);
        assert_ne!(seen[4].3 & flags::DATALESS, 0);
        assert_eq!(seen[4].2, 0);
    }

    #[test]
    fn a_truncated_buffer_does_not_panic() {
        let full = record("a.txt", 0x20, 0, 10, 4096, (1, 0), true);
        for cut in 0..full.len() {
            parse_extd(&full[..cut], &mut |_| {});
        }
    }
}
