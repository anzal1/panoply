//! File system change events from FSEvents (CoreServices).
//!
//! One stream watches a list of folders and reports every change below them, coalesced by the kernel
//! and delivered on a private dispatch queue after `latency` seconds of quiet. Each event carries a
//! monotonically increasing event id; a caller that stores the last id it saw can start the next stream
//! from it and receive what changed while it was not running (the history replay), which is what keeps
//! an index correct across restarts and sleep.
//!
//! The wrapper declares the handful of C functions it needs itself, so there is no Objective-C and no
//! run loop: the stream is attached to a serial dispatch queue. Paths arrive as C strings (no
//! `UseCFTypes`). The callback runs on that queue, so it must return quickly; the Panoply updater just
//! forwards the events over a channel and sleeps.

use std::ffi::{CStr, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// A subfolder tree must be rescanned: events were coalesced or dropped below this path.
pub const MUST_SCAN_SUBDIRS: u32 = 0x1;
/// Events were dropped by the user space buffer.
pub const USER_DROPPED: u32 = 0x2;
/// Events were dropped by the kernel.
pub const KERNEL_DROPPED: u32 = 0x4;
/// Marks the end of the history replay; live events follow.
pub const HISTORY_DONE: u32 = 0x10;
/// A watched root was moved, renamed or deleted.
pub const ROOT_CHANGED: u32 = 0x20;
/// The item was created.
pub const ITEM_CREATED: u32 = 0x100;
/// The item was removed.
pub const ITEM_REMOVED: u32 = 0x200;
/// The item was renamed (moved).
pub const ITEM_RENAMED: u32 = 0x800;
/// The item is a folder.
pub const ITEM_IS_DIR: u32 = 0x20000;

const CREATE_NO_DEFER: u32 = 0x2;
const CREATE_WATCH_ROOT: u32 = 0x4;
const CREATE_FILE_EVENTS: u32 = 0x10;
const SINCE_NOW: u64 = u64::MAX;
const UTF8: u32 = 0x0800_0100;

/// One reported change.
#[derive(Clone, Debug)]
pub struct FsEvent {
    /// The path that changed (the real path, symlinks resolved).
    pub path: PathBuf,
    /// A bit set of the `ITEM_*` and other flags above.
    pub flags: u32,
    /// The event id; store the largest one you have handled.
    pub id: u64,
}

type Callback = Box<dyn Fn(Vec<FsEvent>) + Send + 'static>;

#[repr(C)]
struct StreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> *const c_void>,
}

type RawCallback =
    extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: RawCallback,
        context: *const StreamContext,
        paths: *const c_void,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
    fn FSEventsGetCurrentEventId() -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: [u8; 0];
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        is_external: u8,
    ) -> *const c_void;
    fn CFArrayCreate(
        alloc: *const c_void,
        values: *const *const c_void,
        count: isize,
        callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(obj: *const c_void);
}

unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_release(obj: *mut c_void);
}

extern "C" fn trampoline(
    _stream: *const c_void,
    info: *mut c_void,
    n: usize,
    paths: *mut c_void,
    flags: *const u32,
    ids: *const u64,
) {
    if info.is_null() || paths.is_null() || flags.is_null() || ids.is_null() {
        return;
    }
    // SAFETY: `info` is the pointer to the `Callback` box that `start` leaked into the stream context and
    // frees only after the stream is stopped and invalidated, so it is alive for every call. FSEvents
    // calls the callback serially on our queue, so the shared borrow is the only one.
    let cb = unsafe { &*(info as *const Callback) };
    // SAFETY: without `UseCFTypes`, `paths` is an array of `n` NUL terminated C strings, `flags` and
    // `ids` are arrays of `n` values; all three stay valid until the callback returns.
    let (paths, flags, ids) = unsafe {
        (
            std::slice::from_raw_parts(paths as *const *const c_char, n),
            std::slice::from_raw_parts(flags, n),
            std::slice::from_raw_parts(ids, n),
        )
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if paths[i].is_null() {
            continue;
        }
        // SAFETY: a non-null entry of the paths array is a valid NUL terminated string (see above).
        let bytes = unsafe { CStr::from_ptr(paths[i]) }.to_bytes();
        out.push(FsEvent {
            path: PathBuf::from(std::ffi::OsStr::from_bytes(bytes)),
            flags: flags[i],
            id: ids[i],
        });
    }
    // A panic must not unwind into C.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(out)));
}

/// A running FSEvents stream. Dropping it stops the stream and releases everything.
pub struct EventStream {
    stream: *mut c_void,
    queue: *mut c_void,
    callback: *mut Callback,
}

// SAFETY: the stream and queue are only touched from `start` and `Drop`; FSEvents' own functions for
// stop and release are safe to call from any thread, and the callback box is `Send`.
unsafe impl Send for EventStream {}

