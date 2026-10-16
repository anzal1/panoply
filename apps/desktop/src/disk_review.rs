//! The disk map's review list: what the person picked to clear, moved to the Trash in one go and put
//! back again on Undo. No UI types here, so all of it is tested on its own.
//!
//! The rules it keeps:
//! - Nothing is ever deleted for good. The caller passes in the mover (normally
//!   [`panoply_core::trash_item`]) and the putter ([`panoply_core::put_back`]).
//! - One item failing never stops the batch. Each failure keeps a plain reason.
//! - A filesystem root, the home folder (or any folder that holds it) and the folders the system and apps
//!   need are refused (see `panoply_core::protect`).
//! - When a folder and something inside it are both listed, only the folder is moved and its size
//!   is counted once.
//! - Undo puts back what this list moved and says what it could not, and why.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use panoply_core::TrashError;
use panoply_core::trash::TRASH_NAME;

use crate::units::format_bytes;

/// One thing in the review list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewItem {
    /// Where it is now.
    pub path: PathBuf,
    /// Its size on disk, in bytes.
    pub size_bytes: u64,
    /// True for a folder (it is moved with everything in it).
    pub is_dir: bool,
}

impl ReviewItem {
    /// A new item.
    pub fn new(path: impl Into<PathBuf>, size_bytes: u64, is_dir: bool) -> Self {
        ReviewItem {
            path: path.into(),
            size_bytes,
            is_dir,
        }
    }
}

/// An item that was moved, with what Undo needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    /// Where it came from.
    pub original: PathBuf,
    /// Where the system put it, when the system says (Windows does not).
    pub trashed: Option<PathBuf>,
    /// Its size, in bytes.
    pub size_bytes: u64,
    /// True for a folder.
    pub is_dir: bool,
}

/// An item that could not be moved or put back, with the reason in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The item's path.
    pub path: PathBuf,
    /// Why, as a sentence fragment that can be shown to a person.
    pub reason: String,
}

/// Progress after each item of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashProgress {
    /// Items handled so far, including this one.
    pub done: usize,
    /// Items in the batch.
    pub total: usize,
    /// The item just handled.
    pub path: PathBuf,
    /// Whether it was moved.
    pub moved: bool,
}

/// What [`ReviewList::trash_all`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrashReport {
    /// Items moved to the Trash.
    pub moved: usize,
    /// Bytes those items took.
    pub freed_bytes: u64,
    /// Items that could not be moved, each with its reason.
    pub failed: Vec<Failure>,
    /// Listed items inside a folder that was moved, so they went with it.
    pub inside_moved_folders: usize,
}

impl TrashReport {
    /// One plain line for the result: "Moved 11 items, 4.1 GB, to the Trash. 1 could not be moved."
    pub fn headline(&self) -> String {
        let mut s = format!(
            "Moved {}, {}, to the {TRASH_NAME}.",
            items_word(self.moved),
            format_bytes(self.freed_bytes)
        );
        if !self.failed.is_empty() {
            s.push_str(&format!(" {} could not be moved.", self.failed.len()));
        }
        s
    }
}

/// What [`ReviewList::undo`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UndoReport {
    /// Items put back where they were.
    pub restored: usize,
    /// Items that could not be put back, each with its reason. They are still in the trash.
    pub failed: Vec<Failure>,
}

impl UndoReport {
    /// One plain line for the result.
    pub fn headline(&self) -> String {
        let mut s = format!("Put back {}.", items_word(self.restored));
        if !self.failed.is_empty() {
            s.push_str(&format!(
                " {} could not be put back, see the {TRASH_NAME}.",
                self.failed.len()
            ));
        }
        s
    }
}

/// The review list.
#[derive(Clone, Debug, Default)]
pub struct ReviewList {
    items: Vec<ReviewItem>,
    moved: Vec<Moved>,
    home: Option<PathBuf>,
}

impl ReviewList {
    /// An empty list that protects the current user's home folder.
    pub fn new() -> Self {
        ReviewList::with_home(home_dir())
    }

