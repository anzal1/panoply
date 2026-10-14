//! The wave 1 operations. Each takes the chosen inputs and the resolved options, writes its
//! results through a [`crate::io::Stage`] (hidden temporary name first, never overwriting,
//! nothing left behind on failure or cancel) and reports plain lines for the result card.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::Engine;
use crate::error::{MediaError, Result};
use crate::io::{Io, Naming, Stage, tool_path};
use crate::probe::{self, Profile};
use crate::proc::{self, Progress};
use crate::units::{name_of, size_text};

pub mod audio;
pub mod compress;
pub mod convert;
pub mod extract_audio;
pub mod faststart;
pub mod hdr_to_sdr;
pub mod info;
pub mod normalize;
pub mod quality;
pub mod repair;
pub mod rewrite;
pub mod scan;
pub mod sizing;
pub mod streams;
pub mod to_mono;
pub mod trim;

/// Arguments for one helper run, built a piece at a time.
#[derive(Default, Clone)]
pub struct Args(pub Vec<OsString>);

impl Args {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one argument.
    pub fn a(&mut self, s: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        self.0.push(s.as_ref().to_os_string());
        self
    }

    /// Add a flag and its value.
    pub fn kv(&mut self, k: &str, v: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        self.0.push(OsString::from(k));
        self.0.push(v.as_ref().to_os_string());
        self
    }

    /// Add many arguments.
    pub fn all<I, S>(&mut self, it: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        for s in it {
            self.0.push(s.as_ref().to_os_string());
        }
        self
    }

    /// Add an input file.
    pub fn input(&mut self, path: &Path) -> &mut Self {
        self.kv("-i", tool_path(path))
    }

    /// The finished list.
    pub fn done(self) -> Vec<OsString> {
        self.0
    }
}

/// Rewrite `-name value` option names so they apply to output stream `k` of kind `kind`
/// (`v` or `a`): `-c:v` becomes `-c:v:1`, `-b:v` becomes `-b:v:1`, `-pix_fmt` becomes
/// `-pix_fmt:1`. Used when one command encodes more than one stream of a kind differently.
pub fn per_stream(args: &[String], kind: char, k: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with('-') && a.len() > 1 && !a[1..].starts_with(|c: char| c.is_ascii_digit()) {
            if a.ends_with(&format!(":{kind}")) {
                out.push(format!("{a}:{k}"));
            } else if a.contains(':') {
                out.push(a.clone());
            } else {
                out.push(format!("{a}:{kind}:{k}"));
            }
        } else {
            out.push(a.clone());
        }
        i += 1;
    }
    out
}

/// What one run of an operation shares across its inputs.
pub struct Job<'a> {
    /// The host.
    pub io: &'a dyn Io,
    /// The helper programs.
    pub engine: Arc<Engine>,
    /// Result files in progress.
    pub stage: Stage,
    /// This input covers `lo..hi` of the whole job's progress.
    pub lo: f32,
    /// See `lo`.
    pub hi: f32,
}

impl<'a> Job<'a> {
    /// Start a job: find (or fetch) the helper engine.
    pub fn new(io: &'a dyn Io) -> Result<Self> {
        io.check_cancel()?;
        let engine = Engine::get(io)?;
        Ok(Job {
            io,
            engine,
            stage: Stage::new(),
            lo: 0.0,
            hi: 1.0,
        })
    }

    /// Progress for input `i` of `n`.
    pub fn slice(&mut self, i: usize, n: usize) {
        self.lo = i as f32 / n as f32;
        self.hi = (i + 1) as f32 / n as f32;
    }

    /// The real path of an input the user chose.
    pub fn input(&self, path: &Path) -> Result<PathBuf> {
        self.io.read_path(path)
    }

    /// Probe a file.
    pub fn probe(&self, path: &Path) -> Result<Profile> {
        probe::probe(&self.engine, self.io, path)
    }

