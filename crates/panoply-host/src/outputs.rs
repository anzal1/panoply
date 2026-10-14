//! Result files for built-in modules: named by the never-overwrite rule, written under a
//! temporary name, and moved into place only when complete, so a cancelled or failed job leaves
//! nothing behind (FLOWS.md, shared capability 9).

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use panoply_core::naming::unique_output_path_with;

use crate::{CapabilityGuard, ModuleError};

fn io_err(path: &Path, source: std::io::Error) -> ModuleError {
    ModuleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A file being written. It has a hidden temporary name until [`Outputs::commit`].
pub struct Pending {
    part: PathBuf,
    file: Option<File>,
    dir: PathBuf,
    kind: Kind,
    done: bool,
}

enum Kind {
    /// `<stem> (<label>).<ext>` placed next to `beside`.
    Labelled {
        beside: PathBuf,
        label: String,
        ext: String,
    },
    /// An exact file name inside a folder, with `(2)` added on a clash.
    Named { name: String },
}

impl Pending {
    /// The open temporary file.
    pub fn file(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("a pending output is open until committed")
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.done {
            self.file.take();
            let _ = std::fs::remove_file(&self.part);
        }
    }
}

/// The files and folders one job creates. Dropping it without [`Outputs::finish`] removes them.
pub struct Outputs<'g> {
    guard: &'g CapabilityGuard,
    reserved: HashSet<PathBuf>,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
    counter: u32,
    armed: bool,
}

/// Make a string safe as a file name on every platform.
pub fn sanitize_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let t = t.trim().trim_matches('.').trim().to_string();
    let t: String = t.chars().take(120).collect();
    if t.is_empty() { "output".into() } else { t }
}

impl<'g> Outputs<'g> {
    /// Start tracking the outputs of one job.
    pub fn new(guard: &'g CapabilityGuard) -> Self {
        Outputs {
            guard,
            reserved: HashSet::new(),
            files: Vec::new(),
            dirs: Vec::new(),
            counter: 0,
            armed: true,
        }
    }

    /// A new folder named `<stem> (<label>)` next to `beside` (or in the chosen output folder).
    pub fn dir(&mut self, beside: &Path, label: &str) -> Result<PathBuf, ModuleError> {
        let base = self.guard.output_dir_for(beside)?;
        let stem = beside
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut n = 1u32;
        loop {
            let name = if n == 1 {
                format!("{stem} ({label})")
            } else {
                format!("{stem} ({label} {n})")
            };
            let candidate = base.join(name);
            n += 1;
            if candidate.exists() || self.reserved.contains(&candidate) {
                if n > 10_000 {
                    return Err(io_err(
                        &base,
                        std::io::Error::from(ErrorKind::AlreadyExists),
                    ));
                }
                continue;
            }
            match std::fs::create_dir(&candidate) {
                Ok(()) => {
                    self.reserved.insert(candidate.clone());
                    self.dirs.push(candidate.clone());
                    return Ok(candidate);
                }
                // Someone made it between the check and the create: try the next name.
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err(&candidate, e)),
            }
        }
    }

    /// A file that will be named `<stem> (<label>).<ext>` next to `beside` once committed.
    pub fn file(&mut self, beside: &Path, label: &str, ext: &str) -> Result<Pending, ModuleError> {
        let dir = self.guard.output_dir_for(beside)?;
        self.pending(
            dir,
            Kind::Labelled {
                beside: beside.to_path_buf(),
                label: label.to_string(),
                ext: ext.to_string(),
            },
        )
    }

    /// A file with an exact name inside `dir` (a folder this job made).
    pub fn file_in(&mut self, dir: &Path, name: &str) -> Result<Pending, ModuleError> {
        self.pending(
            dir.to_path_buf(),
            Kind::Named {
                name: sanitize_name(name),
            },
        )
    }

    fn pending(&mut self, dir: PathBuf, kind: Kind) -> Result<Pending, ModuleError> {
        loop {
            self.counter += 1;
            let part = dir.join(format!(
                ".panoply-{}-{}.part",
                std::process::id(),
                self.counter
            ));
            match OpenOptions::new().write(true).create_new(true).open(&part) {
                Ok(file) => {
                    return Ok(Pending {
                        part,
                        file: Some(file),
                        dir,
                        kind,
                        done: false,
                    });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err(&part, e)),
            }
        }
    }

    /// Move a finished file to its final name. Never overwrites.
    pub fn commit(&mut self, mut p: Pending) -> Result<PathBuf, ModuleError> {
        if let Some(f) = p.file.take() {
            f.sync_all().map_err(|e| io_err(&p.part, e))?;
        }
        loop {
            let target = match &p.kind {
                Kind::Labelled { beside, label, ext } => {
                    unique_output_path_with(beside, Some(&p.dir), label, Some(ext), |c| {
                        c.exists() || self.reserved.contains(c)
                    })
                }
                Kind::Named { name } => {
                    let stem = Path::new(name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let ext = Path::new(name)
                        .extension()
                        .map(|s| s.to_string_lossy().into_owned());
                    let mut k = 1;
                    loop {
                        let n = match (k, &ext) {
                            (1, _) => name.clone(),
                            (k, Some(e)) => format!("{stem} ({k}).{e}"),
                            (k, None) => format!("{stem} ({k})"),
                        };
                        let c = p.dir.join(n);
                        if !c.exists() && !self.reserved.contains(&c) {
                            break c;
                        }
                        k += 1;
                    }
                }
            };
            match publish(&p.part, &target) {
                Ok(()) => {
                    p.done = true;
                    self.reserved.insert(target.clone());
                    self.files.push(target.clone());
                    return Ok(target);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    self.reserved.insert(target);
                }
                Err(e) => return Err(io_err(&target, e)),
            }
        }
    }

    /// The files written so far.
    pub fn written(&self) -> &[PathBuf] {
        &self.files
    }

    /// The job succeeded: keep everything and return the files.
    pub fn finish(mut self) -> Vec<PathBuf> {
        self.armed = false;
        std::mem::take(&mut self.files)
    }

    /// Remove every file and folder this job created.
    pub fn rollback(&mut self) {
        for f in self.files.drain(..) {
            let _ = std::fs::remove_file(f);
        }
        for d in self.dirs.drain(..).rev() {
            let _ = std::fs::remove_dir(d);
        }
    }
}

