//! Move a file to the Trash (macOS, Linux) or the Recycle Bin (Windows), and never delete it for good.
//!
//! [`move_to_trash`] is the one call Undo and overwrite go through. The rule it keeps: if the system
//! cannot take the file (a network share, a USB stick, a locked file, a read-only folder), it returns an
//! error whose text is fit to show a person and leaves the file exactly where it was. It never falls
//! back to a permanent delete.
//!
//! - macOS asks `NSFileManager` to trash the item, as Finder does.
//! - Linux follows the freedesktop.org trash specification: `~/.local/share/Trash` with an info file per
//!   item, or the `.Trash-<uid>` folder at the top of another drive, so a move never copies a big file.
//! - Windows asks the shell (`IFileOperation`) to recycle the item, and refuses drives that have no
//!   Recycle Bin.

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
mod mac;
// Built on macOS too, for its tests only, so the Linux logic is exercised on a Mac.
#[cfg(all(unix, any(not(target_os = "macos"), test)))]
mod freedesktop;
#[cfg(windows)]
mod win;

/// What the system calls its trash: "Recycle Bin" on Windows, "Trash" elsewhere. For messages.
pub const TRASH_NAME: &str = if cfg!(windows) {
    "Recycle Bin"
} else {
    "Trash"
};

/// Why a file could not be moved to the trash. In every case the file is still where it was.
#[derive(Debug, thiserror::Error)]
pub enum TrashError {
    /// The file is not there (already moved or deleted).
    #[error("it is already gone")]
    NotFound,
    /// The drive the file is on has no trash to move it to (a network share or a USB stick, for
    /// example). Panoply will not delete the file for good in its place.
    #[error("this drive has no {TRASH_NAME}, so the file was left where it is")]
    NoTrash,
    /// The system tried and refused. `why` is the system's own reason.
    #[error("the {TRASH_NAME} would not take it ({why}), so the file was left where it is")]
    Failed {
        /// The reason the system gave.
        why: String,
    },
    /// An item could not be put back where it came from. The item stays in the trash.
    #[error("it could not be put back ({why}), so it was left in the {TRASH_NAME}")]
    PutBack {
        /// The plain reason: something is in the way, or the system refused.
        why: String,
    },
}

impl TrashError {
    #[cfg(any(unix, windows))]
    pub(crate) fn failed(why: impl std::fmt::Display) -> Self {
        TrashError::Failed {
            why: plain(&why.to_string()),
        }
    }
}

#[cfg(any(unix, windows))]
/// A system message made fit for a sentence: no trailing full stop and no "(os error 13)".
fn plain(message: &str) -> String {
    let mut s = message.trim().trim_end_matches('.').to_string();
    if let Some(i) = s.rfind(" (os error ")
        && s.ends_with(')')
    {
        s.truncate(i);
    }
    s
}

impl From<TrashError> for std::io::Error {
    fn from(e: TrashError) -> Self {
        let kind = match e {
            TrashError::NotFound => std::io::ErrorKind::NotFound,
            _ => std::io::ErrorKind::Other,
        };
        std::io::Error::new(kind, e)
    }
}

/// Move `path` (a file, a link or a folder) to the Trash or Recycle Bin.
///
/// On success the item is no longer at `path` and the user can put it back from the system trash. On
/// error the item is untouched and [`TrashError`] says why in plain words. Use [`trash_item`] when
/// the caller wants to know where the item went.
pub fn move_to_trash(path: &Path) -> Result<(), TrashError> {
    trash_item(path).map(|_| ())
}

/// Like [`move_to_trash`], and also returns where the item is now, when the system says. That
/// location is what [`put_back`] needs for a real Undo.
///
/// macOS gives the new place in the Trash, Linux the item's path under `Trash/files`. Windows does
/// not say, so it returns `None` and the item can only be restored from the Recycle Bin window.
pub fn trash_item(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    if path.symlink_metadata().is_err() {
        return Err(TrashError::NotFound);
    }
    os_trash(path)
}

/// Move an item back out of the trash to `original`, where [`trash_item`] took it from.
///
/// It refuses, and leaves the item in the trash, when something already exists at `original`, so
/// nothing is overwritten. A missing parent folder is made again. On Linux the item's `.trashinfo`
/// record is removed too.
pub fn put_back(trashed: &Path, original: &Path) -> Result<(), TrashError> {
    move_back(trashed, original)?;
    os_forget_record(trashed);
    Ok(())
}

