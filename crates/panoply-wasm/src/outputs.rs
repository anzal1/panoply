//! Output files for one job. The module asks for a file by label and extension, and the host
//! picks the name with panoply-core's never-overwrite rule. Nothing the module says can
//! choose a folder or reuse an existing name.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use panoply_core::naming::unique_output_path_with;

/// Longest label or extension the host will put in a file name.
const MAX_PART: usize = 40;

struct Open {
    path: PathBuf,
    file: File,
}

/// Files a job has created, and the rules for creating them.
pub(crate) struct Outputs {
    out_dir: Option<PathBuf>,
    reserved: HashSet<PathBuf>,
    open: HashMap<u32, Open>,
    next: u32,
    finished: Vec<(String, PathBuf)>,
    created: Vec<PathBuf>,
}

fn clean(part: &str, fallback: &str) -> String {
    let s: String = part
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let s = s.trim().trim_matches('.').trim();
    let s: String = s.chars().take(MAX_PART).collect();
    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

fn clean_ext(ext: &str) -> String {
    let e: String = ext
        .trim_start_matches('.')
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    if e.is_empty() {
        "bin".to_string()
    } else {
        e.to_ascii_lowercase()
    }
}

fn write_err(path: &Path, e: &std::io::Error) -> String {
    let what = match e.kind() {
        ErrorKind::PermissionDenied => "Panoply does not have permission to write there",
        ErrorKind::StorageFull => "The disk is full",
        ErrorKind::NotFound => "The output folder is gone",
        _ => "The file could not be written",
    };
    format!(
        "{what} ({}). Check the folder and the free space, then try again.",
        path.display()
    )
}

impl Outputs {
    /// `out_dir` is the chosen output folder. Without one, outputs go next to the original.
    pub(crate) fn new(out_dir: Option<PathBuf>) -> Self {
        Self {
            out_dir,
            reserved: HashSet::new(),
            open: HashMap::new(),
            next: 1,
            finished: Vec::new(),
            created: Vec::new(),
        }
    }

    /// Create the next free output file and return its handle. `input` is the path the user
    /// chose for the file the output is built from, or `None` for modules that take no files.
    pub(crate) fn begin(
        &mut self,
        input: Option<&Path>,
        label: &str,
        ext: &str,
    ) -> Result<u32, String> {
        let label = clean(label, "output");
        let ext = clean_ext(ext);
        let (base, dir) = match (input, &self.out_dir) {
            (Some(p), Some(d)) => (p.to_path_buf(), d.clone()),
            (Some(p), None) => {
                let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
                (p.to_path_buf(), dir)
            }
            (None, Some(d)) => (PathBuf::from("output"), d.clone()),
            (None, None) => {
                return Err(
                    "Choose an output folder first: this module has no file to put its \
                            result next to."
                        .into(),
                );
            }
        };
        loop {
            let path = unique_output_path_with(&base, Some(&dir), &label, Some(&ext), |p| {
                self.reserved.contains(p) || p.exists()
            });
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let id = self.next;
                    self.next += 1;
                    self.reserved.insert(path.clone());
                    self.created.push(path.clone());
                    self.open.insert(id, Open { path, file });
                    return Ok(id);
                }
                // Someone else took the name between the check and the create: try the next.
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    self.reserved.insert(path);
                }
                Err(e) => return Err(write_err(&path, &e)),
            }
        }
    }

    pub(crate) fn append(&mut self, id: u32, data: &[u8]) -> Result<(), String> {
        let o = self.open.get_mut(&id).ok_or("That output is not open.")?;
        o.file.write_all(data).map_err(|e| write_err(&o.path, &e))
    }

    /// Close an output and return the name the host chose.
    pub(crate) fn finish(&mut self, id: u32) -> Result<String, String> {
        let mut o = self.open.remove(&id).ok_or("That output is not open.")?;
        o.file.flush().map_err(|e| write_err(&o.path, &e))?;
        let name = o
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.finished.push((name.clone(), o.path));
        Ok(name)
    }

    /// Delete outputs that were started and never finished.
    pub(crate) fn discard_unfinished(&mut self) {
        for (_, o) in self.open.drain() {
            drop(o.file);
            let _ = std::fs::remove_file(&o.path);
            self.created.retain(|p| p != &o.path);
        }
    }

    /// Delete everything this job created. Used when the job fails or is cancelled, so no
    /// partial file is ever left behind.
    pub(crate) fn rollback(&mut self) {
        self.open.clear();
        for p in self.created.drain(..) {
            let _ = std::fs::remove_file(p);
        }
        self.finished.clear();
    }

    /// The finished files, in the order they were finished.
    pub(crate) fn finished(&self) -> &[(String, PathBuf)] {
        &self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-outputs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_follow_core_and_never_collide() {
        let d = scratch("names");
        let input = d.join("notes.txt");
        let mut o = Outputs::new(None);
        let a = o.begin(Some(&input), "slugified", "txt").unwrap();
        let b = o.begin(Some(&input), "slugified", ".TXT").unwrap();
        assert_eq!(o.finish(a).unwrap(), "notes (slugified).txt");
        assert_eq!(o.finish(b).unwrap(), "notes (slugified 2).txt");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let d = scratch("exist");
        let input = d.join("a.txt");
        std::fs::write(d.join("a (x).txt"), "keep me").unwrap();
        let mut o = Outputs::new(Some(d.clone()));
        let id = o.begin(Some(&input), "x", "txt").unwrap();
        o.append(id, b"new").unwrap();
        assert_eq!(o.finish(id).unwrap(), "a (x 2).txt");
        assert_eq!(
            std::fs::read_to_string(d.join("a (x).txt")).unwrap(),
            "keep me"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn labels_cannot_escape_the_folder() {
        let d = scratch("escape");
        let input = d.join("a.txt");
        let mut o = Outputs::new(Some(d.clone()));
        let id = o.begin(Some(&input), "../../evil", "../sh").unwrap();
        let name = o.finish(id).unwrap();
        assert!(!name.contains('/') && !name.contains('\\'));
        assert!(d.join(&name).is_file());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn rollback_removes_everything_including_unfinished() {
        let d = scratch("rollback");
        let input = d.join("a.txt");
        let mut o = Outputs::new(None);
        let done = o.begin(Some(&input), "one", "txt").unwrap();
        o.finish(done).unwrap();
        let _half = o.begin(Some(&input), "two", "txt").unwrap();
        o.rollback();
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_input_and_no_folder_is_a_plain_error() {
        let mut o = Outputs::new(None);
        assert!(
            o.begin(None, "x", "txt")
                .unwrap_err()
                .contains("output folder")
        );
    }
}
