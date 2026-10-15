//! Output naming. Originals are never touched, and an output never overwrites a file.
//!
//! `report.pdf` with suffix `compressed` becomes `report (compressed).pdf`, then
//! `report (compressed 2).pdf`, and so on.

use std::path::{Path, PathBuf};

/// Pick a free output path next to `input` (or inside `out_dir` when given).
///
/// `suffix` is the label in brackets, `ext` replaces the extension when given
/// (without the dot), otherwise the input's extension is kept.
pub fn unique_output_path(
    input: &Path,
    out_dir: Option<&Path>,
    suffix: &str,
    ext: Option<&str>,
) -> PathBuf {
    unique_output_path_with(input, out_dir, suffix, ext, |p| p.exists())
}

/// Same as [`unique_output_path`] with an injectable existence check, for tests and for
/// callers that also want to avoid names reserved by in-flight jobs.
pub fn unique_output_path_with(
    input: &Path,
    out_dir: Option<&Path>,
    suffix: &str,
    ext: Option<&str>,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let dir = match out_dir {
        Some(d) => d.to_path_buf(),
        None => input.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = match ext {
        Some(e) => Some(e.trim_start_matches('.').to_string()),
        None => input.extension().map(|e| e.to_string_lossy().into_owned()),
    };
    let mut n = 1u32;
    loop {
        let label = if n == 1 {
            suffix.to_string()
        } else {
            format!("{suffix} {n}")
        };
        let mut name = format!("{stem} ({label})");
        if let Some(e) = &ext {
            name.push('.');
            name.push_str(e);
        }
        let candidate = dir.join(name);
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn first_name_has_plain_suffix() {
        let p = unique_output_path_with(Path::new("/d/name.pdf"), None, "compressed", None, |_| {
            false
        });
        assert_eq!(p, PathBuf::from("/d/name (compressed).pdf"));
    }

    #[test]
    fn collisions_count_up_from_two() {
        let taken: HashSet<PathBuf> = ["/d/name (compressed).pdf", "/d/name (compressed 2).pdf"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let p = unique_output_path_with(Path::new("/d/name.pdf"), None, "compressed", None, |c| {
            taken.contains(c)
        });
        assert_eq!(p, PathBuf::from("/d/name (compressed 3).pdf"));
    }

    #[test]
    fn extension_can_change_and_dir_can_move() {
        let p = unique_output_path_with(
            Path::new("/d/photo.heic"),
            Some(Path::new("/out")),
            "converted",
            Some(".jpg"),
            |_| false,
        );
        assert_eq!(p, PathBuf::from("/out/photo (converted).jpg"));
    }

    #[test]
    fn no_extension_is_fine() {
        let p = unique_output_path_with(Path::new("/d/README"), None, "copy", None, |_| false);
        assert_eq!(p, PathBuf::from("/d/README (copy)"));
    }

    #[test]
    fn dotted_names_keep_inner_dots() {
        let p = unique_output_path_with(Path::new("/d/a.b.tar.gz"), None, "x", None, |_| false);
        assert_eq!(p, PathBuf::from("/d/a.b.tar (x).gz"));
    }

    #[test]
    fn never_overwrites_a_real_file() {
        let dir = std::env::temp_dir().join(format!("panoply-naming-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("doc.pdf");
        std::fs::write(&input, b"x").unwrap();
        let first = unique_output_path(&input, None, "compressed", None);
        std::fs::write(&first, b"x").unwrap();
        let second = unique_output_path(&input, None, "compressed", None);
        assert_ne!(first, second);
        assert!(second.to_string_lossy().ends_with("(compressed 2).pdf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// What an operating system file error means for a person, without the system's own wording
/// ("No space left on device (os error 28)"). Used wherever a read or write fails.
pub fn plain_io(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::StorageFull => "the disk is full".into(),
        K::PermissionDenied => "Panoply does not have permission to write there".into(),
        K::ReadOnlyFilesystem => "the disk is read-only".into(),
        K::NotFound => "the folder is not there any more".into(),
        K::QuotaExceeded => "the disk quota is used up".into(),
        _ => {
            // Drop the "(os error N)" tail the standard library adds.
            let t = e.to_string();
            match t.rfind(" (os error") {
                Some(i) => t[..i].to_string(),
                None => t,
            }
        }
    }
}

#[cfg(test)]
mod plain_io_tests {
    use super::plain_io;
    use std::io::{Error, ErrorKind};

    #[test]
    fn system_wording_and_codes_never_reach_a_person() {
        assert_eq!(
            plain_io(&Error::from(ErrorKind::StorageFull)),
            "the disk is full"
        );
        assert_eq!(
            plain_io(&Error::from(ErrorKind::PermissionDenied)),
            "Panoply does not have permission to write there"
        );
        // The same code means different things on different systems; none of them may show it.
        for code in [2, 5, 13, 28] {
            let text = plain_io(&Error::from_raw_os_error(code));
            assert!(!text.contains("os error"), "{code}: {text}");
        }
    }
}
