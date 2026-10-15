//! The volume under a scan: how big it is, how much is free, and on macOS how much is purgeable.
//!
//! Purgeable space (local Time Machine snapshots, caches the system will delete when it needs room) is
//! reported by the OS, not found by a scan. A scan of a whole volume adds up to "used" only after two
//! more lines are shown: purgeable, and "other" (system data, snapshots, other volumes sharing the APFS
//! container, and folders the scan was not allowed to read). [`VolumeInfo::account`] does that sum.

use std::io;
use std::path::Path;

/// Size and free space of the volume a path lives on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeInfo {
    /// The volume's capacity in bytes.
    pub total: u64,
    /// Bytes free right now, not counting purgeable space.
    pub free: u64,
    /// Bytes the system would free on demand (macOS only, `None` elsewhere).
    pub purgeable: Option<u64>,
    /// True when the scanned path is the top of this volume, so the totals can be compared.
    pub covers_volume: bool,
}

/// Where the bytes of a whole-volume scan went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accounting {
    /// Bytes found by the scan.
    pub scanned: u64,
    /// Bytes the system calls purgeable.
    pub purgeable: u64,
    /// Used bytes the scan did not find: system data, snapshots, unreadable folders, sibling volumes.
    pub other: u64,
    /// Free bytes.
    pub free: u64,
    /// Capacity.
    pub total: u64,
}

impl VolumeInfo {
    /// Bytes in use on the volume (capacity minus free, purgeable space counted as used).
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    /// Split the used space into found, purgeable and other. `None` when the scan did not cover the
    /// whole volume (the numbers would not be comparable).
    pub fn account(&self, scanned: u64) -> Option<Accounting> {
        if !self.covers_volume {
            return None;
        }
        let purgeable = self
            .purgeable
            .unwrap_or(0)
            .min(self.used().saturating_sub(scanned));
        let other = self
            .used()
            .saturating_sub(scanned)
            .saturating_sub(purgeable);
        Some(Accounting {
            scanned,
            purgeable,
            other,
            free: self.free,
            total: self.total,
        })
    }
}

/// Volume facts for the volume `path` is on.
pub fn info(path: &Path) -> io::Result<VolumeInfo> {
    let (total, free) = capacity(path)?;
    let covers = covers_volume(path);
    #[cfg(target_os = "macos")]
    {
        if let Some((t, avail, important)) = mac::capacities(path) {
            return Ok(VolumeInfo {
                total: t,
                free: avail,
                purgeable: Some(important.saturating_sub(avail)),
                covers_volume: covers,
            });
        }
    }
    Ok(VolumeInfo {
        total,
        free,
        purgeable: None,
        covers_volume: covers,
    })
}

fn covers_volume(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return true;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(path), std::fs::metadata(parent)) {
            return a.dev() != b.dev();
        }
    }
    let _ = parent;
    false
}

#[cfg(unix)]
fn capacity(path: &Path) -> io::Result<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    imp::statvfs(&c)
}

#[cfg(windows)]
fn capacity(path: &Path) -> io::Result<(u64, u64)> {
    imp::disk_space(path)
}

#[cfg(not(any(unix, windows)))]
fn capacity(_path: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// True when `path` is the mount point of a network filesystem (NFS, SMB, AFP, WebDAV, FUSE network).
pub(crate) fn is_network_mount(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = CString::new(path.as_os_str().as_bytes()) {
            return imp::is_network(&c);
        }
    }
    let _ = path;
    false
}

