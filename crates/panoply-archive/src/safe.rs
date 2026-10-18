//! The rules that keep an archive inside its folder. Pure functions, tested on their own.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Why an entry was refused. The text finishes the sentence "<name> ...".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// `/etc/passwd`, `C:\x`, `\\server\share`.
    Absolute,
    /// A `..` component.
    Traversal,
    /// A name with a NUL byte or nothing in it.
    BadName,
    /// A link whose target is absolute, empty or could lead outside.
    LinkOutside,
    /// An earlier entry made this path's parent a link, or a file.
    BlockedParent,
    /// Links cannot be made here (Windows without the privilege, other systems).
    #[cfg_attr(unix, allow(dead_code))]
    LinkUnsupported,
    /// A file type that is not a file, a folder or a link (device, pipe, hard link).
    SpecialFile,
}

impl Reject {
    pub fn reason(&self) -> &'static str {
        match self {
            Reject::Absolute => "starts at the root of the disk, not inside the folder",
            Reject::Traversal => "uses \"..\" to climb out of the folder",
            Reject::BadName => "has a name that this system cannot use",
            Reject::LinkOutside => "is a link that points outside the folder",
            Reject::BlockedParent => "would be written through a link or over a file",
            Reject::LinkUnsupported => "is a link, and links are not supported on this system",
            Reject::SpecialFile => {
                "is a device, a pipe, or a hard link to something that was not unpacked"
            }
        }
    }
}

/// The longest single name most file systems hold, in bytes.
const MAX_COMPONENT: usize = 255;

/// True when the system refused a name (too long, not allowed here) rather than failing for a
/// reason that has nothing to do with the entry, like a full disk. Such an entry is skipped; it
/// must not throw away everything unpacked so far.
pub fn is_bad_name(e: &std::io::Error) -> bool {
    use std::io::ErrorKind as K;
    if matches!(e.kind(), K::InvalidFilename | K::InvalidInput) {
        return true;
    }
    #[cfg(target_os = "macos")]
    const NAME_TOO_LONG: i32 = 63;
    #[cfg(target_os = "windows")]
    const NAME_TOO_LONG: i32 = 206;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const NAME_TOO_LONG: i32 = 36;
    e.raw_os_error() == Some(NAME_TOO_LONG)
}

/// What a cleaned entry name is.
#[derive(Debug, PartialEq, Eq)]
pub enum Cleaned {
    /// The archive's own root (`./`, empty): nothing to do.
    Root,
    /// Normal components, relative, nothing odd.
    Path(Vec<String>),
}

/// Turn the name an archive gave an entry into safe relative components, or say why not.
/// `backslash_is_separator` is true for zip, whose names come from Windows tools.
pub fn clean_name(raw: &str, backslash_is_separator: bool) -> Result<Cleaned, Reject> {
    if raw.contains('\0') {
        return Err(Reject::BadName);
    }
    let s: String = if backslash_is_separator {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };
    if s.starts_with('/') {
        return Err(Reject::Absolute);
    }
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Err(Reject::Absolute);
    }
    let mut parts = Vec::new();
    for c in s.split('/') {
        match c {
            "" | "." => {}
            ".." => return Err(Reject::Traversal),
            c if c.len() > MAX_COMPONENT => return Err(Reject::BadName),
            c => parts.push(platform_component(c)),
        }
    }
    if parts.is_empty() {
        return Ok(Cleaned::Root);
    }
    if parts.iter().map(|p| p.len() + 1).sum::<usize>() > 4096 {
        return Err(Reject::BadName);
    }
    Ok(Cleaned::Path(parts))
}

/// Windows cannot hold `:` or `?` in a name and treats `CON` and `NUL` as devices. Other systems
/// keep the name as it is.
fn platform_component(c: &str) -> String {
    if !cfg!(windows) {
        return c.to_string();
    }
    let mut s: String = c
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if device || s.is_empty() {
        s.insert(0, '_');
    }
    s
}

/// Check a link's target. A safe target is relative and has the shape `../../x/y`: some `..` steps
/// that stay inside the folder (no more than the link's own depth), then only plain names. A `..`
/// after a plain name is refused, because an earlier name could itself be a link and the climb
/// would start somewhere else than it looks.
pub fn link_target_ok(target: &str, link_depth: usize) -> Result<(), Reject> {
    if target.is_empty() || target.contains('\0') {
        return Err(Reject::LinkOutside);
    }
    if target.starts_with('/') || target.starts_with('\\') {
        return Err(Reject::LinkOutside);
    }
    let b = target.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Err(Reject::LinkOutside);
    }
    let mut ups = 0usize;
    let mut seen_name = false;
    for c in target.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                if seen_name {
                    return Err(Reject::LinkOutside);
                }
                ups += 1;
                if ups > link_depth {
                    return Err(Reject::LinkOutside);
                }
            }
            _ => seen_name = true,
        }
    }
    Ok(())
}