    /// An empty list that protects `home` (or no home folder, for `None`). Tests use this.
    pub fn with_home(home: Option<PathBuf>) -> Self {
        ReviewList {
            items: Vec::new(),
            moved: Vec::new(),
            home,
        }
    }

    /// Add an item. Returns false when that path is already listed (its size is refreshed).
    pub fn add(&mut self, item: ReviewItem) -> bool {
        if let Some(existing) = self.items.iter_mut().find(|i| i.path == item.path) {
            existing.size_bytes = item.size_bytes;
            existing.is_dir = item.is_dir;
            return false;
        }
        self.items.push(item);
        true
    }

    /// Remove a path. Returns true when it was listed.
    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.path != path);
        self.items.len() != before
    }

    /// Add the item when it is not listed, remove it when it is. Returns true when it is listed now.
    pub fn toggle(&mut self, item: ReviewItem) -> bool {
        if self.remove(&item.path) {
            false
        } else {
            self.items.push(item);
            true
        }
    }

    /// True when the path is listed.
    pub fn contains(&self, path: &Path) -> bool {
        self.items.iter().any(|i| i.path == path)
    }

    /// Everything listed, in the order it was added.
    pub fn items(&self) -> &[ReviewItem] {
        &self.items
    }

    /// Items listed, including ones that would be skipped as protected or inside another listed folder.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing is listed.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Empty the list. What was already moved stays undoable.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The items that would really be moved: not protected, and not inside another listed folder.
    pub fn effective_items(&self) -> Vec<&ReviewItem> {
        let folders: HashSet<&Path> = self
            .items
            .iter()
            .filter(|i| i.is_dir && self.protected_reason(&i.path).is_none())
            .map(|i| i.path.as_path())
            .collect();
        self.items
            .iter()
            .filter(|i| self.protected_reason(&i.path).is_none())
            .filter(|i| !inside_any(&i.path, &folders))
            .collect()
    }

    /// Bytes that would be freed: the effective items only, so a folder and its contents count once.
    pub fn total_bytes(&self) -> u64 {
        self.effective_items().iter().map(|i| i.size_bytes).sum()
    }

    /// Why a path may not be moved, or `None` when it may. The list is `panoply_core::protect`'s: drive
    /// roots, the home folder and what holds it, and the folders the system and apps need
    /// ("macOS needs this folder"). The same check sits under `trash_item`, so it holds even if a
    /// picker forgets to ask.
    pub fn protected_reason(&self, path: &Path) -> Option<&'static str> {
        panoply_core::protect::refusal_for_home(path, self.home.as_deref())
    }

    /// A note for an entry that is allowed only because it is a known cache inside a protected folder.
    pub fn cache_note(&self, path: &Path) -> Option<&'static str> {
        panoply_core::protect::cache_note_for_home(path, self.home.as_deref())
    }

    /// "Move 12 items, 4.2 GB, to the Trash". Counts and sizes are of the effective items.
    pub fn summary(&self) -> String {
        let effective = self.effective_items();
        if effective.is_empty() {
            return "Nothing picked yet".to_string();
        }
        let bytes: u64 = effective.iter().map(|i| i.size_bytes).sum();
        format!(
            "Move {}, {}, to the {TRASH_NAME}",
            items_word(effective.len()),
            format_bytes(bytes)
        )
    }

    /// Items this list moved and that Undo can still put back.
    pub fn moved(&self) -> &[Moved] {
        &self.moved
    }

    /// True when there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.moved.is_empty()
    }

    /// Move every effective item with `mover`. Never stops at a failure: each one is recorded with a
    /// plain reason and the rest go on. `progress` is called after each item.
    ///
    /// Moved items leave the list and are remembered for [`ReviewList::undo`]. Failed and refused
    /// items stay listed so they can be retried or removed.
    pub fn trash_all(
        &mut self,
        mut mover: impl FnMut(&Path) -> Result<Option<PathBuf>, TrashError>,
        mut progress: impl FnMut(&TrashProgress),
    ) -> TrashReport {
        let mut report = TrashReport::default();
        let batch: Vec<ReviewItem> = self.effective_items().into_iter().cloned().collect();
        let refused: Vec<(ReviewItem, &'static str)> = self
            .items
            .iter()
            .filter_map(|i| self.protected_reason(&i.path).map(|r| (i.clone(), r)))
            .collect();
        let total = batch.len() + refused.len();
        let mut done = 0usize;
        let mut moved_dirs: Vec<PathBuf> = Vec::new();

        for (item, why) in &refused {
            done += 1;
            report.failed.push(Failure {
                path: item.path.clone(),
                reason: (*why).to_string(),
            });
            progress(&TrashProgress {
                done,
                total,
                path: item.path.clone(),
                moved: false,
            });
        }

        for item in batch {
            done += 1;
            let ok = match mover(&item.path) {
                Ok(trashed) => {
                    report.moved += 1;
                    report.freed_bytes += item.size_bytes;
                    if item.is_dir {
                        moved_dirs.push(item.path.clone());
                    }
                    self.moved.push(Moved {
                        original: item.path.clone(),
                        trashed,
                        size_bytes: item.size_bytes,
                        is_dir: item.is_dir,
                    });
                    true
                }
                Err(e) => {
                    report.failed.push(Failure {
                        path: item.path.clone(),
                        reason: e.to_string(),
                    });
                    false
                }
            };
            progress(&TrashProgress {
                done,
                total,
                path: item.path.clone(),
                moved: ok,
            });
        }

        let failed_paths: HashSet<&Path> = report.failed.iter().map(|f| f.path.as_path()).collect();
        let moved_paths: HashSet<&Path> = self.moved.iter().map(|m| m.original.as_path()).collect();
        let folders: HashSet<&Path> = moved_dirs.iter().map(PathBuf::as_path).collect();
        let before = self.items.len();
        let mut inside = 0usize;
        self.items.retain(|i| {
            if failed_paths.contains(i.path.as_path()) {
                return true;
            }
            if moved_paths.contains(i.path.as_path()) {
                return false;
            }
            if inside_any(&i.path, &folders) {
                inside += 1;
                return false;
            }
            true
        });
        debug_assert!(before >= self.items.len());
        report.inside_moved_folders = inside;
        report
    }

    /// Put back everything this list moved, with `putter(trashed, original)`. Items the system gave no
    /// location for (Windows) cannot be put back from here: the report says to open the Recycle Bin.
    /// What was put back is listed again; what could not be stays undoable.
    pub fn undo(
        &mut self,
        mut putter: impl FnMut(&Path, &Path) -> Result<(), TrashError>,
    ) -> UndoReport {
        let mut report = UndoReport::default();
        let mut keep = Vec::new();
        // Last moved, first put back, so a folder is back before anything that sat inside it.
        for m in std::mem::take(&mut self.moved).into_iter().rev() {
            let result = match &m.trashed {
                Some(trashed) => putter(trashed, &m.original).map_err(|e| e.to_string()),
                None => Err(format!(
                    "{TRASH_NAME} did not say where it went, so open the {TRASH_NAME} and restore it from there"
                )),
            };
            match result {
                Ok(()) => {
                    report.restored += 1;
                    self.add(ReviewItem::new(m.original, m.size_bytes, m.is_dir));
                }
                Err(reason) => {
                    report.failed.push(Failure {
                        path: m.original.clone(),
                        reason,
                    });
                    keep.push(m);
                }
            }
        }
        keep.reverse();
        self.moved = keep;
        report
    }
}

