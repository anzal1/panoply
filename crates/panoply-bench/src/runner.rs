//! How a module is started, and one measured sample of it.
//!
//! Two runners:
//!
//! - [`Runner::Worker`] runs `panoply-bench worker`, which calls the host registry directly.
//!   It works for every registered module, including ones with no CLI subcommand yet.
//! - [`Runner::Cli`] runs the shipped `panoply` binary with an argument template such as
//!   `hash {input}`. That is the path a user's script takes.
//!
//! Either way the module runs in its own process and gets its own output folder, which is
//! measured and removed after the run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::measure::{self, Measured, TimeStats};
use crate::worker::WorkerReport;
use crate::{BenchError, Result};

/// Placeholder for the input file in a command template.
pub const INPUT: &str = "{input}";
/// Placeholder for the output folder in a command template.
pub const OUT_DIR: &str = "{out_dir}";

/// How to start the module under test.
#[derive(Debug, Clone)]
pub enum Runner {
    /// This binary's `worker` subcommand.
    Worker {
        /// Path of the harness binary.
        exe: PathBuf,
    },
    /// The `panoply` command line with an argument template.
    Cli {
        /// Path of the `panoply` binary.
        bin: PathBuf,
        /// Arguments, with `{input}` and `{out_dir}` replaced per run.
        args: Vec<String>,
    },
}

impl Runner {
    /// Short name for reports.
    pub fn label(&self) -> String {
        match self {
            Runner::Worker { .. } => "registry (panoply-bench worker)".to_string(),
            Runner::Cli { bin, args } => format!("cli ({} {})", bin.display(), args.join(" ")),
        }
    }

    /// The program and arguments for one run.
    pub fn command(&self, module: &str, input: &Path, out_dir: &Path) -> (PathBuf, Vec<OsString>) {
        match self {
            Runner::Worker { exe } => (
                exe.clone(),
                vec![
                    "worker".into(),
                    "--module".into(),
                    module.into(),
                    "--out-dir".into(),
                    out_dir.into(),
                    input.into(),
                ],
            ),
            Runner::Cli { bin, args } => (bin.clone(), expand(args, input, out_dir)),
        }
    }
}

/// Replace the placeholders in an argument template.
pub fn expand(template: &[String], input: &Path, out_dir: &Path) -> Vec<OsString> {
    template
        .iter()
        .map(|a| {
            if a == INPUT {
                input.as_os_str().to_owned()
            } else if a == OUT_DIR {
                out_dir.as_os_str().to_owned()
            } else {
                OsString::from(
                    a.replace(INPUT, &input.to_string_lossy())
                        .replace(OUT_DIR, &out_dir.to_string_lossy()),
                )
            }
        })
        .collect()
}

/// One measured run of a module on one file.
#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    /// Wall time from spawn to exit, milliseconds.
    pub wall_ms: f64,
    /// User plus system CPU, milliseconds (10 ms resolution on macOS).
    pub cpu_ms: f64,
    /// Peak resident set size, bytes.
    pub max_rss_bytes: u64,
    /// Peak physical footprint, bytes (macOS).
    pub peak_footprint_bytes: Option<u64>,
    /// Instructions retired (macOS).
    pub instructions: Option<u64>,
    /// Total size of the files the module wrote, bytes.
    pub output_bytes: u64,
    /// Time spent inside the registry call (worker runner only), milliseconds.
    pub inner_ms: Option<f64>,
    /// True when the module succeeded.
    pub ok: bool,
    /// What went wrong, when it did.
    pub error: Option<String>,
}

impl Sample {
    /// The memory figure budgets are judged on.
    pub fn peak_mem_bytes(&self) -> u64 {
        self.peak_footprint_bytes.unwrap_or(self.max_rss_bytes)
    }
}

/// Total size of regular files under `dir`, 0 when it does not exist.
pub fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