fn put_back_failed(why: impl std::fmt::Display) -> TrashError {
    TrashError::PutBack {
        why: why.to_string(),
    }
}

/// The rename half of [`put_back`]: checks, then moves the item.
fn move_back(trashed: &Path, original: &Path) -> Result<(), TrashError> {
    if trashed.symlink_metadata().is_err() {
        return Err(put_back_failed(
            "it is no longer in the trash, it may have been emptied or restored by hand",
        ));
    }
    if original.symlink_metadata().is_ok() {
        return Err(put_back_failed(
            "something with the same name is already in its old folder",
        ));
    }
    if let Some(parent) = original.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|e| put_back_failed(plain_io(&e)))?;
    }
    match std::fs::rename(trashed, original) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => copy_back(trashed, original),
        Err(e) => Err(put_back_failed(plain_io(&e))),
    }
}

/// A plain file that sits on another drive than its old folder (the Linux copy fallback put it in the
/// home trash): copy it back, and remove the trashed copy only when the copy is whole.
fn copy_back(trashed: &Path, original: &Path) -> Result<(), TrashError> {
    let is_file = trashed
        .symlink_metadata()
        .map(|m| m.is_file())
        .unwrap_or(false);
    if !is_file {
        return Err(put_back_failed(
            "it is on another drive and is not a plain file",
        ));
    }
    if let Err(e) = std::fs::copy(trashed, original) {
        let _ = std::fs::remove_file(original);
        return Err(put_back_failed(plain_io(&e)));
    }
    std::fs::remove_file(trashed).map_err(|e| put_back_failed(plain_io(&e)))
}