/// The current FSEvents event id: what to store when nothing has been replayed yet.
pub fn current_event_id() -> u64 {
    // SAFETY: plain C function with no arguments.
    unsafe { FSEventsGetCurrentEventId() }
}

/// Starts watching `paths`. `since` is the last event id the caller handled (events after it are
/// replayed first, ending with an event flagged [`HISTORY_DONE`]), or `None` to receive only new events.
/// `latency` is how long the kernel waits to coalesce events before calling `callback`.
pub fn start(
    paths: &[&Path],
    since: Option<u64>,
    latency: f64,
    callback: impl Fn(Vec<FsEvent>) + Send + 'static,
) -> Result<EventStream, Error> {
    if paths.is_empty() {
        return Err(Error::new("no folders to watch"));
    }
    let mut strings: Vec<*const c_void> = Vec::new();
    for p in paths {
        let b = p.as_os_str().as_bytes();
        // SAFETY: `b` is a valid readable slice of the stated length; a null allocator means the default.
        let s = unsafe {
            CFStringCreateWithBytes(std::ptr::null(), b.as_ptr(), b.len() as isize, UTF8, 0)
        };
        if s.is_null() {
            for s in strings {
                // SAFETY: each pointer was returned by a Create function above and is released once.
                unsafe { CFRelease(s) };
            }
            return Err(Error::new("a watched path is not valid UTF-8"));
        }
        strings.push(s);
    }
    // SAFETY: `strings` holds `len` valid CFStrings; the type callbacks make the array retain them.
    let array = unsafe {
        CFArrayCreate(
            std::ptr::null(),
            strings.as_ptr(),
            strings.len() as isize,
            std::ptr::addr_of!(kCFTypeArrayCallBacks).cast(),
        )
    };
    for s in strings {
        // SAFETY: the array retained its own reference, so ours is released once.
        unsafe { CFRelease(s) };
    }
    if array.is_null() {
        return Err(Error::new("cannot build the path list"));
    }
    let boxed: *mut Callback = Box::into_raw(Box::new(Box::new(callback) as Callback));
    let ctx = StreamContext {
        version: 0,
        info: boxed.cast(),
        retain: None,
        release: None,
        copy_description: None,
    };
    // SAFETY: all pointers are valid for the call; FSEvents copies the context and the path array.
    let stream = unsafe {
        FSEventStreamCreate(
            std::ptr::null(),
            trampoline,
            &ctx,
            array,
            since.unwrap_or(SINCE_NOW),
            latency,
            CREATE_FILE_EVENTS | CREATE_NO_DEFER | CREATE_WATCH_ROOT,
        )
    };
    // SAFETY: `array` was created above and the stream holds its own copy.
    unsafe { CFRelease(array) };
    if stream.is_null() {
        // SAFETY: the box was leaked above and nothing else holds it.
        drop(unsafe { Box::from_raw(boxed) });
        return Err(Error::new("cannot create the file event stream"));
    }
    // SAFETY: the label is a valid NUL terminated string; a null attribute makes a serial queue.
    let queue =
        unsafe { dispatch_queue_create(c"panoply.find.fsevents".as_ptr(), std::ptr::null()) };
    // SAFETY: `stream` and `queue` are valid objects created above.
    let ok = unsafe {
        FSEventStreamSetDispatchQueue(stream, queue);
        FSEventStreamStart(stream) != 0
    };
    if !ok {
        // SAFETY: undo exactly what was created above, in reverse order.
        unsafe {
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
            dispatch_release(queue);
            drop(Box::from_raw(boxed));
        }
        return Err(Error::new("cannot start the file event stream"));
    }
    Ok(EventStream {
        stream,
        queue,
        callback: boxed,
    })
}

impl Drop for EventStream {
    fn drop(&mut self) {
        // SAFETY: the stream was started and is stopped, invalidated and released once, in that order;
        // after `Invalidate` no further callback runs, so the callback box and the queue can be freed.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
            drop(Box::from_raw(self.callback));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn reports_a_new_file_and_stops_cleanly() {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("panoply-fsev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (tx, rx) = mpsc::channel();
        let stream = start(&[root.as_path()], None, 0.05, move |ev| {
            let _ = tx.send(ev);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(root.join("hello world.txt"), b"x").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while Instant::now() < deadline && !seen {
            if let Ok(batch) = rx.recv_timeout(Duration::from_millis(200)) {
                seen = batch
                    .iter()
                    .any(|e| e.path.ends_with("hello world.txt") && e.flags & ITEM_CREATED != 0);
            }
        }
        drop(stream);
        let _ = std::fs::remove_dir_all(&root);
        assert!(seen, "no creation event arrived within 5 s");
    }

    #[test]
    fn event_ids_only_go_up() {
        let a = current_event_id();
        std::fs::write(
            std::env::temp_dir().join(format!("panoply-fsev-id-{}", std::process::id())),
            b"x",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(current_event_id() >= a);
    }
}