/// A scratch folder that is deleted when dropped.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Create `<tmp>/panoply-bench-<pid>-<n>` in the system temp folder.
    pub fn new() -> Result<Scratch> {
        Scratch::new_in(&std::env::temp_dir())
    }

    /// Create `<parent>/panoply-bench-<pid>-<n>`.
    pub fn new_in(parent: &Path) -> Result<Scratch> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("panoply-bench-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path)
            .map_err(|e| BenchError::io(format!("creating {}", path.display()), e))?;
        Ok(Scratch { path })
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Run `module` on `input` once and measure it.
pub fn sample(runner: &Runner, module: &str, input: &Path) -> Result<Sample> {
    let scratch = Scratch::new()?;
    let (program, args) = runner.command(module, input, scratch.path());
    let m = measure::run_measured(&program, &args)?;
    Ok(to_sample(runner, &m, scratch.path()))
}

fn to_sample(runner: &Runner, m: &Measured, out_dir: &Path) -> Sample {
    let mut ok = m.exit_ok;
    let mut error = (!m.exit_ok).then(|| first_line(&m.stderr).to_string());
    let mut inner_ms = None;
    if let Runner::Worker { .. } = runner {
        match serde_json::from_str::<WorkerReport>(m.stdout.trim()) {
            Ok(r) => {
                inner_ms = Some(r.inner_ms);
                ok = r.ok && m.exit_ok;
                if let Some(e) = r.error {
                    error = Some(e);
                }
            }
            Err(e) => {
                ok = false;
                error = Some(format!(
                    "worker printed no report ({e}): {}",
                    first_line(&m.stderr)
                ));
            }
        }
    }
    let TimeStats {
        max_rss_bytes,
        peak_footprint_bytes,
        instructions,
        ..
    } = m.stats;
    Sample {
        wall_ms: m.wall.as_secs_f64() * 1000.0,
        cpu_ms: m.stats.cpu_s() * 1000.0,
        max_rss_bytes,
        peak_footprint_bytes,
        instructions,
        output_bytes: dir_size(out_dir),
        inner_ms,
        ok,
        error: if ok {
            None
        } else {
            error.or(Some("failed".into()))
        },
    }
}

fn first_line(s: &str) -> &str {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// Run a baseline command (a native tool) on `input` once. `template` uses `{input}`.
pub fn sample_baseline(template: &str, input: &Path) -> Result<Sample> {
    let words: Vec<String> = template.split_whitespace().map(str::to_string).collect();
    let Some((prog, rest)) = words.split_first() else {
        return Err(BenchError::Usage("the baseline command is empty".into()));
    };
    let program = resolve_program(prog);
    let scratch = Scratch::new()?;
    let args = expand(rest, input, scratch.path());
    let m = measure::run_measured(&program, &args)?;
    let mut s = to_sample(
        &Runner::Cli {
            bin: program,
            args: vec![],
        },
        &m,
        scratch.path(),
    );
    s.inner_ms = None;
    Ok(s)
}

/// Find a program on `PATH` when it is not given as a path.
pub fn resolve_program(name: &str) -> PathBuf {
    let p = Path::new(name);
    if p.components().count() > 1 || p.is_absolute() {
        return p.to_path_buf();
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    p.to_path_buf()
}

/// Locate the `panoply` binary next to this one (the same cargo profile), if it is built.
pub fn default_cli_bin() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    let candidate = dir.join(if cfg!(windows) {
        "panoply.exe"
    } else {
        "panoply"
    });
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_expand() {
        let t = vec![
            "hash".to_string(),
            "{input}".to_string(),
            "--out={out_dir}/x".to_string(),
        ];
        let got = expand(&t, Path::new("/a/b.pdf"), Path::new("/tmp/o"));
        assert_eq!(
            got,
            vec![
                OsString::from("hash"),
                "/a/b.pdf".into(),
                "--out=/tmp/o/x".into()
            ]
        );
    }

    #[test]
    fn worker_command_shape() {
        let r = Runner::Worker {
            exe: PathBuf::from("/bin/pb"),
        };
        let (prog, args) = r.command("files.hash", Path::new("/x.bin"), Path::new("/o"));
        assert_eq!(prog, PathBuf::from("/bin/pb"));
        assert_eq!(args[0], OsString::from("worker"));
        assert_eq!(args.last().unwrap(), &OsString::from("/x.bin"));
    }

    #[test]
    fn dir_size_adds_nested_files() {
        let s = Scratch::new().unwrap();
        std::fs::create_dir_all(s.path().join("a/b")).unwrap();
        std::fs::write(s.path().join("one"), [0u8; 10]).unwrap();
        std::fs::write(s.path().join("a/b/two"), [0u8; 32]).unwrap();
        assert_eq!(dir_size(s.path()), 42);
        let p = s.path().to_path_buf();
        drop(s);
        assert!(!p.exists());
    }

    #[test]
    fn baseline_runs_a_native_tool() {
        if !measure::available() {
            return;
        }
        let s = Scratch::new().unwrap();
        let f = s.path().join("f.txt");
        std::fs::write(&f, b"hello\n").unwrap();
        let sample = sample_baseline("/bin/cat {input}", &f).unwrap();
        assert!(sample.ok, "{:?}", sample.error);
        assert!(sample.wall_ms > 0.0);
    }
}
