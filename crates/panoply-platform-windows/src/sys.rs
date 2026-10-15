//! The only place with `unsafe`: handles, overlapped I/O and the Win32 calls, with safe signatures.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES,
    FILE_NOTIFY_CHANGE_CREATION, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdType, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, OPEN_EXISTING, OpenFileById, ReadDirectoryChangesW,
    VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};

const GENERIC_READ: u32 = 0x8000_0000;

/// `ERROR_NOTIFY_ENUM_DIR`: the change buffer overflowed.
pub(crate) const ERROR_NOTIFY_ENUM_DIR: i32 = 1022;
/// `ERROR_JOURNAL_ENTRY_DELETED`: our position fell off the end of the journal.
pub(crate) const ERROR_JOURNAL_ENTRY_DELETED: i32 = 0x49D;

fn wide(p: &OsStr) -> Vec<u16> {
    p.encode_wide().chain(std::iter::once(0)).collect()
}

/// An owned kernel handle, closed on drop.
pub(crate) struct Handle(HANDLE);

// SAFETY: a kernel handle is a process-wide token, valid on any thread; the calls that use it are
// thread safe at the kernel. This type has no interior pointers into Rust memory.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a handle this value owns and closes exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

fn check(h: HANDLE) -> io::Result<Handle> {
    if h == INVALID_HANDLE_VALUE || h.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(h))
    }
}

/// Opens a folder for listing and change notification. `overlapped` is for `ReadDirectoryChangesW`.
pub(crate) fn open_dir(path: &Path, overlapped: bool) -> io::Result<Handle> {
    let w = wide(path.as_os_str());
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
    if overlapped {
        flags |= FILE_FLAG_OVERLAPPED;
    }
    // SAFETY: `w` is a NUL terminated UTF-16 string that outlives the call; the security attributes
    // and template handle are null, which the API allows.
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    check(h)
}

/// Opens a volume device (`\\.\C:`) for reading its change journal.
pub(crate) fn open_volume(device: &str) -> io::Result<Handle> {
    let w = wide(OsStr::new(device));
    // SAFETY: as in `open_dir`.
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    check(h)
}

/// The full path of an open file or folder, with the `\\?\` prefix still on.
pub(crate) fn final_path(h: &Handle) -> io::Result<String> {
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: `buf` is valid for `buf.len()` u16 writes and the handle is open.
        let n = unsafe {
            GetFinalPathNameByHandleW(h.0, buf.as_mut_ptr(), buf.len() as u32, VOLUME_NAME_DOS)
        } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            return Ok(String::from_utf16_lossy(&buf[..n]));
        }
        // Too small: `n` is the size needed, including the NUL.
        buf.resize(n + 1, 0);
    }
}

/// The 64 bit file reference number of an open file or folder.
pub(crate) fn file_id(h: &Handle) -> io::Result<u64> {
    // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid plain-data value.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid out pointer for the call and the handle is open.
    if unsafe { GetFileInformationByHandle(h.0, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
}

/// Opens a file or folder by its reference number, relative to an open handle on its volume.
pub(crate) fn open_by_id(volume: &Handle, id: u64) -> io::Result<Handle> {
    let desc = FILE_ID_DESCRIPTOR {
        dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: FileIdType,
        Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: id as i64 },
    };
    // SAFETY: `desc` is a fully initialised descriptor that outlives the call, the volume handle is open
    // and the security attributes are null.
    let h = unsafe {
        OpenFileById(
            volume.0,
            &desc,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            FILE_FLAG_BACKUP_SEMANTICS,
        )
    };
    check(h)
}

/// A manual reset event: the stop signal, and the completion event of one I/O operation.
pub(crate) struct Signal(HANDLE);

// SAFETY: an event handle may be signalled and waited on from any thread.
unsafe impl Send for Signal {}
// SAFETY: as above; SetEvent and WaitForMultipleObjects are thread safe.
unsafe impl Sync for Signal {}

impl Signal {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name are allowed; manual reset, initially not signalled.
        let h = unsafe { CreateEventW(null(), 1, 0, null()) };
        if h.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Signal(h))
        }
    }

    /// Wakes every wait on this signal, for good.
    pub(crate) fn set(&self) {
        // SAFETY: the event handle is owned by `self` and open.
        unsafe { SetEvent(self.0) };
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        // SAFETY: closed exactly once, by its owner.
        unsafe { CloseHandle(self.0) };
    }
}

/// One overlapped operation and the memory the kernel writes into while it runs. Dropping it cancels
/// and retires a pending operation first, so the buffer is never freed under the kernel. Declare it
/// after the [`Handle`] it runs on, so it is dropped first.
pub(crate) struct IoOp {
    // Boxed so the address handed to the kernel stays put when the IoOp moves.
    ov: Box<OVERLAPPED>,
    done: Signal,
    buf: Vec<u8>,
    input: Vec<u8>,
    pending: Option<HANDLE>,
}