/// Create a new file in `dir` named `name`, or `name (2)`, `name (3)`... when taken. Never opens
/// an existing file and never follows a link at the final name.
pub fn create_unique(dir: &Path, name: &OsStr) -> std::io::Result<(PathBuf, File, bool)> {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    let mut k = 1u32;
    loop {
        let candidate = if k == 1 {
            name.to_os_string()
        } else {
            match &ext {
                Some(e) => format!("{stem} ({k}).{e}").into(),
                None => format!("{stem} ({k})").into(),
            }
        };
        let full = dir.join(&candidate);
        match OpenOptions::new().write(true).create_new(true).open(&full) {
            Ok(f) => return Ok((full, f, k > 1)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                k += 1;
                if k > 100_000 {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> Result<Cleaned, Reject> {
        clean_name(raw, true)
    }

    #[test]
    fn plain_names_pass_and_dots_are_dropped() {
        assert_eq!(
            p("a/b/c.txt").unwrap(),
            Cleaned::Path(vec!["a".into(), "b".into(), "c.txt".into()])
        );
        assert_eq!(
            p("./a//b/").unwrap(),
            Cleaned::Path(vec!["a".into(), "b".into()])
        );
        assert_eq!(p("./").unwrap(), Cleaned::Root);
        assert_eq!(p("").unwrap(), Cleaned::Root);
    }

    #[test]
    fn traversal_and_absolute_names_are_refused() {
        assert_eq!(p("../evil.txt"), Err(Reject::Traversal));
        assert_eq!(p("a/../../evil.txt"), Err(Reject::Traversal));
        assert_eq!(p("..\\evil.txt"), Err(Reject::Traversal));
        assert_eq!(p("/etc/passwd"), Err(Reject::Absolute));
        assert_eq!(p("\\\\server\\share"), Err(Reject::Absolute));
        assert_eq!(p("C:\\Windows\\x"), Err(Reject::Absolute));
        assert_eq!(p("c:/x"), Err(Reject::Absolute));
        assert_eq!(p("a\0b"), Err(Reject::BadName));
        // A backslash is a plain character in a tar name.
        assert_eq!(
            clean_name("..\\x", false).unwrap(),
            Cleaned::Path(vec!["..\\x".into()])
        );
    }

    #[test]
    fn long_components_are_refused_and_the_system_says_so_too() {
        let long = "x".repeat(300);
        assert_eq!(p(&format!("a/{long}")), Err(Reject::BadName));
        assert!(p(&format!("a/{}", "y".repeat(255))).is_ok());
        assert!(is_bad_name(&std::io::Error::from(
            std::io::ErrorKind::InvalidFilename
        )));
        assert!(!is_bad_name(&std::io::Error::from(
            std::io::ErrorKind::StorageFull
        )));
        // The real thing: this system's own error for a name that is too long.
        let d = std::env::temp_dir().join(format!("panoply-archive-long-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let err = std::fs::File::create(d.join(&long)).unwrap_err();
        assert!(is_bad_name(&err), "{err:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn link_targets_stay_inside() {
        assert!(link_target_ok("file.txt", 0).is_ok());
        assert!(link_target_ok("sub/file.txt", 0).is_ok());
        assert!(link_target_ok("../other", 1).is_ok());
        assert!(link_target_ok("../../other", 1).is_err());
        assert!(link_target_ok("..", 0).is_err());
        assert!(link_target_ok("/etc/passwd", 3).is_err());
        assert!(link_target_ok("C:\\x", 3).is_err());
        assert!(link_target_ok("", 3).is_err());
        // Down then up again: refused, an earlier name could be a link.
        assert!(link_target_ok("sub/../x", 3).is_err());
    }

    #[test]
    fn unique_names_add_a_suffix_before_the_extension() {
        let d = std::env::temp_dir().join(format!("panoply-archive-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (a, _, r1) = create_unique(&d, OsStr::new("a.txt")).unwrap();
        let (b, _, r2) = create_unique(&d, OsStr::new("a.txt")).unwrap();
        let (c, _, _) = create_unique(&d, OsStr::new("a.txt")).unwrap();
        assert!(!r1 && r2);
        assert_eq!(a.file_name().unwrap(), "a.txt");
        assert_eq!(b.file_name().unwrap(), "a (2).txt");
        assert_eq!(c.file_name().unwrap(), "a (3).txt");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
