//! The updater: a background thread that turns file system events into index updates.
//!
//! The platform watcher delivers events on its own thread; this module forwards them over a channel to
//! one worker that sleeps in `recv` (zero CPU while nothing changes), waits for the burst to settle
//! (a `git checkout` touching 40,000 files is one batch, not 40,000 wake-ups), takes the write lock
//! once, applies the batch and, now and then, compacts. The id of the last event handled is written to a
//! small file, so the next start asks the OS for what changed while the app was closed.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::live::LiveIndex;

/// A change report from a platform watcher.
pub enum WatchMsg {
    /// Something at these paths changed.
    Changed(PathBuf),
    /// Events below this folder were coalesced or dropped: read it again.
    Rescan(PathBuf),
    /// The id of the newest event seen (macOS), to persist.
    EventId(u64),
    /// Stop the worker.
    Stop,
}

/// A running updater. Dropping it stops the watcher and joins the worker.
pub struct Updater {
    tx: Sender<WatchMsg>,
    worker: Option<JoinHandle<()>>,
    _watcher: Option<Box<dyn std::any::Any + Send>>,
}

/// How long the worker waits after the last event before applying a batch.
const SETTLE: Duration = Duration::from_millis(200);
/// The longest a batch may keep growing.
const MAX_BATCH: Duration = Duration::from_millis(1500);

/// A marker to take before a scan starts: pass it to [`start`] after the scan so changes made while
/// scanning are replayed. `None` where the OS has no event ids.
pub fn marker() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        Some(panoply_platform_macos::fsevents::current_event_id())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Reads an event id saved by a previous run.
pub fn read_event_id(file: &Path) -> Option<u64> {
    std::fs::read_to_string(file).ok()?.trim().parse().ok()
}

/// Starts keeping `index` fresh. `since` is the event id to replay from (the saved id, or the marker
/// taken before the scan); `state_file` is where the newest handled id is stored.
pub fn start(
    index: Arc<RwLock<LiveIndex>>,
    since: Option<u64>,
    state_file: Option<PathBuf>,
) -> io::Result<Updater> {
    let (tx, rx) = mpsc::channel::<WatchMsg>();
    let roots: Vec<PathBuf> = index
        .read()
        .map_err(|_| io::Error::other("index lock poisoned"))?
        .roots()
        .map(Path::to_path_buf)
        .collect();
    let watcher = platform_watch(&index, &roots, since, tx.clone())?;
    let worker = std::thread::Builder::new()
        .name("panoply-find-updater".into())
        .spawn(move || worker_loop(&rx, &index, state_file.as_deref()))
        .map_err(io::Error::other)?;
    Ok(Updater {
        tx,
        worker: Some(worker),
        _watcher: Some(watcher),
    })
}

