//! Measure one child process: wall time, CPU time and peak memory.
//!
//! The command runs under `/usr/bin/time`, which asks the kernel for the child's resource usage
//! when it is reaped. That keeps this crate free of `unsafe` and of platform bindings, and the
//! numbers are the high-water marks of exactly one process, not samples.
//!
//! - macOS: `time -l` gives `maximum resident set size` in bytes and `peak memory footprint`,
//!   which is `phys_footprint`, the figure Activity Monitor calls Memory. It also gives
//!   `instructions retired` and `cycles elapsed`, a CPU measure that is finer than the 10 ms
//!   resolution of the user and system times.
//! - Linux: GNU `time -v` gives `Maximum resident set size` in kilobytes. There is no footprint.
//! - Windows: not supported (`time` does not exist). The harness says so instead of guessing.
//!
//! Wall time is taken by the harness around spawn and wait, so it includes process start and the
//! `time` wrapper (about a millisecond). That is the honest cost a user pays.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{BenchError, Result};

/// Where the measuring tool lives.
const TIME_BIN: &str = "/usr/bin/time";

/// What the kernel reported about the child.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeStats {
    /// Elapsed seconds as `time` saw it. Coarse (10 ms on macOS), use the harness wall time.
    pub real_s: f64,
    /// User CPU seconds.
    pub user_s: f64,
    /// System CPU seconds.
    pub sys_s: f64,
    /// Peak resident set size in bytes.
    pub max_rss_bytes: u64,
    /// Peak physical memory footprint in bytes (macOS only).
    pub peak_footprint_bytes: Option<u64>,
    /// Instructions retired (macOS only).
    pub instructions: Option<u64>,
    /// CPU cycles elapsed (macOS only).
    pub cycles: Option<u64>,
}

impl TimeStats {
    /// Total CPU seconds, user plus system.
    pub fn cpu_s(&self) -> f64 {
        self.user_s + self.sys_s
    }

    /// The memory figure the budgets are judged on: peak footprint where the platform has it,
    /// else peak resident set size.
    pub fn peak_mem_bytes(&self) -> u64 {
        self.peak_footprint_bytes.unwrap_or(self.max_rss_bytes)
    }
}

/// The outcome of one measured run.
#[derive(Debug, Clone)]
pub struct Measured {
    /// True when the child exited with status 0.
    pub exit_ok: bool,
    /// Exit code when there was one.
    pub exit_code: Option<i32>,
    /// What the child wrote to stdout.
    pub stdout: String,
    /// What the child wrote to stderr, without the `time` report.
    pub stderr: String,
    /// Wall time from spawn to exit.
    pub wall: Duration,
    /// Resource usage of the child.
    pub stats: TimeStats,
}

/// True when [`run_measured`] can work on this machine.
pub fn available() -> bool {
    Path::new(TIME_BIN).is_file() && !cfg!(windows)
}

/// The flag that makes `time` print resource usage on this platform.
fn time_flag() -> &'static str {
    if cfg!(target_os = "macos") {
        "-l"
    } else {
        "-v"
    }
}

/// Run `program args` under `time` with stdin closed and return what happened.
pub fn run_measured(program: &Path, args: &[OsString]) -> Result<Measured> {
    if !available() {
        return Err(BenchError::Unsupported(format!(
            "{TIME_BIN} is not available here, so the harness cannot measure child processes. \
             Run it on macOS or Linux."
        )));
    }
    let started = Instant::now();
    let out = Command::new(TIME_BIN)
        .arg(time_flag())
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| BenchError::io(format!("starting {}", program.display()), e))?;
    let wall = started.elapsed();
    let raw_err = String::from_utf8_lossy(&out.stderr).into_owned();
    let (stats, child_err) = parse_time_output(&raw_err).ok_or_else(|| {
        BenchError::Run(format!(
            "could not read the resource report from {TIME_BIN}; stderr was: {}",
            raw_err.trim()
        ))
    })?;
    Ok(Measured {
        exit_ok: out.status.success(),
        exit_code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: child_err,
        wall,
        stats,
    })
}

/// Split `time`'s report off the end of stderr. Returns the stats and the child's own stderr.
pub fn parse_time_output(stderr: &str) -> Option<(TimeStats, String)> {
    parse_bsd(stderr).or_else(|| parse_gnu(stderr))
}

fn number(token: &str) -> Option<f64> {
    token.parse::<f64>().ok()
}

