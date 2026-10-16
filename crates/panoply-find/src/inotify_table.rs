//! The pure parts of the Linux watcher: what an inotify event mask means, the table from watch
//! descriptors to folders, and the rules for which folders get a watch. No system calls here, so the
//! tests run on every platform; `watch_linux.rs` wires it to the kernel.

// Used by the Linux watcher only; the tests run everywhere.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Exclusions;

/// `IN_ATTRIB`.
#[cfg(test)]
pub(crate) const IN_ATTRIB: u32 = 0x4;
/// `IN_CLOSE_WRITE`.
#[cfg(test)]
pub(crate) const IN_CLOSE_WRITE: u32 = 0x8;
/// `IN_MOVED_FROM`.
pub(crate) const IN_MOVED_FROM: u32 = 0x40;
/// `IN_MOVED_TO`.
pub(crate) const IN_MOVED_TO: u32 = 0x80;
/// `IN_CREATE`.
pub(crate) const IN_CREATE: u32 = 0x100;
/// `IN_DELETE`.
#[cfg(test)]
pub(crate) const IN_DELETE: u32 = 0x200;
/// `IN_DELETE_SELF`.
pub(crate) const IN_DELETE_SELF: u32 = 0x400;
/// `IN_MOVE_SELF`.
pub(crate) const IN_MOVE_SELF: u32 = 0x800;
/// `IN_UNMOUNT`.
pub(crate) const IN_UNMOUNT: u32 = 0x2000;
/// `IN_Q_OVERFLOW`: the kernel queue overflowed and events were lost.
pub(crate) const IN_Q_OVERFLOW: u32 = 0x4000;
/// `IN_IGNORED`: the watch is gone (folder deleted, unmounted or `inotify_rm_watch`).
pub(crate) const IN_IGNORED: u32 = 0x8000;
/// `IN_ISDIR`: the subject of the event is a folder.
pub(crate) const IN_ISDIR: u32 = 0x4000_0000;

/// What the updater should do about one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Events were lost: rescan every watched root.
    Overflow,
    /// The watch itself is gone: forget its descriptor.
    WatchGone,
    /// The watched folder itself was deleted, moved or unmounted.
    SelfGone,
    /// A folder appeared here (created or moved in): watch it and everything below it.
    NewDir,
    /// A folder left this place (moved out or renamed): drop the watches under its old path.
    DirLeft,
    /// Something below this folder changed: report the path.
    Change,
}

/// Decodes an event mask.
pub(crate) fn classify(mask: u32) -> Kind {
    if mask & IN_Q_OVERFLOW != 0 {
        Kind::Overflow
    } else if mask & IN_IGNORED != 0 {
        Kind::WatchGone
    } else if mask & (IN_DELETE_SELF | IN_MOVE_SELF | IN_UNMOUNT) != 0 {
        Kind::SelfGone
    } else if mask & IN_ISDIR != 0 && mask & (IN_CREATE | IN_MOVED_TO) != 0 {
        Kind::NewDir
    } else if mask & IN_ISDIR != 0 && mask & IN_MOVED_FROM != 0 {
        Kind::DirLeft
    } else {
        Kind::Change
    }
}

/// Which folder each watch descriptor belongs to.
#[derive(Default)]
pub(crate) struct WatchTable {
    dirs: HashMap<i32, PathBuf>,
}

impl WatchTable {
    /// Records `wd` for `dir`. The kernel hands back the same descriptor for a folder it already
    /// watches, so adding a moved folder again moves its entry to the new path.
    pub(crate) fn insert(&mut self, wd: i32, dir: PathBuf) {
        self.dirs.insert(wd, dir);
    }

    /// The folder a descriptor watches.
    pub(crate) fn dir(&self, wd: i32) -> Option<&Path> {
        self.dirs.get(&wd).map(PathBuf::as_path)
    }

    /// Forgets a descriptor.
    pub(crate) fn remove(&mut self, wd: i32) -> Option<PathBuf> {
        self.dirs.remove(&wd)
    }