/// An I/O error as plain words: no trailing full stop and no "(os error 13)".
fn plain_io(e: &std::io::Error) -> String {
    let mut s = e.to_string().trim().trim_end_matches('.').to_string();
    if let Some(i) = s.rfind(" (os error ")
        && s.ends_with(')')
    {
        s.truncate(i);
    }
    s
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_forget_record(trashed: &Path) {
    freedesktop::forget(trashed);
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn os_forget_record(_trashed: &Path) {}

#[cfg(target_os = "macos")]
fn os_trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    mac::trash(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    freedesktop::trash(path).map(Some)
}

#[cfg(windows)]
fn os_trash(path: &Path) -> Result<Option<PathBuf>, TrashError> {
    win::trash(path).map(|()| None)
}

#[cfg(not(any(unix, windows)))]
fn os_trash(_path: &Path) -> Result<Option<PathBuf>, TrashError> {
    Err(TrashError::NoTrash)
}

/// A name that is unique to this process and moment, for the files the tests trash.
#[cfg(test)]
fn unique(stem: &str, ext: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "{stem}-{}-{nanos}-{}.{ext}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(unique(name, "d"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_missing_file_is_reported_and_nothing_else_happens() {
        let d = temp_dir("panoply-trash-missing");
        let e = move_to_trash(&d.join("not-here.txt")).unwrap_err();
        assert!(matches!(e, TrashError::NotFound));
        assert!(d.exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn error_text_is_plain_words_and_names_the_trash() {
        let a = TrashError::NoTrash.to_string();
        let b = TrashError::failed("Permission denied (os error 13).").to_string();
        for m in [&a, &b] {
            assert!(m.contains(TRASH_NAME), "{m}");
            assert!(m.contains("left where it is"), "{m}");
            assert!(!m.contains("os error"), "{m}");
        }
        assert!(b.contains("Permission denied"));
        let io: std::io::Error = TrashError::NoTrash.into();
        assert_eq!(io.to_string(), a);
    }

    /// The real thing, through the public call: a temp file goes to the system trash and is
    /// findable there. macOS: the file is in `~/.Trash`. Windows: `$Recycle.Bin` for this user holds a
    /// record of the original path. Linux: the freedesktop trash has the file and its info file.
    #[test]
    fn a_file_goes_to_the_system_trash_and_is_there() {
        let name = unique("panoply-trash-test", "txt");
        let dir = trash_test_dir();
        let path = dir.join(&name);
        let body = format!("panoply trash test {name}");
        std::fs::write(&path, &body).unwrap();

        move_to_trash(&path).expect("the system trash takes a plain file in a temp folder");

        assert!(!path.exists(), "the file is still in its folder");
        let found = find_in_system_trash(&path, &name);
        let found = found.expect("the file is in the system trash");
        assert_eq!(std::fs::read_to_string(&found).unwrap(), body);
        remove_from_system_trash(&found, &name);
        let _ = std::fs::remove_dir(&dir);
    }

    /// A real round trip through the public calls: trash a temp file, learn where it went, put it
    /// back, and leave the system trash as it was found.
    #[test]
    fn trash_item_says_where_and_put_back_restores_it() {
        let name = unique("panoply-trash-undo", "txt");
        let dir = trash_test_dir();
        let path = dir.join(&name);
        std::fs::write(&path, "undo me").unwrap();

        let at = trash_item(&path).expect("trashed");
        assert!(!path.exists());
        if cfg!(windows) {
            assert!(at.is_none(), "Windows does not say where");
            let found = find_in_system_trash(&path, &name).expect("in the Recycle Bin");
            remove_from_system_trash(&found, &name);
        } else {
            let at = at.expect("macOS and Linux say where the item went");
            assert_eq!(std::fs::read_to_string(&at).unwrap(), "undo me");
            put_back(&at, &path).expect("put back");
            assert!(!at.exists());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "undo me");
            std::fs::remove_file(&path).unwrap();
        }
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn put_back_refuses_when_something_is_in_the_way_and_keeps_both() {
        let d = temp_dir("panoply-putback-busy");
        let trashed = d.join("trashed.txt");
        let original = d.join("original.txt");
        std::fs::write(&trashed, "from the trash").unwrap();
        std::fs::write(&original, "new file").unwrap();

        let e = put_back(&trashed, &original).unwrap_err();

        assert!(matches!(e, TrashError::PutBack { .. }), "{e}");
        let text = e.to_string();
        assert!(text.contains("already in its old folder"), "{text}");
        assert!(!text.contains("os error"), "{text}");
        assert_eq!(std::fs::read_to_string(&trashed).unwrap(), "from the trash");
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "new file");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn put_back_of_an_item_that_left_the_trash_says_so() {
        let d = temp_dir("panoply-putback-gone");
        let e = put_back(&d.join("gone"), &d.join("original")).unwrap_err();
        assert!(e.to_string().contains("no longer in the trash"), "{e}");
        assert!(!d.join("original").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn put_back_makes_a_missing_folder_again_and_moves_a_folder_whole() {
        let d = temp_dir("panoply-putback-dir");
        let trashed = d.join("pages");
        std::fs::create_dir(&trashed).unwrap();
        std::fs::write(trashed.join("1.png"), "p").unwrap();
        let original = d.join("old").join("deeper").join("pages");

        put_back(&trashed, &original).unwrap();

        assert!(!trashed.exists());
        assert_eq!(
            std::fs::read_to_string(original.join("1.png")).unwrap(),
            "p"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Where the test file is made. Linux uses a folder beside the home trash so the move is a
    /// rename inside one drive; elsewhere the temp folder is fine.
    fn trash_test_dir() -> PathBuf {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let d = freedesktop::data_home().join(unique("panoply-trash-dir", "d"));
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            temp_dir("panoply-trash-real")
        }
    }

    #[cfg(target_os = "macos")]
    fn find_in_system_trash(_original: &Path, name: &str) -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        let p = PathBuf::from(home).join(".Trash").join(name);
        p.exists().then_some(p)
    }

    #[cfg(target_os = "macos")]
    fn remove_from_system_trash(found: &Path, _name: &str) {
        let _ = std::fs::remove_file(found);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn find_in_system_trash(original: &Path, name: &str) -> Option<PathBuf> {
        let trash = freedesktop::data_home().join("Trash");
        let info =
            std::fs::read_to_string(trash.join("info").join(format!("{name}.trashinfo"))).ok()?;
        let want = freedesktop::encode_path(original);
        assert!(info.starts_with("[Trash Info]\n"), "{info}");
        assert!(info.contains(&format!("Path={want}\n")), "{info}");
        let p = trash.join("files").join(name);
        p.exists().then_some(p)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn remove_from_system_trash(found: &Path, name: &str) {
        let _ = std::fs::remove_file(found);
        if let Some(trash) = found.parent().and_then(Path::parent) {
            let _ = std::fs::remove_file(trash.join("info").join(format!("{name}.trashinfo")));
        }
    }

    #[cfg(windows)]
    fn find_in_system_trash(original: &Path, _name: &str) -> Option<PathBuf> {
        win::tests::find_in_recycle_bin(original)
    }

    #[cfg(windows)]
    fn remove_from_system_trash(found: &Path, _name: &str) {
        win::tests::remove_from_recycle_bin(found);
    }
}
