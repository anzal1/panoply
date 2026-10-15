//! What the person picked on the disk map and what happened when it was moved: plain state around
//! [`ReviewList`], with no UI types, so the rules are tested on their own. The view in `disk.rs` only
//! draws it and runs `trash_all` and `undo` on a worker thread.

use std::path::{Path, PathBuf};

use panoply_core::TrashError;

use crate::disk_review::{Failure, ReviewItem, ReviewList, TrashProgress};
use crate::units::format_bytes;

/// The picks, the review panel's state and the result lines.
pub(crate) struct Picks {
    pub list: ReviewList,
    /// The review panel is showing in place of the map.
    pub open: bool,
    /// One plain line about the last move or undo.
    pub result: Option<String>,
    /// What the last move or undo could not do, with reasons (the items stay in the list).
    pub failures: Vec<Failure>,
    /// Something was moved or put back since the scan, so the map no longer matches the disk.
    pub stale: bool,
}

impl Picks {
    /// An empty set that protects the real home folder.
    pub(crate) fn new() -> Picks {
        Picks::with_list(ReviewList::new())
    }

    /// A set around a given list (tests give it a fake home).
    pub(crate) fn with_list(list: ReviewList) -> Picks {
        Picks {
            list,
            open: false,
            result: None,
            failures: Vec::new(),
            stale: false,
        }
    }

    /// Pick or unpick an item. Returns true when it is picked afterwards.
    pub(crate) fn toggle(&mut self, item: ReviewItem) -> bool {
        self.result = None;
        self.failures.clear();
        self.list.toggle(item)
    }

