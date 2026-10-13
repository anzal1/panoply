//! Does memory stay flat as the input grows?
//!
//! FLOWS.md says peak memory is "constant for video and audio regardless of length (streamed)"
//! and that "a 2 GB PDF or 20 GB video works within the same memory cap". Neither claim can be
//! tested with a corpus you can download, so this module makes the input: a sparse file of any
//! size, created on the fly with `set_len` (no data is written, the filesystem stores holes),
//! run through the module in a child process, and deleted afterwards.
//!
//! A module that streams has the same peak at 1 MB and at 2 GB. A module that reads the whole
//! file does not, and the spread between the smallest and the largest peak shows it.
//!
//! Sparse files read back as zeros, so this exercises I/O and memory behavior, not decode paths
//! that depend on content. The real files in the corpus cover those.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::runner::{self, Runner, Scratch};
use crate::stats;
use crate::{BenchError, Result};

/// Default tolerance for "constant": the largest peak may exceed the smallest by this much.
pub const DEFAULT_TOLERANCE_BYTES: u64 = 8_000_000;

/// A sparse file that deletes itself (and its folder) when dropped.
pub struct SparseFile {
    path: PathBuf,
    bytes: u64,
    _scratch: Scratch,
}

impl SparseFile {
    /// Create a sparse file of `bytes` in `dir` (the system temp folder when `None`).
    /// Refuses when the filesystem does not make it sparse, so a typo cannot fill the disk.
    pub fn create(bytes: u64, dir: Option<&Path>) -> Result<SparseFile> {
        let scratch = match dir {
            Some(d) => Scratch::new_in(d)?,
            None => Scratch::new()?,
        };
        let path = scratch.path().join(format!("sparse-{bytes}.bin"));
        let f = File::create(&path)
            .map_err(|e| BenchError::io(format!("creating {}", path.display()), e))?;
        f.set_len(bytes)
            .map_err(|e| BenchError::io(format!("sizing {}", path.display()), e))?;
        drop(f);
        let me = SparseFile {
            path,
            bytes,
            _scratch: scratch,
        };
        if let Some(allocated) = me.allocated_bytes()
            && bytes > 64 << 20
            && allocated > bytes / 100
        {
            return Err(BenchError::Usage(format!(
                "this filesystem allocated {} for a {} sparse file; refusing to continue. \
                 Pass --tmp-dir on a filesystem with sparse file support (APFS, ext4, xfs).",
                stats::human_bytes(allocated),
                stats::human_bytes(bytes)
            )));
        }
        Ok(me)
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Logical size.
    pub fn len(&self) -> u64 {
        self.bytes
    }

    /// True for a zero byte file.
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }

    /// Bytes the filesystem actually allocated for the file, where the platform says.
    #[cfg(unix)]
    pub fn allocated_bytes(&self) -> Option<u64> {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&self.path).ok().map(|m| m.blocks() * 512)
    }

    /// Bytes the filesystem actually allocated for the file, where the platform says.
    #[cfg(not(unix))]
    pub fn allocated_bytes(&self) -> Option<u64> {
        None
    }
}

/// One size in a scale run.
#[derive(Debug, Clone, Serialize)]
pub struct ScalePoint {
    /// Logical size of the input in bytes.
    pub input_bytes: u64,
    /// Bytes the filesystem allocated for it (proves it was sparse).
    pub allocated_bytes: Option<u64>,
    /// Runs measured at this size.
    pub runs: usize,
    /// Median wall time in milliseconds.
    pub wall_ms: f64,
    /// Median CPU time in milliseconds.
    pub cpu_ms: f64,
    /// Highest peak memory over the runs, bytes (footprint where available).
    pub peak_mem_bytes: u64,
    /// Highest peak resident set size over the runs, bytes.
    pub max_rss_bytes: u64,
    /// Input bytes processed per second, in megabytes, from the median wall time.
    pub throughput_mb_s: f64,
    /// True when every run succeeded.
    pub ok: bool,
    /// First error, if any run failed.
    pub error: Option<String>,
}

/// The result of a scale run.
#[derive(Debug, Clone, Serialize)]
pub struct ScaleReport {
    /// Module id.
    pub module: String,
    /// How the module was started.
    pub runner: String,
    /// One entry per size, smallest first.
    pub points: Vec<ScalePoint>,
    /// Allowed spread between the largest and smallest peak, bytes.
    pub tolerance_bytes: u64,
    /// Largest peak minus smallest peak, bytes.
    pub spread_bytes: u64,
    /// Peak memory growth per input byte between the smallest and largest size.
    pub bytes_per_input_byte: f64,
    /// True when the spread is within the tolerance and every run succeeded.
    pub constant: bool,
}