impl Drop for Updater {
    fn drop(&mut self) {
        // Stop the watcher first so nothing sends after the worker has gone.
        self._watcher.take();
        let _ = self.tx.send(WatchMsg::Stop);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_watch(
    index: &Arc<RwLock<LiveIndex>>,
    roots: &[PathBuf],
    since: Option<u64>,
    tx: Sender<WatchMsg>,
) -> io::Result<Box<dyn std::any::Any + Send>> {
    use panoply_platform_macos::fsevents as fse;
    let exclude = index
        .read()
        .map_err(|_| io::Error::other("index lock poisoned"))?
        .exclusions()
        .clone();
    let roots_cb: Vec<PathBuf> = roots.to_vec();
    let paths: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    let stream = fse::start(&paths, since, 0.1, move |events| {
        let mut newest = 0u64;
        for e in events {
            newest = newest.max(e.id);
            if e.flags
                & (fse::MUST_SCAN_SUBDIRS
                    | fse::USER_DROPPED
                    | fse::KERNEL_DROPPED
                    | fse::ROOT_CHANGED)
                != 0
            {
                let target = if e.path.as_os_str().is_empty() {
                    roots_cb.first().cloned().unwrap_or_default()
                } else {
                    e.path.clone()
                };
                let _ = tx.send(WatchMsg::Rescan(target));
                continue;
            }
            if e.flags & fse::HISTORY_DONE != 0 {
                continue;
            }
            // Drop events from folders the index never reads (build output, dependencies, .git) here,
            // before they wake the worker.
            let under = roots_cb.iter().find(|r| e.path.starts_with(r));
            if under.is_none_or(|r| exclude.has_excluded_ancestor(r, &e.path)) {
                continue;
            }
            let _ = tx.send(WatchMsg::Changed(e.path));
        }
        if newest > 0 {
            let _ = tx.send(WatchMsg::EventId(newest));
        }
    })
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(Box::new(stream))
}

#[cfg(target_os = "linux")]
fn platform_watch(
    index: &Arc<RwLock<LiveIndex>>,
    roots: &[PathBuf],
    _since: Option<u64>,
    tx: Sender<WatchMsg>,
) -> io::Result<Box<dyn std::any::Any + Send>> {
    let exclude = index
        .read()
        .map_err(|_| io::Error::other("index lock poisoned"))?
        .exclusions()
        .clone();
    let watcher = crate::watch_linux::InotifyWatcher::start(roots, exclude, tx)?;
    Ok(Box::new(watcher))
}

#[cfg(windows)]
fn platform_watch(
    index: &Arc<RwLock<LiveIndex>>,
    roots: &[PathBuf],
    _since: Option<u64>,
    tx: Sender<WatchMsg>,
) -> io::Result<Box<dyn std::any::Any + Send>> {
    use panoply_platform_windows as pw;
    let exclude = index
        .read()
        .map_err(|_| io::Error::other("index lock poisoned"))?
        .exclusions()
        .clone();
    let roots_cb: Vec<PathBuf> = roots.to_vec();
    let watcher = pw::start(roots, move |event| match event {
        pw::Event::Rescan(p) => {
            let _ = tx.send(WatchMsg::Rescan(p));
        }
        pw::Event::Changed(p) => {
            // Drop events from folders the index never reads before they wake the worker.
            let under = roots_cb.iter().find(|r| p.starts_with(r));
            if under.is_none_or(|r| exclude.has_excluded_ancestor(r, &p)) {
                return;
            }
            let _ = tx.send(WatchMsg::Changed(p));
        }
    })
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(Box::new(watcher))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_watch(
    _index: &Arc<RwLock<LiveIndex>>,
    _roots: &[PathBuf],
    _since: Option<u64>,
    _tx: Sender<WatchMsg>,
) -> io::Result<Box<dyn std::any::Any + Send>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no file watcher on this platform yet",
    ))
}

#[derive(Default)]
struct Batch {
    changed: BTreeSet<PathBuf>,
    rescan: BTreeSet<PathBuf>,
    newest_id: u64,
    stop: bool,
}

impl Batch {
    fn absorb(&mut self, m: WatchMsg) {
        match m {
            WatchMsg::Changed(p) => {
                self.changed.insert(p);
            }
            WatchMsg::Rescan(p) => {
                self.rescan.insert(p);
            }
            WatchMsg::EventId(id) => self.newest_id = self.newest_id.max(id),
            WatchMsg::Stop => self.stop = true,
        }
    }
}

fn worker_loop(
    rx: &mpsc::Receiver<WatchMsg>,
    index: &Arc<RwLock<LiveIndex>>,
    state_file: Option<&Path>,
) {
    let mut last_saved = Instant::now() - Duration::from_secs(60);
    let mut pending_id = 0u64;
    let mut saved_id = 0u64;
    loop {
        let mut batch = Batch::default();
        // Idle: block with no timeout, so there are no wake-ups at all.
        match rx.recv() {
            Ok(m) => batch.absorb(m),
            Err(_) => return,
        }
        let began = Instant::now();
        while !batch.stop && began.elapsed() < MAX_BATCH {
            match rx.recv_timeout(SETTLE) {
                Ok(m) => batch.absorb(m),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => batch.stop = true,
            }
        }
        pending_id = pending_id.max(batch.newest_id);
        if (!batch.changed.is_empty() || !batch.rescan.is_empty())
            && let Ok(mut live) = index.write()
        {
            // A rescan covers everything below it.
            for r in &batch.rescan {
                live.rescan_dir(r);
            }
            // Parents before children: shorter paths first.
            let mut ordered: Vec<&PathBuf> = batch.changed.iter().collect();
            ordered.sort_by_key(|p| p.components().count());
            for p in ordered {
                if !batch.rescan.iter().any(|r| p.starts_with(r)) {
                    live.apply_path(p);
                }
            }
            live.finish_batch();
            if live.wants_compaction() {
                live.compact();
            }
        }
        if let Some(f) = state_file
            && pending_id > saved_id
            && (batch.stop || last_saved.elapsed() > Duration::from_secs(2))
        {
            if std::fs::write(f, format!("{pending_id}\n")).is_ok() {
                saved_id = pending_id;
            }
            last_saved = Instant::now();
        }
        if batch.stop {
            return;
        }
    }
}
