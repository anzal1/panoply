//! What the engine needs from the host, and the staging area for result files.
//!
//! The host hands an [`Io`] to every run. It answers three questions: may this input be read,
//! which folder do results for it go to, and has the user cancelled. Results are written under
//! a hidden temporary name by the helper program, then moved to their final name only when the
//! whole job succeeded and only when nothing of that name exists, so a failed or cancelled job
//! leaves nothing behind and no file is ever overwritten (FLOWS.md, shared capabilities 4 and 9).

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use panoply_core::CancelToken;

use crate::error::{MediaError, Result};

/// The host side of a run.
pub trait Io {
    /// The real path of an input the user chose, or an error when it is not allowed.
    fn read_path(&self, path: &Path) -> Result<PathBuf>;
    /// The folder results for `input` are written to: the chosen output folder, or the folder
    /// the input is in.
    fn output_dir_for(&self, input: &Path) -> Result<PathBuf>;
    /// The cancel flag. Long work polls it.
    fn cancel(&self) -> &CancelToken;
    /// Report progress, 0.0 to 1.0.
    fn progress(&self, fraction: f32, label: &str);
    /// Add one line to the job's report (the result card).
    fn note(&self, line: String);
    /// True when this run may use the network (to fetch the helper engine once).
    fn network_allowed(&self) -> bool {
        true
    }

