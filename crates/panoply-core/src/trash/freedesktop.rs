//! Linux and other Unix systems: the freedesktop.org trash specification, version 1.0.
//!
//! A trashed item is two files. The item itself goes to `<trash>/files/<name>`, and
//! `<trash>/info/<name>.trashinfo` records where it came from and when, which is how a file manager
//! puts it back. The home trash is `$XDG_DATA_HOME/Trash` (`~/.local/share/Trash`). A file on another
//! drive goes to that drive's own trash instead (`$topdir/.Trash/<uid>` when the administrator made
//! one, else `$topdir/.Trash-<uid>`), so a move is a rename and never copies a large file. Only when
//! the other drive refuses a trash folder does a plain file get copied into the home trash.

// On macOS this module is built for its tests only; the real trash call there is in mac.rs.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::TrashError;

/// `$XDG_DATA_HOME`, or `~/.local/share` when it is unset, empty or not absolute.
pub(super) fn data_home() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        let d = PathBuf::from(d);
        if d.is_absolute() {
            return d;
        }
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.local/share"))
}

pub(super) fn trash(path: &Path) -> Result<PathBuf, TrashError> {
    if std::env::var_os("HOME").is_none() && std::env::var_os("XDG_DATA_HOME").is_none() {
        return Err(TrashError::failed("there is no home folder"));
    }
    trash_in(path, &data_home().join("Trash"), effective_uid())
}

/// Remove the `.trashinfo` record of an item that was taken out of `<trash>/files/` again, so the
/// file manager does not list an item that is no longer there. Does nothing for a path that is not
/// inside a trash `files` folder.
pub(super) fn forget(trashed: &Path) {
    let (Some(files), Some(name)) = (trashed.parent(), trashed.file_name()) else {
        return;
    };
    if files.file_name() != Some(OsStr::new("files")) {
        return;
    }
    let Some(trash) = files.parent() else {
        return;
    };
    let mut info_name = name.to_os_string();
    info_name.push(".trashinfo");
    let _ = fs::remove_file(trash.join("info").join(info_name));
}

/// Trash `path` into `home_trash` (or the trash of its own drive). Returns where the item went.
pub(super) fn trash_in(path: &Path, home_trash: &Path, uid: u32) -> Result<PathBuf, TrashError> {
    let abs = resolve(path)?;
    let meta = fs::symlink_metadata(&abs).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => TrashError::NotFound,
        _ => TrashError::failed(e),
    })?;
    let name = abs
        .file_name()
        .ok_or_else(|| TrashError::failed("it has no name"))?
        .to_os_string();

    make_trash_dirs(home_trash).map_err(TrashError::failed)?;
    let home_dev = fs::metadata(home_trash)
        .map(|m| m.dev())
        .map_err(TrashError::failed)?;

    if meta.dev() == home_dev {
        return move_into(&abs, &name, home_trash, &encode_path(&abs));
    }

    // Another drive: its own trash first, then the copy fallback.
    let top = top_dir(&abs, meta.dev());
    for candidate in drive_trash_candidates(&top, uid) {
        if make_trash_dirs(&candidate).is_ok() {
            let rel = abs.strip_prefix(&top).unwrap_or(&abs);
            return move_into(&abs, &name, &candidate, &encode_path(rel));
        }
    }
    if meta.is_dir() {
        return Err(TrashError::NoTrash);
    }
    copy_into(&abs, &name, home_trash, &encode_path(&abs))
}

/// An absolute path with the folders resolved but the item itself not followed, so a link is
/// trashed as a link.
fn resolve(path: &Path) -> Result<PathBuf, TrashError> {
    let abs = std::path::absolute(path).map_err(TrashError::failed)?;
    match (abs.parent(), abs.file_name()) {
        (Some(dir), Some(name)) => {
            let dir = fs::canonicalize(dir).map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => TrashError::NotFound,
                _ => TrashError::failed(e),
            })?;
            Ok(dir.join(name))
        }
        _ => Err(TrashError::failed("it has no name")),
    }
}

/// The highest folder above `path` that is still on drive `dev`: the mount point.
fn top_dir(path: &Path, dev: u64) -> PathBuf {
    let mut top = path.parent().unwrap_or(path).to_path_buf();
    for dir in path.ancestors().skip(1) {
        match fs::metadata(dir) {
            Ok(m) if m.dev() == dev => top = dir.to_path_buf(),
            _ => break,
        }
    }
    top
}

/// The trash folders the spec allows on a drive, best first.
fn drive_trash_candidates(top: &Path, uid: u32) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let shared = top.join(".Trash");
    if let Ok(m) = fs::symlink_metadata(&shared) {
        // Only when the administrator set it up: a real folder with the sticky bit.
        if m.is_dir() && m.permissions().mode() & 0o1000 != 0 {
            v.push(shared.join(uid.to_string()));
        }
    }
    v.push(top.join(format!(".Trash-{uid}")));
    v
}