// SAFETY: the raw handles inside are kernel tokens (see `Handle`), and the OVERLAPPED is only touched by
// the thread that owns this value.
unsafe impl Send for IoOp {}

impl IoOp {
    pub(crate) fn new(buf_len: usize) -> io::Result<Self> {
        let done = Signal::new()?;
        // SAFETY: an all-zero OVERLAPPED is the documented initial state.
        let mut ov: Box<OVERLAPPED> = Box::new(unsafe { std::mem::zeroed() });
        ov.hEvent = done.0;
        Ok(IoOp {
            ov,
            done,
            buf: vec![0u8; buf_len],
            input: Vec::new(),
            pending: None,
        })
    }

    fn arm(&mut self) {
        // SAFETY: the event is owned by `self.done`.
        unsafe { ResetEvent(self.done.0) };
        self.ov.Internal = 0;
        self.ov.InternalHigh = 0;
        self.ov.Anonymous.Anonymous.Offset = 0;
        self.ov.Anonymous.Anonymous.OffsetHigh = 0;
    }

    fn started(&mut self, h: &Handle, ok: i32) -> io::Result<()> {
        if ok != 0 {
            // Completed at once; the event is signalled and `wait` collects the result.
            self.pending = Some(h.0);
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
            self.pending = Some(h.0);
            Ok(())
        } else {
            Err(e)
        }
    }

    /// Starts a recursive `ReadDirectoryChangesW` on a handle opened with `overlapped`.
    pub(crate) fn dir_changes(&mut self, h: &Handle) -> io::Result<()> {
        self.arm();
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_ATTRIBUTES
            | FILE_NOTIFY_CHANGE_SIZE
            | FILE_NOTIFY_CHANGE_LAST_WRITE
            | FILE_NOTIFY_CHANGE_CREATION;
        // SAFETY: `self.buf` and `self.ov` live in `self` and are not touched until `wait` returns or
        // `Drop` cancels the call; the handle is open and overlapped; the byte count pointer may be
        // null for an overlapped call and no completion routine is given.
        let ok = unsafe {
            ReadDirectoryChangesW(
                h.0,
                self.buf.as_mut_ptr().cast(),
                self.buf.len() as u32,
                1,
                filter,
                null_mut(),
                &mut *self.ov,
                None,
            )
        };
        self.started(h, ok)
    }

    /// Starts `DeviceIoControl` with `input`, output into this operation's buffer.
    pub(crate) fn ioctl(&mut self, h: &Handle, code: u32, input: &[u8]) -> io::Result<()> {
        self.arm();
        self.input.clear();
        self.input.extend_from_slice(input);
        // SAFETY: input and output buffers live in `self` until the operation is retired; the handle is
        // open and overlapped; the byte count pointer may be null for an overlapped call.
        let ok = unsafe {
            DeviceIoControl(
                h.0,
                code,
                self.input.as_ptr().cast(),
                self.input.len() as u32,
                self.buf.as_mut_ptr().cast(),
                self.buf.len() as u32,
                null_mut(),
                &mut *self.ov,
            )
        };
        self.started(h, ok)
    }

    /// Waits for the operation or for `stop`. `Ok(Some(n))`: it finished with `n` bytes in
    /// [`IoOp::bytes`]. `Ok(None)`: stop was signalled and the operation is cancelled. `Err`: the
    /// operation failed.
    pub(crate) fn wait(&mut self, stop: &Signal) -> io::Result<Option<usize>> {
        let Some(h) = self.pending else {
            return Err(io::Error::other("no operation in flight"));
        };
        let handles = [self.done.0, stop.0];
        // SAFETY: both handles are open events and the array has two entries.
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if r == WAIT_OBJECT_0 {
            self.pending = None;
            let mut n = 0u32;
            // SAFETY: the operation on `h` using `self.ov` has completed (its event was signalled).
            let ok = unsafe { GetOverlappedResult(h, &*self.ov, &mut n, 0) };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(Some(n as usize))
            }
        } else if r == WAIT_OBJECT_0 + 1 {
            self.retire();
            Ok(None)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// The first `n` bytes the last operation wrote.
    pub(crate) fn bytes(&self, n: usize) -> &[u8] {
        &self.buf[..n.min(self.buf.len())]
    }

    /// Cancels the pending operation and waits until the kernel is done with the buffer.
    fn retire(&mut self) {
        if let Some(h) = self.pending.take() {
            let mut n = 0u32;
            // SAFETY: `h` is the handle the operation was started on and `self.ov` identifies it; the
            // second call blocks until the kernel has finished with `self.buf` and `self.ov`, so both
            // may be freed afterwards.
            unsafe {
                CancelIoEx(h, &*self.ov);
                GetOverlappedResult(h, &*self.ov, &mut n, 1);
            }
        }
    }
}

impl Drop for IoOp {
    fn drop(&mut self) {
        self.retire();
    }
}