    /// Fail with [`MediaError::Cancelled`] when the user cancelled.
    fn check_cancel(&self) -> Result<()> {
        if self.cancel().is_cancelled() {
            Err(MediaError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// An [`Io`] for tests and tools that run outside the host: any path may be read and results go
/// next to the input (or into one folder).
pub struct OpenIo {
    cancel: CancelToken,
    out_dir: Option<PathBuf>,
    notes: std::sync::Mutex<Vec<String>>,
    progress: std::sync::Mutex<Vec<(f32, String)>>,
}

impl OpenIo {
    /// Results next to each input.
    pub fn new() -> Self {
        Self {
            cancel: CancelToken::new(),
            out_dir: None,
            notes: Default::default(),
            progress: Default::default(),
        }
    }

    /// Results into this folder.
    pub fn with_out_dir(mut self, dir: PathBuf) -> Self {
        self.out_dir = Some(dir);
        self
    }

    /// Use this cancel token.
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// The report lines collected so far.
    pub fn notes(&self) -> Vec<String> {
        self.notes.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The progress events collected so far.
    pub fn progress_events(&self) -> Vec<(f32, String)> {
        self.progress
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

impl Default for OpenIo {
    fn default() -> Self {
        Self::new()
    }
}

impl Io for OpenIo {
    fn read_path(&self, path: &Path) -> Result<PathBuf> {
        path.canonicalize().map_err(|e| missing(path, &e))
    }

    fn output_dir_for(&self, input: &Path) -> Result<PathBuf> {
        if let Some(d) = &self.out_dir {
            return Ok(d.clone());
        }
        let real = input.canonicalize().map_err(|e| missing(input, &e))?;
        real.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| MediaError::Io("There is no folder to write the result to.".into()))
    }

    fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    fn progress(&self, fraction: f32, label: &str) {
        self.progress
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((fraction, label.to_string()));
    }

    fn note(&self, line: String) {
        self.notes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line);
    }
}

fn missing(path: &Path, e: &std::io::Error) -> MediaError {
    match e.kind() {
        ErrorKind::NotFound => MediaError::Io(format!(
            "Could not find {}. Check that the file was not moved or deleted.",
            path.display()
        )),
        ErrorKind::PermissionDenied => MediaError::Io(format!(
            "Panoply does not have permission to open {}. Allow access to the folder in your system settings, then try again.",
            path.display()
        )),
        _ => MediaError::Io(format!(
            "Could not open {} ({e}). Check the file, then try again.",
            path.display()
        )),
    }
}

/// How a finished result is named.
#[derive(Debug, Clone)]
pub enum Naming {
    /// `<stem> (<label>).<ext>`, counting up on a clash: `<stem> (<label> 2).<ext>`.
    Labelled(String),
    /// `<stem>.<ext>` when free, then `<stem> (2).<ext>`.
    Plain,
}

/// The files and folders one job creates. Dropping it without [`Stage::finish`] removes them.
pub struct Stage {
    reserved: HashSet<PathBuf>,
    parts: Vec<PathBuf>,
    done: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
    counter: u32,
    armed: bool,
}

impl Default for Stage {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage {
    /// An empty stage.
    pub fn new() -> Self {
        Self {
            reserved: HashSet::new(),
            parts: Vec::new(),
            done: Vec::new(),
            dirs: Vec::new(),
            counter: 0,
            armed: true,
        }
    }

    /// A fresh hidden temporary path inside `dir` with this extension. The file is created
    /// (empty) so the name is reserved; the helper program writes over it.
    pub fn part(&mut self, dir: &Path, ext: &str) -> Result<PathBuf> {
        loop {
            self.counter += 1;
            let path = dir.join(format!(
                ".panoply-media-{}-{}.part.{}",
                std::process::id(),
                self.counter,
                ext.trim_start_matches('.')
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {
                    self.parts.push(path.clone());
                    return Ok(path);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(MediaError::Io(format!(
                        "Could not write in {}: {}. Check the folder and the free space, then try again.",
                        dir.display(),
                        panoply_core::naming::plain_io(&e)
                    )));
                }
            }
        }
    }

    /// A temporary path that is not created (the helper program makes it, for example a folder
    /// of pieces).
    pub fn scratch(&mut self, dir: &Path, tag: &str) -> PathBuf {
        self.counter += 1;
        let p = dir.join(format!(
            ".panoply-media-{}-{}.{}",
            std::process::id(),
            self.counter,
            tag
        ));
        self.parts.push(p.clone());
        p
    }

    /// Make a new folder named `<stem> (<label>)` next to `beside` (inside `dir`).
    pub fn dir(&mut self, dir: &Path, beside: &Path, label: &str) -> Result<PathBuf> {
        let stem = crate::units::stem_of(beside);
        let mut n = 1u32;
        loop {
            let name = if n == 1 {
                format!("{stem} ({label})")
            } else {
                format!("{stem} ({label} {n})")
            };
            let candidate = dir.join(name);
            n += 1;
            if candidate.exists() || self.reserved.contains(&candidate) {
                if n > 10_000 {
                    return Err(MediaError::Io("Too many folders with that name.".into()));
                }
                continue;
            }
            match std::fs::create_dir(&candidate) {
                Ok(()) => {
                    self.reserved.insert(candidate.clone());
                    self.dirs.push(candidate.clone());
                    return Ok(candidate);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(MediaError::Io(format!(
                        "Could not create a folder in {} ({e}).",
                        dir.display()
                    )));
                }
            }
        }
    }

    /// Move a finished temporary file to its final name in `dir`. Never overwrites.
    pub fn commit(
        &mut self,
        part: &Path,
        dir: &Path,
        input: &Path,
        naming: &Naming,
        ext: &str,
    ) -> Result<PathBuf> {
        let stem = crate::units::stem_of(input);
        let ext = ext.trim_start_matches('.');
        let mut n = 1u32;
        loop {
            let name = match (naming, n) {
                (Naming::Labelled(l), 1) => format!("{stem} ({l}).{ext}"),
                (Naming::Labelled(l), n) => format!("{stem} ({l} {n}).{ext}"),
                (Naming::Plain, 1) => format!("{stem}.{ext}"),
                (Naming::Plain, n) => format!("{stem} ({n}).{ext}"),
            };
            n += 1;
            let target = dir.join(name);
            if target.exists() || self.reserved.contains(&target) {
                continue;
            }
            match publish(part, &target) {
                Ok(()) => {
                    self.parts.retain(|p| p != part);
                    self.reserved.insert(target.clone());
                    self.done.push(target.clone());
                    return Ok(target);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    self.reserved.insert(target);
                }
                Err(e) => {
                    return Err(MediaError::Io(format!(
                        "Could not write {}: {}. Check the folder and the free space, then try again.",
                        target.display(),
                        panoply_core::naming::plain_io(&e)
                    )));
                }
            }
        }
    }

    /// Like [`Stage::commit`] with an exact file name (a `(2)` is added on a clash).
    pub fn commit_named(&mut self, part: &Path, dir: &Path, name: &str) -> Result<PathBuf> {
        let p = Path::new(name);
        let stem = crate::units::stem_of(p);
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut n = 1u32;
        loop {
            let file = match (n, ext.is_empty()) {
                (1, _) => name.to_string(),
                (n, true) => format!("{stem} ({n})"),
                (n, false) => format!("{stem} ({n}).{ext}"),
            };
            n += 1;
            let target = dir.join(file);
            if target.exists() || self.reserved.contains(&target) {
                continue;
            }
            match publish(part, &target) {
                Ok(()) => {
                    self.parts.retain(|x| x != part);
                    self.reserved.insert(target.clone());
                    self.done.push(target.clone());
                    return Ok(target);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    self.reserved.insert(target);
                }
                Err(e) => {
                    return Err(MediaError::Io(format!(
                        "Could not write {}: {}. Check the folder and the free space, then try again.",
                        target.display(),
                        panoply_core::naming::plain_io(&e)
                    )));
                }
            }
        }
    }

    /// Forget a temporary file that is no longer needed, and remove it.
    pub fn discard(&mut self, part: &Path) {
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_dir_all(part);
        self.parts.retain(|p| p != part);
    }

    /// The files committed so far.
    pub fn written(&self) -> &[PathBuf] {
        &self.done
    }

    /// The job succeeded: keep the committed files, remove any leftover temporary ones.
    pub fn finish(mut self) -> Vec<PathBuf> {
        self.armed = false;
        for p in self.parts.drain(..) {
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_dir_all(&p);
        }
        std::mem::take(&mut self.done)
    }

    /// Remove everything this job created.
    pub fn rollback(&mut self) {
        for p in self.parts.drain(..) {
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_dir_all(&p);
        }
        for f in self.done.drain(..) {
            let _ = std::fs::remove_file(f);
        }
        for d in self.dirs.drain(..).rev() {
            let _ = std::fs::remove_dir(d);
        }
    }
}

impl Drop for Stage {
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

/// The path to hand to the helper program: absolute, never starting with `-`, and without the
/// Windows extended-length prefix that ffmpeg does not read.
pub fn tool_path(path: &Path) -> String {
    let s = path.to_string_lossy().into_owned();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    if s.starts_with('-') {
        format!("./{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-media-io-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn results_never_overwrite_and_leave_no_parts() {
        let d = scratch("names");
        let input = d.join("clip.mov");
        std::fs::write(&input, "x").unwrap();
        std::fs::write(d.join("clip.mp4"), "keep").unwrap();
        let mut st = Stage::new();
        let p = st.part(&d, "mp4").unwrap();
        std::fs::File::create(&p)
            .unwrap()
            .write_all(b"new")
            .unwrap();
        let out = st.commit(&p, &d, &input, &Naming::Plain, "mp4").unwrap();
        assert_eq!(out.file_name().unwrap(), "clip (2).mp4");
        assert_eq!(std::fs::read_to_string(d.join("clip.mp4")).unwrap(), "keep");
        let files = st.finish();
        assert_eq!(files.len(), 1);
        assert!(
            std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().contains(".part."))
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn labelled_names_count_up() {
        let d = scratch("label");
        let input = d.join("a.mp4");
        std::fs::write(&input, "x").unwrap();
        std::fs::write(d.join("a (trimmed).mp4"), "k").unwrap();
        let mut st = Stage::new();
        let p = st.part(&d, "mp4").unwrap();
        let out = st
            .commit(&p, &d, &input, &Naming::Labelled("trimmed".into()), "mp4")
            .unwrap();
        assert_eq!(out.file_name().unwrap(), "a (trimmed 2).mp4");
        st.finish();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn dropping_without_finish_removes_everything() {
        let d = scratch("rollback");
        let input = d.join("a.mp4");
        std::fs::write(&input, "x").unwrap();
        {
            let mut st = Stage::new();
            let dir = st.dir(&d, &input, "streams").unwrap();
            let p = st.part(&dir, "mkv").unwrap();
            st.commit_named(&p, &dir, "a 1.mkv").unwrap();
            let _half = st.part(&d, "mp4").unwrap();
        }
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            1,
            "only the input is left"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tool_paths_cannot_look_like_options() {
        assert_eq!(tool_path(Path::new("-i")), "./-i");
        assert_eq!(tool_path(Path::new("/a/b.mp4")), "/a/b.mp4");
    }
}