/// Run `module` over a sparse file of each size and summarise the memory behavior.
pub fn run_scale(
    runner: &Runner,
    module: &str,
    sizes: &[u64],
    runs: usize,
    tmp_dir: Option<&Path>,
    tolerance_bytes: u64,
    mut progress: impl FnMut(&str),
) -> Result<ScaleReport> {
    let mut sizes = sizes.to_vec();
    sizes.sort_unstable();
    sizes.dedup();
    if sizes.is_empty() {
        return Err(BenchError::Usage("give at least one size".into()));
    }
    let mut points = Vec::new();
    for &bytes in &sizes {
        let file = SparseFile::create(bytes, tmp_dir)?;
        progress(&format!(
            "  {} sparse file ({} allocated)",
            stats::human_bytes(bytes),
            file.allocated_bytes()
                .map_or("unknown".to_string(), stats::human_bytes)
        ));
        let mut walls = Vec::new();
        let mut cpus = Vec::new();
        let (mut peak, mut rss) = (0u64, 0u64);
        let mut error = None;
        for _ in 0..runs.max(1) {
            let s = runner::sample(runner, module, file.path())?;
            walls.push(s.wall_ms);
            cpus.push(s.cpu_ms);
            peak = peak.max(s.peak_mem_bytes());
            rss = rss.max(s.max_rss_bytes);
            if !s.ok && error.is_none() {
                error = s.error.clone().or(Some("failed".into()));
            }
        }
        let wall = stats::median(&walls);
        points.push(ScalePoint {
            input_bytes: bytes,
            allocated_bytes: file.allocated_bytes(),
            runs: runs.max(1),
            wall_ms: wall,
            cpu_ms: stats::median(&cpus),
            peak_mem_bytes: peak,
            max_rss_bytes: rss,
            throughput_mb_s: if wall > 0.0 {
                bytes as f64 / 1e6 / (wall / 1000.0)
            } else {
                0.0
            },
            ok: error.is_none(),
            error,
        });
        // `file` drops here: the sparse file and its folder are deleted before the next size.
    }
    Ok(summarize(runner.label(), module, points, tolerance_bytes))
}

/// Compute the spread and the verdict from measured points.
pub fn summarize(
    runner: String,
    module: &str,
    points: Vec<ScalePoint>,
    tolerance_bytes: u64,
) -> ScaleReport {
    let lo = points.iter().map(|p| p.peak_mem_bytes).min().unwrap_or(0);
    let hi = points.iter().map(|p| p.peak_mem_bytes).max().unwrap_or(0);
    let spread = hi - lo;
    let slope = match (points.first(), points.last()) {
        (Some(a), Some(b)) if b.input_bytes > a.input_bytes => {
            (b.peak_mem_bytes as f64 - a.peak_mem_bytes as f64)
                / (b.input_bytes - a.input_bytes) as f64
        }
        _ => 0.0,
    };
    let all_ok = points.iter().all(|p| p.ok);
    ScaleReport {
        module: module.to_string(),
        runner,
        points,
        tolerance_bytes,
        spread_bytes: spread,
        bytes_per_input_byte: slope,
        constant: all_ok && spread <= tolerance_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(input: u64, peak: u64) -> ScalePoint {
        ScalePoint {
            input_bytes: input,
            allocated_bytes: Some(0),
            runs: 1,
            wall_ms: 10.0,
            cpu_ms: 10.0,
            peak_mem_bytes: peak,
            max_rss_bytes: peak,
            throughput_mb_s: 100.0,
            ok: true,
            error: None,
        }
    }

    #[test]
    fn flat_memory_is_constant() {
        let r = summarize(
            "r".into(),
            "m",
            vec![point(1 << 20, 2_000_000), point(1 << 31, 2_400_000)],
            DEFAULT_TOLERANCE_BYTES,
        );
        assert!(r.constant);
        assert_eq!(r.spread_bytes, 400_000);
        assert!(r.bytes_per_input_byte < 0.001);
    }

    #[test]
    fn memory_that_tracks_input_is_not_constant() {
        // A module that reads the whole file: peak grows with the input.
        let r = summarize(
            "r".into(),
            "m",
            vec![point(1 << 20, 2_000_000), point(1 << 31, 2_150_000_000)],
            DEFAULT_TOLERANCE_BYTES,
        );
        assert!(!r.constant);
        assert!(r.bytes_per_input_byte > 0.9);
    }

    #[test]
    fn a_failed_run_is_never_constant() {
        let mut p = point(1 << 20, 1_000_000);
        p.ok = false;
        p.error = Some("boom".into());
        assert!(!summarize("r".into(), "m", vec![p], DEFAULT_TOLERANCE_BYTES).constant);
    }

    #[test]
    fn sparse_file_is_sparse_and_deleted() {
        let path;
        {
            let f = SparseFile::create(256 << 20, None).unwrap();
            path = f.path().to_path_buf();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), 256 << 20);
            assert_eq!(f.len(), 256 << 20);
            if let Some(a) = f.allocated_bytes() {
                assert!(a < 8 << 20, "allocated {a}");
            }
        }
        assert!(!path.exists(), "sparse file must be deleted on drop");
    }
}