impl Drop for Outputs<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.rollback();
        }
    }
}

/// Put `part` at `target` without ever replacing something that is there.
fn publish(part: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(part, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(part);
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(e),
        Err(_) => {
            // No hard links here (some network and exFAT volumes): rename after a check.
            if target.exists() {
                Err(std::io::Error::from(ErrorKind::AlreadyExists))
            } else {
                std::fs::rename(part, target)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-outputs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn guard_for(input: &Path) -> CapabilityGuard {
        CapabilityGuard::new(&[input.to_path_buf()], None, false).unwrap()
    }

    #[test]
    fn files_land_next_to_the_input_and_never_overwrite() {
        let d = scratch("next");
        let input = d.join("report.pdf");
        std::fs::write(&input, "x").unwrap();
        std::fs::write(d.join("report (compressed).pdf"), "keep").unwrap();
        let g = guard_for(&input);
        let mut o = Outputs::new(&g);
        let mut p = o.file(&input, "compressed", "pdf").unwrap();
        p.file().write_all(b"new").unwrap();
        let path = o.commit(p).unwrap();
        assert_eq!(path.file_name().unwrap(), "report (compressed 2).pdf");
        assert_eq!(
            std::fs::read_to_string(d.join("report (compressed).pdf")).unwrap(),
            "keep"
        );
        let files = o.finish();
        assert_eq!(files.len(), 1);
        // No temporary leftovers.
        assert!(
            std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().ends_with(".part"))
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn dropping_without_finish_removes_everything() {
        let d = scratch("rollback");
        let input = d.join("a.pdf");
        std::fs::write(&input, "x").unwrap();
        let g = guard_for(&input);
        {
            let mut o = Outputs::new(&g);
            let dir = o.dir(&input, "split").unwrap();
            assert!(dir.ends_with("a (split)"));
            let mut p = o.file_in(&dir, "a 1.pdf").unwrap();
            p.file().write_all(b"1").unwrap();
            o.commit(p).unwrap();
            let mut half = o.file_in(&dir, "a 2.pdf").unwrap();
            half.file().write_all(b"2").unwrap();
            // never committed
        }
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            1,
            "only the input is left"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn folders_and_named_files_count_up_on_clashes() {
        let d = scratch("clash");
        let input = d.join("a.pdf");
        std::fs::write(&input, "x").unwrap();
        std::fs::create_dir(d.join("a (split)")).unwrap();
        let g = guard_for(&input);
        let mut o = Outputs::new(&g);
        let dir = o.dir(&input, "split").unwrap();
        assert!(dir.ends_with("a (split 2)"), "{dir:?}");
        for _ in 0..2 {
            let mut p = o.file_in(&dir, "part.pdf").unwrap();
            p.file().write_all(b"x").unwrap();
            o.commit(p).unwrap();
        }
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"part.pdf".to_string()) && names.contains(&"part (2).pdf".to_string()),
            "{names:?}"
        );
        o.finish();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_chosen_output_folder_wins_and_unchosen_inputs_cannot_place_files() {
        let d = scratch("chosen");
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let input = d.join("a.pdf");
        std::fs::write(&input, "x").unwrap();
        let g = CapabilityGuard::new(std::slice::from_ref(&input), Some(&out), false).unwrap();
        let mut o = Outputs::new(&g);
        let mut p = o.file(&input, "x", "pdf").unwrap();
        p.file().write_all(b"1").unwrap();
        let path = o.commit(p).unwrap();
        assert!(path.starts_with(out.canonicalize().unwrap()));
        o.finish();
        let g = guard_for(&input);
        let stranger = d.join("other.pdf");
        std::fs::write(&stranger, "x").unwrap();
        let mut o = Outputs::new(&g);
        assert!(matches!(
            o.file(&stranger, "x", "pdf"),
            Err(ModuleError::NotAllowed(_))
        ));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(sanitize_name("a/b:c*d"), "a-b-c-d");
        assert_eq!(sanitize_name("  ..  "), "output");
    }
}