fn make_trash_dirs(trash: &Path) -> io::Result<()> {
    for sub in ["files", "info"] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(trash.join(sub))?;
    }
    Ok(())
}

/// Claim a free name by creating its info file (which cannot clash, whoever else is trashing), then
/// rename the item into `files`.
fn move_into(
    abs: &Path,
    name: &OsStr,
    trash: &Path,
    recorded_path: &str,
) -> Result<PathBuf, TrashError> {
    let (info, target) = claim(name, trash, recorded_path)?;
    match fs::rename(abs, &target) {
        Ok(()) => Ok(target),
        Err(e) => {
            let _ = fs::remove_file(&info);
            Err(TrashError::failed(e))
        }
    }
}

/// The fallback for a plain file whose drive has no usable trash: copy it into the home trash, and
/// only when the copy is complete remove the original. A failure removes the copy, not the original.
fn copy_into(
    abs: &Path,
    name: &OsStr,
    trash: &Path,
    recorded_path: &str,
) -> Result<PathBuf, TrashError> {
    let (info, target) = claim(name, trash, recorded_path)?;
    let undo = |target: &Path| {
        let _ = fs::remove_file(target);
        let _ = fs::remove_file(&info);
    };
    if let Err(e) = fs::copy(abs, &target) {
        undo(&target);
        return Err(TrashError::failed(e));
    }
    if let Err(e) = fs::remove_file(abs) {
        undo(&target);
        return Err(TrashError::failed(e));
    }
    Ok(target)
}

/// Create `info/<name>.trashinfo` for the first free name and return it with the `files` path.
fn claim(
    name: &OsStr,
    trash: &Path,
    recorded_path: &str,
) -> Result<(PathBuf, PathBuf), TrashError> {
    let files = trash.join("files");
    let info_dir = trash.join("info");
    let record = format!(
        "[Trash Info]\nPath={recorded_path}\nDeletionDate={}\n",
        local_stamp()
    );
    for n in 1u32..10_000 {
        let mut candidate = name.to_os_string();
        if n > 1 {
            candidate.push(format!(".{n}"));
        }
        let target = files.join(&candidate);
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let mut info_name = candidate.clone();
        info_name.push(".trashinfo");
        let info = info_dir.join(info_name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(record.as_bytes()) {
                    let _ = fs::remove_file(&info);
                    return Err(TrashError::failed(e));
                }
                return Ok((info, target));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(TrashError::failed(e)),
        }
    }
    Err(TrashError::failed("too many items with the same name"))
}