    /// Removes and returns every descriptor at or below `dir`.
    pub(crate) fn remove_under(&mut self, dir: &Path) -> Vec<i32> {
        let gone: Vec<i32> = self
            .dirs
            .iter()
            .filter(|(_, p)| p.starts_with(dir))
            .map(|(wd, _)| *wd)
            .collect();
        for wd in &gone {
            self.dirs.remove(wd);
        }
        gone
    }

    /// How many folders are watched.
    pub(crate) fn len(&self) -> usize {
        self.dirs.len()
    }
}

/// True when the folder `name` at `path` must not get a watch: the same policy the walker applies, so
/// the watcher costs nothing for dependency and build folders. `has_cargo` says a `Cargo.toml` sits
/// next to it (a Rust build folder called `target`).
pub(crate) fn dir_skipped(ex: &Exclusions, name: &str, path: &Path, has_cargo: bool) -> bool {
    let (folded, _) = crate::fold::fold(name);
    ex.name_excluded(&folded)
        || ex.path_excluded(path)
        || (ex.cargo_target && name == "target" && has_cargo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(classify(IN_Q_OVERFLOW), Kind::Overflow);
        assert_eq!(classify(IN_IGNORED), Kind::WatchGone);
        assert_eq!(classify(IN_DELETE_SELF), Kind::SelfGone);
        assert_eq!(classify(IN_UNMOUNT), Kind::SelfGone);
        assert_eq!(classify(IN_CREATE | IN_ISDIR), Kind::NewDir);
        assert_eq!(classify(IN_MOVED_TO | IN_ISDIR), Kind::NewDir);
        assert_eq!(classify(IN_MOVED_FROM | IN_ISDIR), Kind::DirLeft);
        assert_eq!(classify(IN_CREATE), Kind::Change);
        assert_eq!(classify(IN_CLOSE_WRITE), Kind::Change);
        assert_eq!(classify(IN_DELETE | IN_ISDIR), Kind::Change);
        assert_eq!(classify(IN_ATTRIB), Kind::Change);
        // Overflow wins over everything else in the same mask.
        assert_eq!(classify(IN_Q_OVERFLOW | IN_CREATE), Kind::Overflow);
    }

    #[test]
    fn table_moves_and_prunes() {
        let mut t = WatchTable::default();
        t.insert(1, PathBuf::from("/r"));
        t.insert(2, PathBuf::from("/r/a"));
        t.insert(3, PathBuf::from("/r/a/b"));
        t.insert(4, PathBuf::from("/r/ab"));
        assert_eq!(t.dir(3), Some(Path::new("/r/a/b")));
        // A moved folder is added again and keeps its descriptor with the new path.
        t.insert(2, PathBuf::from("/r/z"));
        assert_eq!(t.dir(2), Some(Path::new("/r/z")));
        let mut gone = t.remove_under(Path::new("/r/a"));
        gone.sort_unstable();
        // `/r/ab` is a sibling, not a child of `/r/a`; `/r/z` was moved away from it already.
        assert_eq!(gone, vec![3]);
        assert_eq!(t.len(), 3);
        assert!(t.remove(4).is_some());
        assert!(t.remove(4).is_none());
    }

    #[test]
    fn skip_rules() {
        let ex = Exclusions::defaults();
        assert!(dir_skipped(
            &ex,
            "node_modules",
            Path::new("/r/node_modules"),
            false
        ));
        assert!(dir_skipped(&ex, ".git", Path::new("/r/.git"), false));
        assert!(dir_skipped(
            &ex,
            "build.noindex",
            Path::new("/r/build.noindex"),
            false
        ));
        assert!(!dir_skipped(&ex, "src", Path::new("/r/src"), false));
        assert!(dir_skipped(&ex, "target", Path::new("/r/target"), true));
        assert!(!dir_skipped(&ex, "target", Path::new("/r/target"), false));
        assert!(!dir_skipped(
            &Exclusions::none(),
            "node_modules",
            Path::new("/r/node_modules"),
            false
        ));
    }
}