/// macOS and BSD `time -l`: a `real user sys` line, then `value  label` lines.
fn parse_bsd(stderr: &str) -> Option<(TimeStats, String)> {
    let lines: Vec<&str> = stderr.lines().collect();
    let start = lines.iter().position(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        t.len() == 6
            && t[1] == "real"
            && t[3] == "user"
            && t[5] == "sys"
            && number(t[0]).is_some()
            && number(t[2]).is_some()
            && number(t[4]).is_some()
    })?;
    let t: Vec<&str> = lines[start].split_whitespace().collect();
    let mut s = TimeStats {
        real_s: number(t[0])?,
        user_s: number(t[2])?,
        sys_s: number(t[4])?,
        ..TimeStats::default()
    };
    for l in &lines[start + 1..] {
        let l = l.trim();
        let (value, label) = match l.split_once(char::is_whitespace) {
            Some(p) => p,
            None => continue,
        };
        let Ok(v) = value.parse::<u64>() else {
            continue;
        };
        match label.trim() {
            "maximum resident set size" => s.max_rss_bytes = v,
            "peak memory footprint" => s.peak_footprint_bytes = Some(v),
            "instructions retired" => s.instructions = Some(v),
            "cycles elapsed" => s.cycles = Some(v),
            _ => {}
        }
    }
    let child_err = lines[..start].join("\n");
    Some((s, child_err))
}

/// GNU `time -v`: a tab-indented block that starts with `Command being timed:`.
fn parse_gnu(stderr: &str) -> Option<(TimeStats, String)> {
    let lines: Vec<&str> = stderr.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with("Command being timed:"))?;
    let mut s = TimeStats::default();
    for l in &lines[start..] {
        let Some((key, value)) = l.trim().rsplit_once(": ") else {
            continue;
        };
        let value = value.trim();
        if key.starts_with("User time") {
            s.user_s = number(value).unwrap_or(0.0);
        } else if key.starts_with("System time") {
            s.sys_s = number(value).unwrap_or(0.0);
        } else if key.starts_with("Elapsed (wall clock)") {
            s.real_s = parse_clock(value).unwrap_or(0.0);
        } else if key.starts_with("Maximum resident set size") {
            s.max_rss_bytes = value.parse::<u64>().unwrap_or(0) * 1024;
        }
    }
    Some((s, lines[..start].join("\n")))
}

/// `h:mm:ss`, `m:ss.hh` or plain seconds.
fn parse_clock(text: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in text.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total)
}

/// Where this binary lives, for starting its own `worker` subcommand.
pub fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().map_err(|e| BenchError::io("finding the harness binary", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: &str = "child said this\nand this\n        0.23 real         0.20 user         0.01 sys\n\
             3407872  maximum resident set size\n                   0  average shared memory size\n\
              11423216  instructions retired\n             5279368  cycles elapsed\n\
             2294336  peak memory footprint\n";

    #[test]
    fn reads_the_macos_report() {
        let (s, err) = parse_time_output(MAC).unwrap();
        assert_eq!(s.real_s, 0.23);
        assert_eq!(s.user_s, 0.20);
        assert_eq!(s.sys_s, 0.01);
        assert_eq!(s.max_rss_bytes, 3_407_872);
        assert_eq!(s.peak_footprint_bytes, Some(2_294_336));
        assert_eq!(s.instructions, Some(11_423_216));
        assert_eq!(s.cycles, Some(5_279_368));
        assert_eq!(err, "child said this\nand this");
        assert_eq!(s.peak_mem_bytes(), 2_294_336);
        assert!((s.cpu_s() - 0.21).abs() < 1e-9);
    }

    #[test]
    fn reads_the_gnu_report() {
        let text = "oops\n\tCommand being timed: \"sha256sum x\"\n\tUser time (seconds): 0.40\n\tSystem time (seconds): 0.05\n\
                    \tPercent of CPU this job got: 99%\n\tElapsed (wall clock) time (h:mm:ss or m:ss): 0:01.50\n\
                    \tMaximum resident set size (kbytes): 2048\n";
        let (s, err) = parse_time_output(text).unwrap();
        assert_eq!(s.user_s, 0.40);
        assert_eq!(s.real_s, 1.5);
        assert_eq!(s.max_rss_bytes, 2048 * 1024);
        assert_eq!(s.peak_footprint_bytes, None);
        assert_eq!(s.peak_mem_bytes(), 2_097_152);
        assert_eq!(err, "oops");
    }

    #[test]
    fn rejects_text_without_a_report() {
        assert!(parse_time_output("just an error\n").is_none());
    }

    #[test]
    fn clock_formats() {
        assert_eq!(parse_clock("1:02:03.5"), Some(3723.5));
        assert_eq!(parse_clock("0:00.25"), Some(0.25));
        assert_eq!(parse_clock("7"), Some(7.0));
        assert_eq!(parse_clock("x"), None);
    }

    #[test]
    fn measures_a_real_child() {
        if !available() {
            return;
        }
        let m = run_measured(Path::new("/bin/echo"), &[OsString::from("hello")]).unwrap();
        assert!(m.exit_ok);
        assert_eq!(m.stdout.trim(), "hello");
        assert!(m.stats.max_rss_bytes > 0);
        assert!(m.wall > Duration::ZERO);
    }
}