/// Every mounted filesystem as (mount point, is a network filesystem). macOS only: its bulk directory call
/// reports a mount point as the folder it covers, so the scanner needs the list to know where another
/// volume starts. Empty elsewhere (Linux and Windows report the mounted volume's own identity).
pub(crate) fn mount_points() -> Vec<(std::path::PathBuf, bool)> {
    #[cfg(target_os = "macos")]
    return imp::mounts();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

#[cfg(unix)]
mod imp {
    #![allow(unsafe_code)]
    use std::ffi::CStr;
    use std::io;

    pub(super) fn statvfs(path: &CStr) -> io::Result<(u64, u64)> {
        // SAFETY: valid NUL-terminated path and a zeroed out-buffer.
        unsafe {
            let mut s: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(path.as_ptr(), &mut s) != 0 {
                return Err(io::Error::last_os_error());
            }
            let frsize = if s.f_frsize > 0 {
                s.f_frsize as u64
            } else {
                s.f_bsize as u64
            };
            Ok((s.f_blocks as u64 * frsize, s.f_bavail as u64 * frsize))
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn mounts() -> Vec<(std::path::PathBuf, bool)> {
        use std::os::unix::ffi::OsStrExt;
        // MNT_NOWAIT: answer from the kernel's cache, never wait on a network share that has gone away.
        // SAFETY: a null buffer asks only for the count; the second call gets room for that many
        // statfs records and the kernel fills at most that many.
        unsafe {
            let n = libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT);
            if n <= 0 {
                return Vec::new();
            }
            let mut buf: Vec<libc::statfs> = vec![std::mem::zeroed(); n as usize + 8];
            let bytes = (buf.len() * std::mem::size_of::<libc::statfs>()) as libc::c_int;
            let got = libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT);
            if got <= 0 {
                return Vec::new();
            }
            buf.truncate(got as usize);
            buf.iter()
                .map(|s| {
                    let on = CStr::from_ptr(s.f_mntonname.as_ptr()).to_bytes();
                    let ty = CStr::from_ptr(s.f_fstypename.as_ptr()).to_bytes();
                    (
                        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(on)),
                        matches!(
                            ty,
                            b"nfs" | b"smbfs" | b"afpfs" | b"webdav" | b"cifs" | b"ftp" | b"autofs"
                        ),
                    )
                })
                .collect()
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn is_network(path: &CStr) -> bool {
        // SAFETY: valid path, zeroed out-buffer; f_fstypename is a NUL-terminated array.
        unsafe {
            let mut s: libc::statfs = std::mem::zeroed();
            if libc::statfs(path.as_ptr(), &mut s) != 0 {
                return false;
            }
            let name = CStr::from_ptr(s.f_fstypename.as_ptr()).to_bytes();
            matches!(
                name,
                b"nfs" | b"smbfs" | b"afpfs" | b"webdav" | b"cifs" | b"ftp" | b"autofs"
            )
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn is_network(path: &CStr) -> bool {
        // SAFETY: valid path, zeroed out-buffer.
        unsafe {
            let mut s: libc::statfs = std::mem::zeroed();
            if libc::statfs(path.as_ptr(), &mut s) != 0 {
                return false;
            }
            // NFS, SMB, CIFS, SMB2, CEPH, AFS, 9P, FUSE (sshfs, rclone and friends are network by nature).
            matches!(
                s.f_type as u64,
                0x6969
                    | 0x517B
                    | 0xFF53_4D42
                    | 0xFE53_4D42
                    | 0x00C3_6400
                    | 0x5346_414F
                    | 0x0102_1997
                    | 0x6573_5546
            )
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(super) fn is_network(_path: &CStr) -> bool {
        false
    }
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    pub(super) fn disk_space(path: &Path) -> io::Result<(u64, u64)> {
        let mut w: Vec<u16> = path.as_os_str().encode_wide().collect();
        w.push(0);
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: `w` is NUL-terminated and the three out-pointers are valid for the call.
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(w.as_ptr()),
                Some(&mut avail),
                Some(&mut total),
                Some(&mut free),
            )
        }
        .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
        Ok((total, avail))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    #![allow(unsafe_code)]
    use std::path::Path;

    use objc2_foundation::{
        NSArray, NSNumber, NSString, NSURL, NSURLVolumeAvailableCapacityForImportantUsageKey,
        NSURLVolumeAvailableCapacityKey, NSURLVolumeTotalCapacityKey,
    };

    /// (total, available, available for important usage) from the same keys Finder reads. Important
    /// usage counts purgeable space as available.
    pub(super) fn capacities(path: &Path) -> Option<(u64, u64, u64)> {
        let text = path.to_str()?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        // SAFETY: these are Foundation's own constant keys, valid for the life of the process.
        let keys = unsafe {
            NSArray::from_slice(&[
                NSURLVolumeTotalCapacityKey,
                NSURLVolumeAvailableCapacityKey,
                NSURLVolumeAvailableCapacityForImportantUsageKey,
            ])
        };
        let dict = url.resourceValuesForKeys_error(&keys).ok()?;
        // SAFETY: the keys are the constants above.
        let get = |k: &objc2_foundation::NSURLResourceKey| -> Option<u64> {
            let v = dict.objectForKey(k)?;
            let n = v.downcast_ref::<NSNumber>()?;
            Some(n.longLongValue().max(0) as u64)
        };
        // SAFETY: Foundation constants.
        let (t, a, i) = unsafe {
            (
                get(NSURLVolumeTotalCapacityKey)?,
                get(NSURLVolumeAvailableCapacityKey)?,
                get(NSURLVolumeAvailableCapacityForImportantUsageKey)?,
            )
        };
        Some((t, a, i))
    }
}
