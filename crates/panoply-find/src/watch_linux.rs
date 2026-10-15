//! The Linux watcher: inotify through rustix's safe API.
//!
//! One inotify descriptor watches every indexed folder, recursively: the watcher walks each root once
//! and adds a watch per folder the [`Exclusions`] let in, then adds a watch for each folder that appears
//! later. The thread sleeps in `poll` on two descriptors (inotify and an eventfd that `Drop` writes to),
//! so an idle tree costs no CPU and no timer. A kernel queue overflow becomes a rescan of every root.
//!
//! The watch limit (`fs.inotify.max_user_watches`) is a hard cap shared by every program of the user.
//! When `inotify_add_watch` fails with `ENOSPC` the watcher stops adding watches and keeps the ones it
//! has: folders beyond the limit are not watched until the next start. It says so once on stderr. It
//! never falls back to polling.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use rustix::fs::inotify::{self, CreateFlags, Reader, WatchFlags};
use rustix::io::Errno;

use crate::Exclusions;
use crate::inotify_table::{Kind, WatchTable, classify, dir_skipped};
use crate::watch::WatchMsg;

const WATCH_MASK: WatchFlags = WatchFlags::CREATE
    .union(WatchFlags::DELETE)
    .union(WatchFlags::CLOSE_WRITE)
    .union(WatchFlags::ATTRIB)
    .union(WatchFlags::MOVED_FROM)
    .union(WatchFlags::MOVED_TO)
    .union(WatchFlags::DELETE_SELF)
    .union(WatchFlags::MOVE_SELF)
    .union(WatchFlags::ONLYDIR)
    .union(WatchFlags::DONT_FOLLOW)
    .union(WatchFlags::EXCL_UNLINK);

/// A running inotify watcher. Dropping it wakes the thread and joins it.
pub(crate) struct InotifyWatcher {
    stop: OwnedFd,
    thread: Option<JoinHandle<()>>,
}