/// A path as the `Path=` key wants it: percent-encoded bytes, with `/` and the unreserved
/// characters left alone.
pub(super) fn encode_path(path: &Path) -> String {
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        match b {
            b'/' | b'.' | b'-' | b'_' | b'~' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[allow(unsafe_code)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments, no side effects and cannot fail.
    unsafe { libc::geteuid() }
}

/// The local time as `YYYY-MM-DDThh:mm:ss`, which is what `DeletionDate=` wants (no time zone).
#[allow(unsafe_code)]
fn local_stamp() -> String {
    // SAFETY: `time` accepts a null pointer. `localtime_r` reads `now` and writes only into `tm`,
    // which is a plain C struct of integers, valid when zeroed.
    let tm = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            None
        } else {
            Some(tm)
        }
    };
    match tm {
        Some(tm) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        ),
        None => "1970-01-01T00:00:00".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{temp_dir, unique};
    use super::*;

    const UID: u32 = 4242;

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    #[test]
    fn the_item_moves_and_an_info_file_says_where_from() {
        let d = temp_dir("panoply-fd-basic");
        let home = d.join("Trash");
        let src = d.join("x (compressed).pdf");
        fs::write(&src, "body").unwrap();

        let at = trash_in(&src, &home, UID).unwrap();

        assert!(!src.exists());
        assert_eq!(at, home.join("files/x (compressed).pdf"));
        assert_eq!(read(&at), "body");
        let info = read(&home.join("info/x (compressed).pdf.trashinfo"));
        let want = format!(
            "Path={}/x%20%28compressed%29.pdf\n",
            encode_path(&fs::canonicalize(&d).unwrap())
        );
        assert!(info.starts_with("[Trash Info]\n"), "{info}");
        assert!(info.contains(&want), "{info}");
        let stamp = info.split("DeletionDate=").nth(1).unwrap().trim();
        let b = stamp.as_bytes();
        assert_eq!(stamp.len(), 19, "{stamp}");
        assert!(b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':');
        assert_eq!(
            fs::metadata(home.join("files")).unwrap().mode() & 0o777,
            0o700
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn two_files_with_one_name_both_survive() {
        let d = temp_dir("panoply-fd-clash");
        let home = d.join("Trash");
        let mut spots = Vec::new();
        for body in ["first", "second", "third"] {
            let src = d.join("same.txt");
            fs::write(&src, body).unwrap();
            spots.push(trash_in(&src, &home, UID).unwrap());
        }
        assert_eq!(read(&spots[0]), "first");
        assert_eq!(read(&spots[1]), "second");
        assert_eq!(read(&spots[2]), "third");
        assert!(home.join("info/same.txt.2.trashinfo").exists());
        assert!(home.join("info/same.txt.3.trashinfo").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_folder_and_a_link_are_moved_as_themselves() {
        let d = temp_dir("panoply-fd-kinds");
        let home = d.join("Trash");
        let folder = d.join("pages");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("1.png"), "p").unwrap();
        let target = d.join("real.txt");
        fs::write(&target, "real").unwrap();
        let link = d.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let f = trash_in(&folder, &home, UID).unwrap();
        let l = trash_in(&link, &home, UID).unwrap();

        assert!(!folder.exists() && f.join("1.png").exists());
        assert!(fs::symlink_metadata(&l).unwrap().file_type().is_symlink());
        assert_eq!(
            read(&target),
            "real",
            "the link's target must not be touched"
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_missing_item_is_an_error_and_makes_no_info_file() {
        let d = temp_dir("panoply-fd-missing");
        let home = d.join("Trash");
        let e = trash_in(&d.join("nope"), &home, UID).unwrap_err();
        assert!(matches!(e, TrashError::NotFound));
        assert!(
            fs::read_dir(home.join("info"))
                .map(|mut r| r.next().is_none())
                .unwrap_or(true)
        );
        fs::remove_dir_all(d).unwrap();
    }

    /// A file on another drive (tmpfs at /dev/shm, when there is one) goes to that drive's own
    /// `.Trash-<uid>`, with a path relative to the drive's top, and is not copied to the home trash.
    #[test]
    fn a_file_on_another_drive_goes_to_that_drives_trash() {
        let shm = Path::new("/dev/shm");
        let d = temp_dir("panoply-fd-drive");
        let home = d.join("Trash");
        let same_drive = match (fs::metadata(shm), fs::metadata(&d)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev(),
            _ => true,
        };
        if same_drive {
            eprintln!("skipped: no second drive to test with");
            fs::remove_dir_all(d).unwrap();
            return;
        }
        let name = unique("panoply-fd-shm", "txt");
        let src = shm.join(&name);
        fs::write(&src, "on tmpfs").unwrap();
        let trash_dir = shm.join(format!(".Trash-{UID}"));

        let at = trash_in(&src, &home, UID).unwrap();

        assert!(!src.exists());
        assert_eq!(at, trash_dir.join("files").join(&name));
        assert_eq!(read(&at), "on tmpfs");
        let info = read(&trash_dir.join("info").join(format!("{name}.trashinfo")));
        assert!(info.contains(&format!("Path={name}\n")), "{info}");
        assert!(!home.join("files").join(&name).exists());
        let _ = fs::remove_dir_all(trash_dir);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_failed_move_keeps_the_file_and_leaves_no_info_file() {
        // A trash whose `files` folder cannot take the item: `files` is a plain file.
        let d = temp_dir("panoply-fd-fail");
        let home = d.join("Trash");
        let src = d.join("keep.txt");
        fs::write(&src, "keep me").unwrap();
        fs::create_dir_all(home.join("info")).unwrap();
        fs::write(home.join("files"), "in the way").unwrap();

        let e = trash_in(&src, &home, UID).unwrap_err();

        assert!(matches!(e, TrashError::Failed { .. }), "{e}");
        assert_eq!(read(&src), "keep me");
        assert!(fs::read_dir(home.join("info")).unwrap().next().is_none());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn put_back_restores_the_item_and_removes_its_info_file() {
        let d = temp_dir("panoply-fd-putback");
        let home = d.join("Trash");
        let src = d.join("keep.txt");
        fs::write(&src, "body").unwrap();
        let at = trash_in(&src, &home, UID).unwrap();
        let info = home.join("info/keep.txt.trashinfo");
        assert!(info.exists());

        super::super::move_back(&at, &src).unwrap();
        forget(&at);

        assert_eq!(read(&src), "body");
        assert!(!at.exists());
        assert!(!info.exists(), "the info file must go with the item");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn put_back_refused_keeps_the_info_file() {
        let d = temp_dir("panoply-fd-putback-busy");
        let home = d.join("Trash");
        let src = d.join("keep.txt");
        fs::write(&src, "old").unwrap();
        let at = trash_in(&src, &home, UID).unwrap();
        fs::write(&src, "new").unwrap();

        assert!(super::super::move_back(&at, &src).is_err());

        assert_eq!(read(&at), "old");
        assert_eq!(read(&src), "new");
        assert!(home.join("info/keep.txt.trashinfo").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn forget_ignores_a_path_outside_a_files_folder() {
        let d = temp_dir("panoply-fd-forget");
        fs::create_dir_all(d.join("info")).unwrap();
        fs::write(d.join("info/x.trashinfo"), "keep").unwrap();
        forget(&d.join("x"));
        assert!(d.join("info/x.trashinfo").exists());
        fs::remove_dir_all(d).unwrap();
    }
}