    /// Run ffmpeg. A non-zero exit becomes a plain error about `what`.
    pub fn ffmpeg(
        &self,
        args: Args,
        seconds: Option<f64>,
        label: &str,
        frac: (f32, f32),
        what: &Path,
    ) -> Result<()> {
        let lo = self.lo + (self.hi - self.lo) * frac.0;
        let hi = self.lo + (self.hi - self.lo) * frac.1;
        let progress = Progress::part(seconds, lo, hi, label);
        let done = proc::run_ffmpeg(&self.engine, self.io, &args.done(), &progress)?;
        if done.ok {
            Ok(())
        } else {
            Err(proc::explain(&done.tail, &name_of(what)))
        }
    }

    /// A temporary result path next to `input` (or in the chosen folder).
    pub fn part(&mut self, input: &Path, ext: &str) -> Result<(PathBuf, PathBuf)> {
        let dir = self.io.output_dir_for(input)?;
        let part = self.stage.part(&dir, ext)?;
        Ok((dir, part))
    }

    /// Move a finished temporary file to its final name.
    pub fn commit(
        &mut self,
        part: &Path,
        dir: &Path,
        input: &Path,
        naming: &Naming,
        ext: &str,
    ) -> Result<PathBuf> {
        self.stage.commit(part, dir, input, naming, ext)
    }

    /// Add a result card line.
    pub fn note(&self, line: impl Into<String>) {
        self.io.note(line.into());
    }

    /// Check a finished file: it must open, and its length must match what was asked within a
    /// small tolerance. A mismatch is reported, not hidden.
    pub fn verify(&self, out: &Path, expect_seconds: Option<f64>) -> Result<Profile> {
        let p = probe::probe_trusted(&self.engine, self.io, out).map_err(|_| {
            MediaError::Failed(format!(
                "The result for {} could not be read back, so it was not kept. Try again, or try another format.",
                name_of(out)
            ))
        })?;
        if let (Some(want), Some(got)) = (expect_seconds, p.duration)
            && (want - got).abs() > (0.5_f64).max(want * 0.02)
        {
            self.note(format!(
                "Heads up: the result is {} long and the original is {}.",
                crate::units::duration_text(got),
                crate::units::duration_text(want)
            ));
        }
        Ok(p)
    }

    /// Keep the committed files and drop any leftovers.
    pub fn finish(self) -> Vec<PathBuf> {
        self.stage.finish()
    }
}

/// "12.3 MB to 11.9 MB, 3% smaller" or "bigger".
pub fn change_text(before: u64, after: u64) -> String {
    if before == 0 {
        return size_text(after);
    }
    let pct = (1.0 - after as f64 / before as f64) * 100.0;
    if pct >= 1.0 {
        format!(
            "{} to {}, {:.0}% smaller",
            size_text(before),
            size_text(after),
            pct
        )
    } else if pct <= -1.0 {
        format!(
            "{} to {}, {:.0}% bigger",
            size_text(before),
            size_text(after),
            -pct
        )
    } else {
        format!("{} to {}", size_text(before), size_text(after))
    }
}

/// Prefix a message with the file name when a job has several inputs.
pub fn with_name(many: bool, path: &Path, e: MediaError) -> MediaError {
    if !many {
        return e;
    }
    let n = name_of(path);
    match e {
        MediaError::Cancelled => e,
        MediaError::BadOption(m) => MediaError::BadOption(format!("{n}: {m}")),
        MediaError::Unsupported(m) => MediaError::Unsupported(format!("{n}: {m}")),
        MediaError::Corrupt(m) => MediaError::Corrupt(format!("{n}: {m}")),
        MediaError::Failed(m) => MediaError::Failed(format!("{n}: {m}")),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_stream_rewrites_names_not_values() {
        let a: Vec<String> = [
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "4000000",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "60",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = per_stream(&a, 'v', 1);
        assert_eq!(
            r,
            [
                "-c:v:1",
                "h264_videotoolbox",
                "-b:v:1",
                "4000000",
                "-pix_fmt:v:1",
                "yuv420p",
                "-g:v:1",
                "60"
            ]
        );
    }

    #[test]
    fn change_text_is_plain() {
        assert!(change_text(10_000_000, 5_000_000).contains("50% smaller"));
        assert!(change_text(1_000_000, 2_000_000).contains("bigger"));
        assert_eq!(change_text(1_000_000, 1_000_000), "1.0 MB to 1.0 MB");
    }
}
