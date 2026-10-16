//! The watcher: picks the USN journal or `ReadDirectoryChangesW` per root and runs the workers.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use windows_sys::Win32::System::Ioctl::{FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL};

use crate::notify::parse_notices;
use crate::paths::{rebase, strip_verbatim, volume_device};
use crate::sys::{self, Handle, IoOp, Signal};
use crate::usn::{
    JournalData, WANTED_REASONS, encode_read_request, parse_journal_data, parse_records,
};
use crate::{Backend, Error, Event, Options};

type Callback = Arc<dyn Fn(Event) + Send + Sync + 'static>;

/// A running watcher. Dropping it stops every worker and waits for them.
pub struct Watcher {
    stop: Arc<Signal>,
    threads: Vec<JoinHandle<()>>,
    backends: Vec<(PathBuf, Backend)>,
}

impl Watcher {
    /// What reports changes for each root, as chosen at start.
    pub fn backends(&self) -> &[(PathBuf, Backend)] {
        &self.backends
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.set();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// A watched root: the path as the index knows it, and the path the OS reports for it.
#[derive(Clone)]
struct Root {
    orig: PathBuf,
    resolved: String,
}

/// Starts watching `roots` and calls `callback` from worker threads for every change. The callback must
/// return quickly; the Panoply updater only forwards the event over a channel.
pub fn start(
    roots: &[PathBuf],
    callback: impl Fn(Event) + Send + Sync + 'static,
) -> Result<Watcher, Error> {
    start_with(roots, Options::default(), callback)
}

/// Like [`start`], with explicit [`Options`].
pub fn start_with(
    roots: &[PathBuf],
    options: Options,
    callback: impl Fn(Event) + Send + Sync + 'static,
) -> Result<Watcher, Error> {
    let cb: Callback = Arc::new(callback);
    let stop = Arc::new(Signal::new()?);
    let mut groups: HashMap<String, Vec<Root>> = HashMap::new();
    let mut singles: Vec<Root> = Vec::new();
    for r in roots {
        let resolved = sys::open_dir(r, false)
            .and_then(|h| sys::final_path(&h))
            .map(|p| strip_verbatim(&p));
        match resolved {
            Ok(res) => {
                let root = Root {
                    orig: r.clone(),
                    resolved: res.clone(),
                };
                match volume_device(&res) {
                    Some(dev) if options.usn_journal => groups.entry(dev).or_default().push(root),
                    _ => singles.push(root),
                }
            }
            Err(e) => return Err(Error::new(format!("cannot open {}: {e}", r.display()))),
        }
    }
    let mut w = Watcher {
        stop: stop.clone(),
        threads: Vec::new(),
        backends: Vec::new(),
    };
    for (device, group) in groups {
        match UsnVolume::open(&device, &group, &stop) {
            Ok(vol) => {
                for r in &group {
                    w.backends.push((r.orig.clone(), Backend::UsnJournal));
                }
                let (stop, cb) = (stop.clone(), cb.clone());
                w.threads.push(spawn("panoply-find-usn", move || {
                    usn_worker(vol, &group, &stop, &cb);
                })?);
            }
            // Not NTFS, journal off, or no privilege: ReadDirectoryChangesW per root.
            Err(_) => singles.extend(group),
        }
    }
    for root in singles {
        let handle = sys::open_dir(&root.orig, true)?;
        w.backends
            .push((root.orig.clone(), Backend::DirectoryChanges));
        let (stop, cb) = (stop.clone(), cb.clone());
        w.threads.push(spawn("panoply-find-rdcw", move || {
            dir_changes_worker(handle, &root, &stop, &cb);
        })?);
    }
    Ok(w)
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>, Error> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map_err(|e| Error::new(e.to_string()))
}

/// An open volume with its journal.
struct UsnVolume {
    handle: Handle,
    journal: JournalData,
}

impl UsnVolume {
    fn query(handle: &Handle, stop: &Signal) -> std::io::Result<JournalData> {
        let mut op = IoOp::new(4096)?;
        op.ioctl(handle, FSCTL_QUERY_USN_JOURNAL, &[])?;
        let n = op
            .wait(stop)?
            .ok_or_else(|| std::io::Error::other("stopped"))?;
        parse_journal_data(op.bytes(n)).ok_or_else(|| std::io::Error::other("short journal reply"))
    }

    /// Opens the volume and proves a path can be turned back into a name: the root folder opened by its
    /// own reference number. Any failure (access denied, not NTFS, journal off) sends the roots to
    /// `ReadDirectoryChangesW`.
    fn open(device: &str, roots: &[Root], stop: &Signal) -> std::io::Result<Self> {
        let handle = sys::open_volume(device)?;
        let journal = Self::query(&handle, stop)?;
        let first = sys::open_dir(&roots[0].orig, false)?;
        let id = sys::file_id(&first)?;
        sys::open_by_id(&handle, id)?;
        Ok(UsnVolume { handle, journal })
    }
}

enum Ended {
    Stopped,
    Degrade,
}

fn rescan_all(roots: &[Root], cb: &Callback) {
    for r in roots {
        cb(Event::Rescan(r.orig.clone()));
    }
}

fn usn_worker(vol: UsnVolume, roots: &[Root], stop: &Signal, cb: &Callback) {
    if matches!(usn_loop(&vol, roots, stop, cb), Ended::Degrade) {
        // The journal went away under us. Report what may have been missed, then keep the roots fresh
        // with one directory watcher each; this thread waits for them.
        rescan_all(roots, cb);
        std::thread::scope(|s| {
            for root in roots {
                s.spawn(move || {
                    if let Ok(h) = sys::open_dir(&root.orig, true) {
                        dir_changes_worker(h, root, stop, cb);
                    }
                });
            }
        });
    }
}

fn usn_loop(vol: &UsnVolume, roots: &[Root], stop: &Signal, cb: &Callback) -> Ended {
    let Ok(mut op) = IoOp::new(64 * 1024) else {
        return Ended::Degrade;
    };
    let mut journal = vol.journal;
    let mut next = journal.next;
    // Folder reference number to path, so a burst in one folder resolves it once.
    let mut folders: HashMap<u64, Option<String>> = HashMap::new();
    loop {
        let req = encode_read_request(next, WANTED_REASONS, journal.id);
        if op.ioctl(&vol.handle, FSCTL_READ_USN_JOURNAL, &req).is_err() {
            return Ended::Degrade;
        }
        let n = match op.wait(stop) {
            Ok(Some(n)) => n,
            Ok(None) => return Ended::Stopped,
            Err(e) if e.raw_os_error() == Some(sys::ERROR_JOURNAL_ENTRY_DELETED) => {
                // The journal wrapped past our position: changes were lost. Take the new position first and
                // rescan after, so a change made between the two is read by the rescan or seen next round.
                match UsnVolume::query(&vol.handle, stop) {
                    Ok(j) => {
                        journal = j;
                        next = j.next;
                        folders.clear();
                        rescan_all(roots, cb);
                        continue;
                    }
                    Err(_) => return Ended::Degrade,
                }
            }
            Err(_) => return Ended::Degrade,
        };
        let parsed = parse_records(op.bytes(n));
        if let Some(new_next) = parsed.next
            && new_next > next
        {
            next = new_next;
        }
        if parsed.unusable > 0 || parsed.damaged {
            // Records we could not read are changes we cannot place: read the roots again.
            rescan_all(roots, cb);
            folders.clear();
        }
        let records = parsed.records;
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut drop_cache = false;
        for rec in &records {
            let parent = folders.entry(rec.parent).or_insert_with(|| {
                sys::open_by_id(&vol.handle, rec.parent)
                    .and_then(|h| sys::final_path(&h))
                    .ok()
                    .map(|p| strip_verbatim(&p))
            });
            if let Some(dir) = parent.as_deref() {
                let name = OsString::from_wide(&rec.name);
                let full = Path::new(dir).join(name);
                let full = full.to_string_lossy();
                for r in roots {
                    if let Some(p) = rebase(&r.resolved, &full, &r.orig)
                        && seen.insert(p.clone())
                    {
                        cb(Event::Changed(p));
                    }
                }
            }
            drop_cache |= rec.moves_paths();
        }
        if drop_cache {
            folders.clear();
        }
        // A cache that grows without bound on a busy volume is a leak.
        if folders.len() > 4096 {
            folders.clear();
        }
    }
}

fn dir_changes_worker(handle: Handle, root: &Root, stop: &Signal, cb: &Callback) {
    let Ok(mut op) = IoOp::new(64 * 1024) else {
        cb(Event::Rescan(root.orig.clone()));
        return;
    };
    loop {
        if op.dir_changes(&handle).is_err() {
            cb(Event::Rescan(root.orig.clone()));
            return;
        }
        match op.wait(stop) {
            Ok(None) => return,
            // Zero bytes means the buffer overflowed and the kernel dropped the changes.
            Ok(Some(0)) => cb(Event::Rescan(root.orig.clone())),
            Ok(Some(n)) => {
                let mut seen: HashSet<Vec<u16>> = HashSet::new();
                for notice in parse_notices(op.bytes(n)) {
                    if seen.insert(notice.name.clone()) {
                        let rel = OsString::from_wide(&notice.name);
                        cb(Event::Changed(root.orig.join(rel)));
                    }
                }
            }
            Err(e) if e.raw_os_error() == Some(sys::ERROR_NOTIFY_ENUM_DIR) => {
                cb(Event::Rescan(root.orig.clone()));
            }
            // The root is gone or unreadable: say so once and stop, rather than spin.
            Err(_) => {
                cb(Event::Rescan(root.orig.clone()));
                return;
            }
        }
    }
}
