//! Open folders relative to their parent, and keep the descriptor while the children need it.
//!
//! Opening `/a/b/c/d/e` by path makes the kernel look up five names; with the parent descriptor in hand
//! `openat(parent, "e")` looks up one. On a million files in 80 thousand folders the saved lookups are a
//! large share of the work, and on APFS every lookup is a B-tree search once the vnode cache is cold.
//! Descriptors are a limited resource, so at most [`KEEP_MAX`] are alive at a time; past that the children
//! open by path, which is slower and always works.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Descriptors the scanner may hold open for children at once.
const KEEP_MAX: usize = 1024;

/// An open folder. Closing is automatic.
pub(crate) struct DirFd {
    fd: OwnedFd,
}

impl Drop for DirFd {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

const FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;

impl DirFd {
    /// Open `path`, relative to `parent` when given and when the name lookup is all that is needed.
    pub fn open(path: &Path, parent: Option<&DirFd>) -> io::Result<DirFd> {
        if let (Some(p), Some(name)) = (parent, path.file_name()) {
            if let Ok(c) = CString::new(name.as_bytes()) {
                // SAFETY: `p` is open, `c` is NUL-terminated; O_NOFOLLOW refuses a folder swapped for a link.
                let fd =
                    unsafe { libc::openat(p.fd.as_raw_fd(), c.as_ptr(), FLAGS | libc::O_NOFOLLOW) };
                if fd >= 0 {
                    return Ok(Self::wrap(fd));
                }
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::EMFILE)
                    && err.raw_os_error() != Some(libc::ENFILE)
                {
                    return Err(err);
                }
            }
        }
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: `c` is NUL-terminated for the call.
        let fd = unsafe { libc::open(c.as_ptr(), FLAGS) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self::wrap(fd))
    }

    fn wrap(fd: libc::c_int) -> DirFd {
        LIVE.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `fd` was just returned by open and nothing else owns it.
        DirFd {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        }
    }

    pub fn raw(&self) -> libc::c_int {
        self.fd.as_raw_fd()
    }

    /// Hand the descriptor to the children, or close it when too many are alive.
    pub fn keep(self) -> Option<DirFd> {
        if LIVE.load(Ordering::Relaxed) <= KEEP_MAX {
            Some(self)
        } else {
            None
        }
    }
}

/// Raise the soft limit on open files once, so many folders can stay open. Failure is harmless.
pub(crate) fn raise_limit() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: plain getrlimit/setrlimit on this process with a valid struct.
        unsafe {
            let mut lim = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) == 0 {
                let want = (KEEP_MAX as u64 * 2 + 256).min(lim.rlim_max as u64);
                #[cfg(target_os = "macos")]
                let want = want.min(10240);
                if (lim.rlim_cur as u64) < want {
                    lim.rlim_cur = want as libc::rlim_t;
                    let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &lim);
                }
            }
        }
    });
}
