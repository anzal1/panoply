//! Run a module over a set of files and collect one result per file.

use std::path::PathBuf;

use serde::Serialize;

use crate::Result;
use crate::runner::{self, Runner};
use crate::stats;

/// One file to run.
#[derive(Debug, Clone)]
pub struct Input {
    /// Corpus id, or the file name for ad hoc files.
    pub id: String,
    /// Corpus kind, or `file` for ad hoc files.
    pub kind: String,
    /// Where the file is.
    pub path: PathBuf,
    /// Size on disk in bytes.
    pub bytes: u64,
}

/// What to measure besides memory.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Runs per file. Wall and CPU are medians, memory is the maximum.
    pub runs: usize,
    /// Treat the module as an instant transform and check the first preview budget.
    pub instant: bool,
    /// Files up to this size count as a "typical file" for the first preview budget, bytes.
    pub typical_bytes: u64,
    /// A native command to compare speed with, `{input}` marks the file.
    pub baseline: Option<String>,
    /// Allowed ratio of our wall time to the baseline's. 1.10 gives a 10% noise margin.
    pub speed_tolerance: f64,
    /// Only compare speed on files at least this large, bytes (below it both are process start).
    pub baseline_min_bytes: u64,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            runs: 3,
            instant: false,
            typical_bytes: 10_000_000,
            baseline: None,
            speed_tolerance: 1.10,
            baseline_min_bytes: 4_000_000,
        }
    }
}

/// The native tool's numbers on the same file.
#[derive(Debug, Clone, Serialize)]
pub struct BaselineResult {
    /// Median wall time, milliseconds.
    pub wall_ms: f64,
    /// Highest peak memory, bytes.
    pub peak_mem_bytes: u64,
    /// Our median wall time divided by the baseline's.
    pub ratio: f64,
}

/// One file's measurements.
#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    /// Corpus id or file name.
    pub id: String,
    /// Corpus kind or `file`.
    pub kind: String,
    /// Input size in bytes.
    pub input_bytes: u64,
    /// Runs measured.
    pub runs: usize,
    /// Median wall time, milliseconds.
    pub wall_ms: f64,
    /// Slowest run, milliseconds.
    pub wall_ms_max: f64,
    /// Median CPU time, milliseconds.
    pub cpu_ms: f64,
    /// Median time inside the registry call, milliseconds (worker runner only).
    pub inner_ms: Option<f64>,
    /// Median instructions retired by the child, in millions (macOS). Finer than CPU time.
    pub minstr: Option<f64>,
    /// Highest peak memory over the runs, bytes (footprint where available).
    pub peak_mem_bytes: u64,
    /// Highest peak resident set size, bytes.
    pub max_rss_bytes: u64,
    /// Highest peak physical footprint, bytes (macOS).
    pub peak_footprint_bytes: Option<u64>,
    /// Bytes the module wrote (median run).
    pub output_bytes: u64,
    /// True when every run succeeded. A failing module is still measured: it must stay in budget.
    pub ok: bool,
    /// First error.
    pub error: Option<String>,
    /// Native tool comparison, when requested and the file is large enough.
    pub baseline: Option<BaselineResult>,
}

/// All files of one run.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    /// Module id.
    pub module: String,
    /// How the module was started.
    pub runner: String,
    /// Runs per file.
    pub runs: usize,
    /// One entry per file.
    pub files: Vec<FileResult>,
    /// Matching corpus entries that were not on disk (run `fetch.py` and `generate.py`).
    pub skipped_missing: usize,
    /// The options in effect.
    pub instant: bool,
    /// Typical file limit in bytes.
    pub typical_bytes: u64,
    /// Baseline command, when one was used.
    pub baseline_cmd: Option<String>,
    /// Speed tolerance in effect.
    pub speed_tolerance: f64,
}

/// Run `module` on every input and measure it.
pub fn run_inputs(
    runner: &Runner,
    module: &str,
    inputs: &[Input],
    opts: &RunOptions,
    skipped_missing: usize,
    mut progress: impl FnMut(usize, usize, &Input),
) -> Result<RunReport> {
    let runs = opts.runs.max(1);
    let mut files = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        progress(i + 1, inputs.len(), input);
        let mut walls = Vec::new();
        let mut cpus = Vec::new();
        let mut inners = Vec::new();
        let mut instrs = Vec::new();
        let mut outs = Vec::new();
        let (mut peak, mut rss) = (0u64, 0u64);
        let mut footprint: Option<u64> = None;
        let mut error = None;
        for _ in 0..runs {
            let s = runner::sample(runner, module, &input.path)?;
            walls.push(s.wall_ms);
            cpus.push(s.cpu_ms);
            if let Some(x) = s.inner_ms {
                inners.push(x);
            }
            if let Some(i) = s.instructions {
                instrs.push(i as f64 / 1e6);
            }
            outs.push(s.output_bytes as f64);
            peak = peak.max(s.peak_mem_bytes());
            rss = rss.max(s.max_rss_bytes);
            if let Some(f) = s.peak_footprint_bytes {
                footprint = Some(footprint.map_or(f, |x| x.max(f)));
            }
            if !s.ok && error.is_none() {
                error = s.error.clone().or(Some("failed".into()));
            }
        }
        let wall = stats::median(&walls);
        let baseline = match &opts.baseline {
            Some(cmd) if input.bytes >= opts.baseline_min_bytes => {
                let mut bw = Vec::new();
                let mut bp = 0u64;
                for _ in 0..runs {
                    let b = runner::sample_baseline(cmd, &input.path)?;
                    bw.push(b.wall_ms);
                    bp = bp.max(b.peak_mem_bytes());
                }
                let bm = stats::median(&bw);
                Some(BaselineResult {
                    wall_ms: bm,
                    peak_mem_bytes: bp,
                    ratio: if bm > 0.0 { wall / bm } else { 0.0 },
                })
            }
            _ => None,
        };
        files.push(FileResult {
            id: input.id.clone(),
            kind: input.kind.clone(),
            input_bytes: input.bytes,
            runs,
            wall_ms: wall,
            wall_ms_max: stats::max(&walls),
            cpu_ms: stats::median(&cpus),
            inner_ms: (!inners.is_empty()).then(|| stats::median(&inners)),
            minstr: (!instrs.is_empty()).then(|| stats::median(&instrs)),
            peak_mem_bytes: peak,
            max_rss_bytes: rss,
            peak_footprint_bytes: footprint,
            output_bytes: stats::median(&outs) as u64,
            ok: error.is_none(),
            error,
            baseline,
        });
    }
    Ok(RunReport {
        module: module.to_string(),
        runner: runner.label(),
        runs,
        files,
        skipped_missing,
        instant: opts.instant,
        typical_bytes: opts.typical_bytes,
        baseline_cmd: opts.baseline.clone(),
        speed_tolerance: opts.speed_tolerance,
    })
}