/// "1 item", "12 items".
fn items_word(n: usize) -> String {
    if n == 1 {
        "1 item".to_string()
    } else {
        format!("{n} items")
    }
}

/// True when a folder in `folders` is a proper ancestor of `path`.
fn inside_any(path: &Path, folders: &HashSet<&Path>) -> bool {
    path.ancestors().skip(1).any(|a| folders.contains(a))
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The text block that explains macOS Full Disk Access, shown when a scan was refused some folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FdaExplainer {
    /// The heading.
    pub title: &'static str,
    /// One line of why.
    pub why: &'static str,
    /// Three short steps.
    pub steps: [&'static str; 3],
}

impl FdaExplainer {
    /// The block, as it is always worded.
    pub fn new() -> Self {
        FdaExplainer {
            title: "Some folders need Full Disk Access",
            why: "macOS keeps a few folders closed to apps until you allow it. Panoply only reads file names and sizes, and nothing leaves this Mac.",
            steps: [
                "Open System Settings, then Privacy and Security, then Full Disk Access.",
                "Switch on Panoply in the list.",
                "Quit and reopen Panoply, then scan again.",
            ],
        }
    }

    /// The block when the scan needed it (`ScanReport::needs_full_disk_access()`), else `None`.
    pub fn if_needed(needs_full_disk_access: bool) -> Option<Self> {
        needs_full_disk_access.then(Self::new)
    }

    /// The whole block as plain text, one step per line, numbered.
    pub fn text(&self) -> String {
        let mut s = format!("{}\n{}\n", self.title, self.why);
        for (i, step) in self.steps.iter().enumerate() {
            s.push_str(&format!("{}. {step}\n", i + 1));
        }
        s
    }
}

impl Default for FdaExplainer {
    fn default() -> Self {
        FdaExplainer::new()
    }
}

/// The link that opens the Full Disk Access pane of System Settings on macOS.
pub fn fda_settings_url() -> &'static str {
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn item(p: &str, size: u64, dir: bool) -> ReviewItem {
        ReviewItem::new(p, size, dir)
    }

    fn list() -> ReviewList {
        ReviewList::with_home(Some(PathBuf::from("/Users/me")))
    }

    #[test]
    fn add_remove_toggle_and_total() {
        let mut l = list();
        assert!(l.add(item("/Users/me/a.bin", 1_000_000, false)));
        assert!(l.add(item("/Users/me/b.bin", 2_000_000, false)));
        assert!(
            !l.add(item("/Users/me/a.bin", 1_500_000, false)),
            "same path twice"
        );
        assert_eq!(l.len(), 2);
        assert_eq!(l.total_bytes(), 3_500_000);
        assert!(l.remove(Path::new("/Users/me/b.bin")));
        assert!(!l.remove(Path::new("/Users/me/b.bin")));
        assert!(l.toggle(item("/Users/me/c.bin", 10, false)));
        assert!(l.contains(Path::new("/Users/me/c.bin")));
        assert!(!l.toggle(item("/Users/me/c.bin", 10, false)));
        assert!(!l.contains(Path::new("/Users/me/c.bin")));
        assert_eq!(l.total_bytes(), 1_500_000);
    }

    #[test]
    fn summary_is_plain_text() {
        let mut l = list();
        assert_eq!(l.summary(), "Nothing picked yet");
        l.add(item("/Users/me/a", 880_000, false));
        assert_eq!(
            l.summary(),
            format!("Move 1 item, 880 KB, to the {TRASH_NAME}")
        );
        l.add(item("/Users/me/b", 4_000_000_000, true));
        assert_eq!(
            l.summary(),
            format!("Move 2 items, 4.0 GB, to the {TRASH_NAME}")
        );
        assert!(!l.summary().contains('\u{2014}'));
    }

    #[test]
    fn a_folder_and_a_file_inside_it_count_and_move_once() {
        let mut l = list();
        l.add(item("/Users/me/big/inner.bin", 400, false));
        l.add(item("/Users/me/big", 1000, true));
        l.add(item("/Users/me/other.bin", 50, false));
        assert_eq!(l.total_bytes(), 1050);
        assert_eq!(l.effective_items().len(), 2);

        let calls = RefCell::new(Vec::new());
        let report = l.trash_all(
            |p| {
                calls.borrow_mut().push(p.to_path_buf());
                Ok(Some(PathBuf::from("/trash").join(p.file_name().unwrap())))
            },
            |_| {},
        );

        assert_eq!(
            *calls.borrow(),
            vec![
                PathBuf::from("/Users/me/big"),
                PathBuf::from("/Users/me/other.bin")
            ]
        );
        assert_eq!(report.moved, 2);
        assert_eq!(report.freed_bytes, 1050);
        assert_eq!(report.inside_moved_folders, 1);
        assert!(report.failed.is_empty());
        assert!(l.is_empty(), "everything left the list");
    }

    #[test]
    fn one_failure_does_not_stop_the_batch_and_the_failed_item_stays() {
        let mut l = list();
        l.add(item("/Users/me/a", 10, false));
        l.add(item("/Users/me/locked", 20, false));
        l.add(item("/Users/me/c", 30, false));
        let progress = RefCell::new(Vec::new());

        let report = l.trash_all(
            |p| {
                if p.ends_with("locked") {
                    Err(TrashError::Failed {
                        why: "Permission denied".into(),
                    })
                } else {
                    Ok(Some(PathBuf::from("/trash").join(p.file_name().unwrap())))
                }
            },
            |p| progress.borrow_mut().push((p.done, p.total, p.moved)),
        );

        assert_eq!(report.moved, 2);
        assert_eq!(report.freed_bytes, 40);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].path, PathBuf::from("/Users/me/locked"));
        assert!(report.failed[0].reason.contains("Permission denied"));
        assert_eq!(
            *progress.borrow(),
            vec![(1, 3, true), (2, 3, false), (3, 3, true)]
        );
        assert_eq!(l.len(), 1);
        assert!(l.contains(Path::new("/Users/me/locked")));
        assert_eq!(l.moved().len(), 2);
        assert!(report.headline().contains("1 could not be moved"));
    }

    #[test]
    fn roots_and_the_home_folder_are_refused_and_never_sent_to_the_mover() {
        let mut l = list();
        l.add(item("/", 9, true));
        l.add(item("/Users/me", 9, true));
        l.add(item("/Users", 9, true));
        l.add(item("/Users/me/ok.bin", 5, false));
        let calls = RefCell::new(Vec::new());

        let report = l.trash_all(
            |p| {
                calls.borrow_mut().push(p.to_path_buf());
                Ok(None)
            },
            |_| {},
        );

        assert_eq!(*calls.borrow(), vec![PathBuf::from("/Users/me/ok.bin")]);
        assert_eq!(report.moved, 1);
        assert_eq!(report.failed.len(), 3);
        assert!(report.failed.iter().all(|f| !f.reason.is_empty()));
        assert_eq!(l.len(), 3, "the refused items stay listed");
    }

    #[test]
    fn a_system_folder_that_slips_into_the_list_never_reaches_the_mover() {
        let sys = if cfg!(windows) {
            r"C:\Windows"
        } else if cfg!(target_os = "macos") {
            "/System"
        } else {
            "/usr/lib"
        };
        let mut l = list();
        l.add(item(sys, 1_000, true));
        l.add(item("/Users/me/ok.bin", 5, false));
        let calls = RefCell::new(Vec::new());
        let report = l.trash_all(
            |p| {
                calls.borrow_mut().push(p.to_path_buf());
                Ok(None)
            },
            |_| {},
        );
        assert_eq!(*calls.borrow(), vec![PathBuf::from("/Users/me/ok.bin")]);
        assert_eq!(
            l.total_bytes(),
            0,
            "the refused item adds nothing to the total"
        );
        let f = report
            .failed
            .iter()
            .find(|f| f.path == Path::new(sys))
            .unwrap();
        assert!(f.reason.ends_with("needs this folder"), "{}", f.reason);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn library_is_locked_but_a_cache_folder_inside_it_can_go_with_a_note() {
        let l = list();
        let lib = Path::new("/Users/me/Library");
        assert_eq!(l.protected_reason(lib), Some("macOS needs this folder"));
        assert_eq!(
            l.protected_reason(Path::new("/Users/me/Library/Application Support")),
            Some("macOS needs this folder")
        );
        let cache = Path::new("/Users/me/Library/Caches/com.example.app");
        assert_eq!(l.protected_reason(cache), None);
        assert!(l.cache_note(cache).is_some());
        assert_eq!(l.cache_note(Path::new("/Users/me/Movies")), None);
        for p in [
            "/Applications",
            "/Library",
            "/System",
            "/Users/me",
            "/Volumes/Backup",
        ] {
            assert!(l.protected_reason(Path::new(p)).is_some(), "{p}");
        }
    }

    #[test]
    fn a_refused_folder_does_not_swallow_the_items_inside_it() {
        let mut l = list();
        l.add(item("/Users/me", 100, true));
        l.add(item("/Users/me/ok.bin", 5, false));
        assert_eq!(l.total_bytes(), 5);
        assert_eq!(l.effective_items().len(), 1);
    }

    #[test]
    fn windows_style_roots_are_refused() {
        // `Path::parent` of a drive root is None only on Windows, so this checks the plain-root rule
        // with what every platform shares.
        let l = ReviewList::with_home(None);
        assert!(l.protected_reason(Path::new("/")).is_some());
        assert!(l.protected_reason(Path::new("/Users/me/ok.bin")).is_none());
    }

    #[test]
    fn undo_puts_back_what_was_moved_and_lists_it_again() {
        let mut l = list();
        l.add(item("/Users/me/a", 10, false));
        l.add(item("/Users/me/b", 20, true));
        l.trash_all(
            |p| Ok(Some(PathBuf::from("/trash").join(p.file_name().unwrap()))),
            |_| {},
        );
        assert!(l.can_undo());
        assert!(l.is_empty());
        let puts = RefCell::new(Vec::new());

        let report = l.undo(|trashed, original| {
            puts.borrow_mut()
                .push((trashed.to_path_buf(), original.to_path_buf()));
            Ok(())
        });

        assert_eq!(report.restored, 2);
        assert!(report.failed.is_empty());
        assert_eq!(
            *puts.borrow(),
            vec![
                (PathBuf::from("/trash/b"), PathBuf::from("/Users/me/b")),
                (PathBuf::from("/trash/a"), PathBuf::from("/Users/me/a")),
            ]
        );
        assert!(!l.can_undo());
        assert_eq!(l.len(), 2);
        assert_eq!(l.total_bytes(), 30);
    }

    #[test]
    fn undo_reports_what_could_not_be_put_back_and_keeps_it_undoable() {
        let mut l = list();
        l.add(item("/Users/me/busy", 10, false));
        l.add(item("/Users/me/free", 20, false));
        l.add(item("/Users/me/nowhere", 30, false));
        let mut n = 0;
        l.trash_all(
            |p| {
                n += 1;
                // The third item gets no location, as on Windows.
                if n == 3 {
                    Ok(None)
                } else {
                    Ok(Some(PathBuf::from("/trash").join(p.file_name().unwrap())))
                }
            },
            |_| {},
        );

        let report = l.undo(|_, original| {
            if original.ends_with("busy") {
                Err(TrashError::PutBack {
                    why: "something with the same name is already in its old folder".into(),
                })
            } else {
                Ok(())
            }
        });

        assert_eq!(report.restored, 1);
        assert_eq!(report.failed.len(), 2);
        let nowhere = report
            .failed
            .iter()
            .find(|f| f.path.ends_with("nowhere"))
            .unwrap();
        assert!(
            nowhere.reason.contains(&format!("open the {TRASH_NAME}")),
            "{}",
            nowhere.reason
        );
        let busy = report
            .failed
            .iter()
            .find(|f| f.path.ends_with("busy"))
            .unwrap();
        assert!(
            busy.reason.contains("already in its old folder"),
            "{}",
            busy.reason
        );
        assert_eq!(l.moved().len(), 2, "the two that stayed are still undoable");
        assert!(l.contains(Path::new("/Users/me/free")));
        assert!(report.headline().contains("2 could not be put back"));
    }

    #[test]
    fn full_disk_access_text_is_plain_and_only_shown_when_needed() {
        assert!(FdaExplainer::if_needed(false).is_none());
        let e = FdaExplainer::if_needed(true).expect("shown when needed");
        assert_eq!(e.steps.len(), 3);
        let text = e.text();
        for word in [
            "System Settings",
            "Privacy and Security",
            "Full Disk Access",
            "Panoply",
        ] {
            assert!(text.contains(word), "{word} missing from {text}");
        }
        assert!(text.contains("reopen") || text.contains("restart"));
        assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'));
        assert!(fda_settings_url().starts_with("x-apple.systempreferences:"));
        assert!(fda_settings_url().ends_with("Privacy_AllFiles"));
    }
}