impl InotifyWatcher {
    /// Starts watching `roots`. Fails only when inotify itself cannot be created.
    pub(crate) fn start(
        roots: &[PathBuf],
        exclude: Exclusions,
        tx: Sender<WatchMsg>,
    ) -> io::Result<Self> {
        let inot = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        let stop = eventfd(0, EventfdFlags::CLOEXEC)?;
        let stop_for_thread = stop.try_clone()?;
        let mut state = State {
            inot,
            table: WatchTable::default(),
            roots: roots.to_vec(),
            root_dev: HashMap::new(),
            exclude,
            tx,
            limit_hit: false,
        };
        let thread = std::thread::Builder::new()
            .name("panoply-find-inotify".into())
            .spawn(move || state.run(&stop_for_thread))?;
        Ok(InotifyWatcher {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for InotifyWatcher {
    fn drop(&mut self) {
        // An eventfd write of 1 makes the descriptor readable, which wakes the thread's poll.
        let _ = rustix::io::write(&self.stop, &1u64.to_ne_bytes());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct State {
    inot: OwnedFd,
    table: WatchTable,
    roots: Vec<PathBuf>,
    root_dev: HashMap<PathBuf, u64>,
    exclude: Exclusions,
    tx: Sender<WatchMsg>,
    limit_hit: bool,
}

impl State {
    fn run(&mut self, stop: &OwnedFd) {
        for root in self.roots.clone() {
            if let Ok(md) = std::fs::symlink_metadata(&root) {
                self.root_dev.insert(root.clone(), md.dev());
                self.add_tree(&root, &root);
            }
        }
        let mut buf = [MaybeUninit::<u8>::uninit(); 64 * 1024];
        loop {
            let mut fds = [
                PollFd::new(&self.inot, PollFlags::IN),
                PollFd::new(stop, PollFlags::IN),
            ];
            match poll(&mut fds, None) {
                Ok(_) => {}
                Err(Errno::INTR) => continue,
                Err(_) => return,
            }
            if !fds[1].revents().is_empty() {
                return;
            }
            // Take the events out of the kernel queue first; handling them adds watches, which needs the
            // descriptor again while the reader still borrows it.
            let mut events: Vec<(i32, u32, Vec<u8>)> = Vec::new();
            let mut reader = Reader::new(&self.inot, &mut buf);
            loop {
                match reader.next() {
                    Ok(ev) => events.push((
                        ev.wd(),
                        ev.events().bits(),
                        ev.file_name().map(|c| c.to_bytes().to_vec()).unwrap_or_default(),
                    )),
                    Err(Errno::INTR) => {}
                    Err(Errno::AGAIN) => break,
                    Err(_) => return,
                }
            }
            for (wd, mask, name) in events {
                self.handle(wd, mask, &name);
            }
        }
    }

    fn handle(&mut self, wd: i32, mask: u32, name: &[u8]) {
        match classify(mask) {
            Kind::Overflow => {
                for r in &self.roots {
                    let _ = self.tx.send(WatchMsg::Rescan(r.clone()));
                }
            }
            Kind::WatchGone => {
                self.table.remove(wd);
            }
            Kind::SelfGone => {
                let Some(dir) = self.table.dir(wd).map(Path::to_path_buf) else {
                    return;
                };
                if self.roots.contains(&dir) {
                    let _ = self.tx.send(WatchMsg::Rescan(dir));
                }
                // A folder below a root is reported by its parent's own watch.
            }
            kind => {
                let Some(dir) = self.table.dir(wd) else {
                    return;
                };
                let path = dir.join(OsStr::from_bytes(name));
                let Some(root) = self.roots.iter().find(|r| path.starts_with(r)).cloned() else {
                    return;
                };
                if self.exclude.has_excluded_ancestor(&root, &path) {
                    return;
                }
                match kind {
                    Kind::NewDir => {
                        let name = String::from_utf8_lossy(name);
                        let has_cargo = path.parent().is_some_and(|p| p.join("Cargo.toml").exists());
                        // An excluded folder is indexed by name but gets no watch.
                        if !dir_skipped(&self.exclude, &name, &path, has_cargo) {
                            self.add_tree(&root, &path);
                        }
                    }
                    Kind::DirLeft => {
                        for gone in self.table.remove_under(&path) {
                            let _ = inotify::remove_watch(&self.inot, gone);
                        }
                    }
                    _ => {}
                }
                let _ = self.tx.send(WatchMsg::Changed(path));
            }
        }
    }

    /// Adds a watch for `dir` and every folder below it that the exclusions let in.
    fn add_tree(&mut self, root: &Path, dir: &Path) {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            if self.limit_hit {
                return;
            }
            match inotify::add_watch(&self.inot, &d, WATCH_MASK) {
                Ok(wd) => self.table.insert(wd, d.clone()),
                Err(Errno::NOSPC) => {
                    self.limit_hit = true;
                    eprintln!(
                        "panoply-find: the inotify watch limit is reached after {} folders; \
                         folders beyond it are not watched until the limit is raised \
                         (fs.inotify.max_user_watches) and the app restarts",
                        self.table.len()
                    );
                    return;
                }
                // Gone already, not a folder, or not allowed: nothing to watch here.
                Err(_) => continue,
            }
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
            let mut has_cargo = false;
            for ent in rd.flatten() {
                let name = ent.file_name();
                if name == "Cargo.toml" {
                    has_cargo = true;
                }
                if ent.file_type().is_ok_and(|t| t.is_dir()) {
                    subdirs.push((name.to_string_lossy().into_owned(), ent.path()));
                }
            }
            let dev = self.root_dev.get(root).copied();
            for (name, path) in subdirs {
                if dir_skipped(&self.exclude, &name, &path, has_cargo) {
                    continue;
                }
                if self.exclude.one_file_system
                    && let (Some(rd), Ok(md)) = (dev, std::fs::symlink_metadata(&path))
                    && md.dev() != rd
                {
                    continue;
                }
                stack.push(path);
            }
        }
    }
}