    /// True when the path is picked.
    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.list.contains(path)
    }

    /// How many items are picked.
    pub(crate) fn count(&self) -> usize {
        self.list.len()
    }

    /// Take one item out of the list.
    pub(crate) fn remove(&mut self, path: &Path) {
        self.list.remove(path);
        if self.list.is_empty() && !self.list.can_undo() {
            self.open = false;
        }
    }

    /// "3 selected, 4.2 MB": the count of what is picked and the bytes a move would free.
    pub(crate) fn selected_line(&self) -> String {
        format!(
            "{} selected, {}",
            self.count(),
            format_bytes(self.list.total_bytes())
        )
    }

    /// True when a move would do something.
    pub(crate) fn can_move(&self) -> bool {
        !self.list.effective_items().is_empty()
    }

    /// Move everything picked with `mover` (the real one on a worker thread, a fake in tests).
    /// Never deletes: the mover decides where things go.
    pub(crate) fn run_move(
        &mut self,
        mover: impl FnMut(&Path) -> Result<Option<PathBuf>, TrashError>,
        progress: impl FnMut(&TrashProgress),
    ) {
        let report = self.list.trash_all(mover, progress);
        self.result = Some(report.headline());
        self.failures = report.failed;
        if report.moved > 0 {
            self.stale = true;
        }
    }

    /// Put back what the last moves took, with `putter(trashed, original)`.
    pub(crate) fn run_undo(&mut self, putter: impl FnMut(&Path, &Path) -> Result<(), TrashError>) {
        let report = self.list.undo(putter);
        self.result = Some(report.headline());
        self.failures = report.failed;
        if report.restored > 0 {
            self.stale = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn picks() -> Picks {
        Picks::with_list(ReviewList::with_home(Some(PathBuf::from("/Users/me"))))
    }

    fn item(p: &str, size: u64, dir: bool) -> ReviewItem {
        ReviewItem::new(p, size, dir)
    }

    fn fake_trash(p: &Path) -> Result<Option<PathBuf>, TrashError> {
        Ok(Some(PathBuf::from("/Trash").join(p.file_name().unwrap())))
    }

    #[test]
    fn toggle_picks_then_unpicks_and_counts() {
        let mut p = picks();
        assert!(p.toggle(item("/Users/me/a.mov", 5_000_000, false)));
        assert!(p.toggle(item("/Users/me/b.zip", 1_000_000, false)));
        assert_eq!(p.count(), 2);
        assert_eq!(p.selected_line(), "2 selected, 6.0 MB");
        assert!(p.contains(Path::new("/Users/me/a.mov")));
        assert!(!p.toggle(item("/Users/me/a.mov", 5_000_000, false)));
        assert_eq!(p.count(), 1);
        assert!(!p.contains(Path::new("/Users/me/a.mov")));
    }

    #[test]
    fn a_folder_and_its_content_count_once() {
        let mut p = picks();
        p.toggle(item("/Users/me/Movies", 6_000_000, true));
        p.toggle(item("/Users/me/Movies/a.mov", 5_000_000, false));
        assert_eq!(p.count(), 2);
        assert_eq!(p.selected_line(), "2 selected, 6.0 MB");
    }

    #[test]
    fn removing_the_last_item_closes_the_panel() {
        let mut p = picks();
        p.toggle(item("/Users/me/a.mov", 10, false));
        p.open = true;
        p.remove(Path::new("/Users/me/a.mov"));
        assert!(!p.open);
        assert_eq!(p.count(), 0);
        assert!(!p.can_move());
    }

    #[test]
    fn a_move_uses_the_mover_only_and_reports_in_plain_words() {
        let mut p = picks();
        p.toggle(item("/Users/me/a.mov", 5_000_000, false));
        p.toggle(item("/Users/me/b.zip", 1_000_000, false));
        let seen = RefCell::new(Vec::new());
        p.run_move(
            |path| {
                seen.borrow_mut().push(path.to_path_buf());
                fake_trash(path)
            },
            |_| {},
        );
        assert_eq!(seen.borrow().len(), 2);
        assert_eq!(p.count(), 0);
        assert!(p.stale);
        assert!(p.list.can_undo());
        let line = p.result.clone().unwrap();
        assert!(line.contains('2'), "{line}");
        assert!(!line.contains('\u{2014}'), "no em dashes: {line}");
    }

    #[test]
    fn a_failure_stays_in_the_list_with_its_reason() {
        let mut p = picks();
        p.toggle(item("/Users/me/a.mov", 10, false));
        p.toggle(item("/Users/me/locked.db", 20, false));
        p.run_move(
            |path| {
                if path.ends_with("locked.db") {
                    Err(TrashError::Failed {
                        why: "it is in use".to_string(),
                    })
                } else {
                    fake_trash(path)
                }
            },
            |_| {},
        );
        assert_eq!(p.count(), 1);
        assert!(p.contains(Path::new("/Users/me/locked.db")));
        assert_eq!(p.failures.len(), 1);
        assert!(p.failures[0].reason.contains("in use"));
    }

    #[test]
    fn the_home_folder_and_roots_are_refused_and_never_reach_the_mover() {
        let mut p = picks();
        p.toggle(item("/Users/me", 1_000, true));
        p.toggle(item("/", 1_000, true));
        assert!(!p.can_move());
        let calls = RefCell::new(0);
        p.run_move(
            |path| {
                *calls.borrow_mut() += 1;
                fake_trash(path)
            },
            |_| {},
        );
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(p.failures.len(), 2);
        assert_eq!(p.count(), 2);
        assert!(!p.stale);
    }

    #[test]
    fn undo_lists_the_items_again_and_marks_the_map_stale() {
        let mut p = picks();
        p.toggle(item("/Users/me/a.mov", 10, false));
        p.run_move(fake_trash, |_| {});
        assert_eq!(p.count(), 0);
        let back = RefCell::new(Vec::new());
        p.run_undo(|trashed, original| {
            back.borrow_mut()
                .push((trashed.to_path_buf(), original.to_path_buf()));
            Ok(())
        });
        assert_eq!(back.borrow().len(), 1);
        assert_eq!(back.borrow()[0].1, PathBuf::from("/Users/me/a.mov"));
        assert!(p.contains(Path::new("/Users/me/a.mov")));
        assert!(!p.list.can_undo());
    }

    #[test]
    fn toggling_clears_the_old_result_line() {
        let mut p = picks();
        p.toggle(item("/Users/me/a.mov", 10, false));
        p.run_move(fake_trash, |_| {});
        assert!(p.result.is_some());
        p.toggle(item("/Users/me/c.mov", 10, false));
        assert!(p.result.is_none());
    }
}
