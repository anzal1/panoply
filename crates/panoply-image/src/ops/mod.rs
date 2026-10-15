//! The wave 1 image operations: convert, compress, resize, resize to exact KB, strip metadata.
//!
//! Each operation takes the host's [`Io`] (guarded file access, cancel, progress, report lines),
//! the input paths and the options, and returns the files it wrote. Outputs never overwrite
//! anything, and a job that fails or is cancelled leaves no partial file behind.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use panoply_core::CancelToken;

use crate::error::{ImageError, Result};

mod big;
pub mod common;
pub mod compress;
pub mod convert;
pub mod fit;
pub mod kb;
pub mod options;
pub mod resize;
pub mod search;
pub mod strip;

/// How an output file is named next to its input.
#[derive(Debug, Clone)]
pub enum NameSpec {
    /// `name.ext`, then `name (1).ext`, `name (2).ext` when taken (Convert).
    Plain,
    /// `name (label).ext`, then `name (label 2).ext` (Compress, Resize, Metadata).
    Label(String),
}

/// What an operation needs from its host.
pub trait Io {
    /// Open a chosen input for reading.
    fn open_read(&self, path: &Path) -> Result<File>;
    /// Create a brand new output file for `input`. Never overwrites.
    fn create_output(&self, input: &Path, name: &NameSpec, ext: &str) -> Result<(PathBuf, File)>;
    /// The cancel flag, polled between batches.
    fn cancel(&self) -> &CancelToken;
    /// Report progress, 0 to 1.
    fn progress(&self, fraction: f32, label: &str);
    /// Add a line to the result card.
    fn note(&self, line: String);
}

/// A plain file system [`Io`]: reads any path, writes next to the original (or into `out_dir`),
/// collects the card lines. Used by tests, examples and the benchmark worker.
pub struct FsIo {
    /// Where outputs go; `None` means next to each input.
    pub out_dir: Option<PathBuf>,
    cancel: CancelToken,
    notes: std::sync::Mutex<Vec<String>>,
}

impl FsIo {
    /// Outputs next to the inputs.
    pub fn new() -> Self {
        Self {
            out_dir: None,
            cancel: CancelToken::new(),
            notes: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Outputs into a folder.
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            out_dir: Some(dir.into()),
            ..Self::new()
        }
    }

    /// Take the card lines collected so far.
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut *self.notes.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl Default for FsIo {
    fn default() -> Self {
        Self::new()
    }
}

impl Io for FsIo {
    fn open_read(&self, path: &Path) -> Result<File> {
        File::open(path).map_err(|e| {
            ImageError::Io(format!(
                "Could not open {} ({e}). Check that the file was not moved or deleted.",
                path.display()
            ))
        })
    }

    fn create_output(&self, input: &Path, name: &NameSpec, ext: &str) -> Result<(PathBuf, File)> {
        let dir = match &self.out_dir {
            Some(d) => d.clone(),
            None => input.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        loop {
            let path = pick_name(input, &dir, name, ext, |p| p.exists());
            // Read and write: Compress checks a file by reading it back before it commits.
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => return Ok((path, f)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(ImageError::Io(format!(
                        "Could not write {}: {}. Check the folder and the free space, then try again.",
                        path.display(),
                        panoply_core::naming::plain_io(&e)
                    )));
                }
            }
        }
    }

    fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    fn progress(&self, _fraction: f32, _label: &str) {}

    fn note(&self, line: String) {
        self.notes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line);
    }
}

/// An output being written. Dropped without [`Output::commit`] it deletes the partial file.
pub struct Output {
    /// Where the file is.
    pub path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl Output {
    /// Create the output for `input`.
    pub fn create(io: &dyn Io, input: &Path, name: &NameSpec, ext: &str) -> Result<Output> {
        let (path, file) = io.create_output(input, name, ext)?;
        Ok(Output {
            path,
            file: Some(file),
            committed: false,
        })
    }

    /// The open file, for streaming writers.
    pub fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("output is open")
    }

    /// Write all bytes.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.file().write_all(bytes).map_err(ImageError::from)
    }

    /// Finish: the file stays.
    pub fn commit(mut self) -> PathBuf {
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
        }
        self.committed = true;
        self.path.clone()
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if !self.committed {
            drop(self.file.take());
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Pick a free name next to `input` (or inside `dir`), without touching the disk beyond
/// `exists`. Used by every [`Io`] implementation so naming is the same everywhere.
pub fn pick_name(
    input: &Path,
    dir: &Path,
    name: &NameSpec,
    ext: &str,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    let ext = ext.trim_start_matches('.');
    let mut n = 1u32;
    loop {
        let file = match (name, n) {
            (NameSpec::Plain, 1) => format!("{stem}.{ext}"),
            (NameSpec::Plain, n) => format!("{stem} ({}).{ext}", n - 1),
            (NameSpec::Label(l), 1) => format!("{stem} ({l}).{ext}"),
            (NameSpec::Label(l), n) => format!("{stem} ({l} {n}).{ext}"),
        };
        let candidate = dir.join(file);
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Human size, "3.2 MB", "610 KB", "412 bytes" (decimal, the way file managers show it).
pub fn human_bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.0} KB", n as f64 / 1e3)
    } else if n >= 1000 {
        format!("{:.1} KB", n as f64 / 1e3)
    } else {
        format!("{n} bytes")
    }
}

/// "n,nnn" with thousands separators.
pub fn with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_count_from_one() {
        // Built with `join`, so the expected paths use the platform's own separator.
        let d = Path::new("/d");
        let taken = [d.join("a.jpg"), d.join("a (1).jpg")];
        let p = pick_name(&d.join("a.heic"), d, &NameSpec::Plain, "jpg", |c| {
            taken.iter().any(|t| t == c)
        });
        assert_eq!(p, d.join("a (2).jpg"));
        let p = pick_name(&d.join("a.heic"), d, &NameSpec::Plain, "png", |_| false);
        assert_eq!(p, d.join("a.png"));
    }

    #[test]
    fn labels_count_from_two() {
        let d = Path::new("/d");
        let taken = [d.join("a (compressed).jpg")];
        let p = pick_name(
            &d.join("a.jpg"),
            d,
            &NameSpec::Label("compressed".into()),
            "jpg",
            |c| taken.iter().any(|t| t == c),
        );
        assert_eq!(p, d.join("a (compressed 2).jpg"));
    }

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(human_bytes(412), "412 bytes");
        assert_eq!(human_bytes(1500), "1.5 KB");
        assert_eq!(human_bytes(610_000), "610 KB");
        assert_eq!(human_bytes(3_200_000), "3.2 MB");
        assert_eq!(with_commas(48213), "48,213");
        assert_eq!(with_commas(999), "999");
    }
}
