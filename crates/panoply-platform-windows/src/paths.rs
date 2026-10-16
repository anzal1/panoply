//! Path helpers for the watcher. Pure string work, so it is tested on every platform.

use std::path::{Path, PathBuf};

/// Removes the `\\?\` prefix `GetFinalPathNameByHandleW` adds: `\\?\C:\x` becomes `C:\x` and
/// `\\?\UNC\srv\share` becomes `\\srv\share`.
pub fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    }
}

/// The device path of the volume a path with a drive letter is on: `C:\Users` gives `\\.\C:`. `None`
/// for a path with no drive letter (a network share, a folder mounted without a letter).
pub fn volume_device(path: &str) -> Option<String> {
    let b = path.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        Some(format!(r"\\.\{}:", (b[0] as char).to_ascii_uppercase()))
    } else {
        None
    }
}

/// Re-roots a resolved path: when `resolved_path` is `resolved_root` or below it (compared without
/// regard to ASCII case, on a component boundary), returns the same relative part under `orig_root`,
/// the root exactly as the index knows it.
pub fn rebase(resolved_root: &str, resolved_path: &str, orig_root: &Path) -> Option<PathBuf> {
    let root = resolved_root.trim_end_matches('\\');
    let rb = root.as_bytes();
    let pb = resolved_path.as_bytes();
    if pb.len() < rb.len() || !pb[..rb.len()].eq_ignore_ascii_case(rb) {
        return None;
    }
    let rest = &resolved_path[rb.len()..];
    if rest.is_empty() {
        return Some(orig_root.to_path_buf());
    }
    let rel = rest.strip_prefix('\\')?;
    Some(orig_root.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\a"), r"C:\Users\a");
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(strip_verbatim(r"C:\plain"), r"C:\plain");
    }

    #[test]
    fn volumes() {
        assert_eq!(volume_device(r"c:\Users").as_deref(), Some(r"\\.\C:"));
        assert_eq!(volume_device(r"D:\").as_deref(), Some(r"\\.\D:"));
        assert_eq!(volume_device(r"\\srv\share"), None);
        assert_eq!(volume_device(""), None);
    }

    #[test]
    fn rebasing() {
        let orig = Path::new(r"C:\Users\me");
        assert_eq!(
            rebase(r"C:\Users\Me", r"c:\users\me\docs\a.txt", orig),
            Some(orig.join(r"docs\a.txt"))
        );
        assert_eq!(
            rebase(r"C:\Users\me\", r"C:\Users\me", orig),
            Some(orig.to_path_buf())
        );
        // A sibling that merely shares the prefix is not below the root.
        assert_eq!(rebase(r"C:\Users\me", r"C:\Users\meow\a", orig), None);
        assert_eq!(rebase(r"C:\Users\me", r"C:\Other\a", orig), None);
        assert_eq!(rebase(r"C:\Users\me", r"C:\Us", orig), None);
    }
}
