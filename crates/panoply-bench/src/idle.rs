//! Idle cost: what a running process holds and burns while no job runs.
//!
//! FLOWS.md: "0% CPU and no memory held when no job is running", and the SwiftUI shell kill test
//! measured 30 MB idle (ARCHITECTURE.md section 7). This module samples a process for a window
//! of seconds and reports CPU used and memory held.
//!
//! What it can sample:
//!
//! - the host itself, with nothing running: `panoply-bench worker-idle` builds the registry and
//!   waits (the default target, and the closest thing to a long-running CLI mode until the CLI
//!   or MCP server has one),
//! - any command you give it (`idle --cmd panoply mcp`), started with stdin held open,
//! - any running process or app shell by pid (`idle --pid 4242 --pid 4243`), the way
//!   `prototypes/shell-test/bench.py` measures the SwiftUI, Tauri and Flutter shells. Helper
//!   processes (WebKit under Tauri) each need their own `--pid`.
//!
//! CPU comes from `ps -o cputime`, which has 10 ms resolution, so a 10 s window resolves 0.1%.
//! "0%" is judged as at most [`IDLE_CPU_MAX_PCT`]. Memory is resident set size from `ps`, plus
//! `phys_footprint` from `footprint(1)` on macOS. Windows has no `ps`; there the same numbers
//! (processor time and working set) come from `Get-Process` in PowerShell.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{BenchError, Result};

/// Highest CPU use that still counts as idle, in percent of one core over the window.
/// Two scheduler ticks (20 ms) in a 10 s window.
pub const IDLE_CPU_MAX_PCT: f64 = 0.2;

/// What to sample.
#[derive(Debug, Clone)]
pub enum Target {
    /// Start `panoply-bench worker-idle` (the host with nothing running).
    HostWorker {
        /// Path of the harness binary.
        exe: PathBuf,
    },
    /// Start this command and sample it.
    Command {
        /// Program and arguments.
        argv: Vec<String>,
    },
    /// Sample processes that are already running.
    Pids(Vec<u32>),
}

/// Sampling parameters.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    /// Wait this long after start before the first sample, so startup work is not counted.
    pub settle: Duration,
    /// Length of the measured window.
    pub window: Duration,
    /// Time between memory samples.
    pub interval: Duration,
}

/// One memory sample of the sampled processes together.
#[derive(Debug, Clone, Serialize)]
pub struct MemSample {
    /// Seconds since the window began.
    pub at_s: f64,
    /// Resident set size summed over the processes, bytes.
    pub rss_bytes: u64,
}

/// The idle measurement.
#[derive(Debug, Clone, Serialize)]
pub struct IdleReport {
    /// What was sampled, for the report.
    pub target: String,
    /// Process ids sampled.
    pub pids: Vec<u32>,
    /// Settle time in seconds.
    pub settle_s: f64,
    /// Window length in seconds.
    pub window_s: f64,
    /// CPU seconds used by all sampled processes during the window.
    pub cpu_seconds: f64,
    /// CPU use as a percentage of one core over the window.
    pub cpu_pct: f64,
    /// Peak resident set size over the window, bytes.
    pub peak_rss_bytes: u64,
    /// Physical footprint at the end of the window, bytes (macOS only).
    pub footprint_bytes: Option<u64>,
    /// For the host worker: milliseconds from spawn to `ready` (process start plus registry).
    pub start_to_ready_ms: Option<f64>,
    /// Milliseconds the worker spent building the registry itself.
    pub registry_ms: Option<f64>,
    /// Memory samples.
    pub samples: Vec<MemSample>,
    /// True when CPU is within [`IDLE_CPU_MAX_PCT`].
    pub cpu_idle: bool,
}

/// Parse `ps` cputime: `0:00.02`, `1:02.50`, `1:02:03.00` or `2-01:00:00`.
pub fn parse_cputime(text: &str) -> Option<f64> {
    let t = text.trim();
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) => (d.parse::<f64>().ok()?, r),
        None => (0.0, t),
    };
    let mut total = 0.0;
    for part in rest.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(days * 86_400.0 + total)
}

/// Parse the footprint line of `footprint -p`: `Footprint: 912 KB`, `12 MB`, `1.2 GB`.
pub fn parse_footprint(output: &str) -> Option<u64> {
    for line in output.lines() {
        let Some(after) = line.split("Footprint:").nth(1) else {
            continue;
        };
        let mut it = after.split_whitespace();
        let value: f64 = it.next()?.replace(',', "").parse().ok()?;
        let mult = match it.next()?.to_ascii_uppercase().as_str() {
            "B" => 1.0,
            "KB" => 1024.0,
            "MB" => 1024.0 * 1024.0,
            "GB" => 1024.0 * 1024.0 * 1024.0,
            _ => return None,
        };
        return Some((value * mult) as u64);
    }
    None
}

/// CPU seconds and resident bytes for one process, via `ps`. `None` when it has exited.
#[cfg(not(windows))]
fn ps(pid: u32) -> Option<(f64, u64)> {
    let out = Command::new("ps")
        .args(["-o", "cputime=,rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split_whitespace();
    let cpu = parse_cputime(it.next()?)?;
    let rss_kb: u64 = it.next()?.parse().ok()?;
    Some((cpu, rss_kb * 1024))
}

/// CPU seconds and working set bytes for one process. Windows has no `ps`, so this asks
/// PowerShell for the kernel's own counters (`TotalProcessorTime` has 100 ns resolution,
/// `WorkingSet64` is the resident set). `None` when the process has exited.
#[cfg(windows)]
fn ps(pid: u32) -> Option<(f64, u64)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "$p = Get-Process -Id {pid} -ErrorAction SilentlyContinue; \
         if ($p) {{ '{{0}} {{1}}' -f $p.TotalProcessorTime.Ticks, $p.WorkingSet64 }}"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    parse_windows_sample(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `<cpu ticks of 100 ns> <working set bytes>`, the line the Windows sampler prints.
#[cfg(any(windows, test))]
fn parse_windows_sample(text: &str) -> Option<(f64, u64)> {
    let mut it = text.split_whitespace();
    let ticks: u64 = it.next()?.parse().ok()?;
    let bytes: u64 = it.next()?.parse().ok()?;
    Some((ticks as f64 / 1e7, bytes))
}

fn footprint(pid: u32) -> Option<u64> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = Command::new("footprint")
        .args(["-p", &pid.to_string()])
        .output()
        .ok()?;
    parse_footprint(&String::from_utf8_lossy(&out.stdout))
}

/// Measure idle cost of `target` over `window`.
pub fn measure(target: &Target, window: Window) -> Result<IdleReport> {
    let mut child = None;
    let mut keep_stdin = None;
    let mut start_to_ready_ms = None;
    let mut registry_ms = None;
    let (label, pids): (String, Vec<u32>) = match target {
        Target::Pids(p) => (format!("pids {p:?}"), p.clone()),
        Target::HostWorker { exe } => {
            let started = Instant::now();
            let mut c = Command::new(exe)
                .arg("worker-idle")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| BenchError::io(format!("starting {}", exe.display()), e))?;
            let stdout = c.stdout.take().expect("piped stdout");
            let mut line = String::new();
            BufReader::new(stdout)
                .read_line(&mut line)
                .map_err(|e| BenchError::io("waiting for the host worker", e))?;
            start_to_ready_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
            registry_ms = crate::worker::parse_ready_line(&line);
            if registry_ms.is_none() {
                return Err(BenchError::Run(format!(
                    "host worker said {:?}, expected a ready line",
                    line.trim()
                )));
            }
            let pid = c.id();
            keep_stdin = c.stdin.take();
            child = Some(c);
            (
                "host worker (registry loaded, no job running)".to_string(),
                vec![pid],
            )
        }
        Target::Command { argv } => {
            let Some((prog, args)) = argv.split_first() else {
                return Err(BenchError::Usage("--cmd needs a program".into()));
            };
            let mut c = Command::new(crate::runner::resolve_program(prog))
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| BenchError::io(format!("starting {prog}"), e))?;
            let pid = c.id();
            keep_stdin = c.stdin.take();
            child = Some(c);
            (format!("command {}", argv.join(" ")), vec![pid])
        }
    };
    if pids.is_empty() {
        return Err(BenchError::Usage("no process to sample".into()));
    }

    std::thread::sleep(window.settle);
    let sum = |f: &dyn Fn(u32) -> Option<(f64, u64)>| -> Option<(f64, u64)> {
        let mut cpu = 0.0;
        let mut rss = 0;
        for &p in &pids {
            let (c, r) = f(p)?;
            cpu += c;
            rss += r;
        }
        Some((cpu, rss))
    };
    let (cpu0, rss0) = sum(&ps).ok_or_else(|| {
        BenchError::Run("the process is not running (it exited during settle)".into())
    })?;
    let began = Instant::now();
    let mut samples = vec![MemSample {
        at_s: 0.0,
        rss_bytes: rss0,
    }];
    let mut peak = rss0;
    while began.elapsed() < window.window {
        let left = window.window.saturating_sub(began.elapsed());
        std::thread::sleep(window.interval.min(left));
        if let Some((_, rss)) = sum(&ps) {
            peak = peak.max(rss);
            samples.push(MemSample {
                at_s: began.elapsed().as_secs_f64(),
                rss_bytes: rss,
            });
        }
    }
    let elapsed = began.elapsed().as_secs_f64();
    let (cpu1, _) =
        sum(&ps).ok_or_else(|| BenchError::Run("the process exited during the window".into()))?;
    let footprint_bytes = {
        let mut total = 0;
        let mut all = true;
        for &p in &pids {
            match footprint(p) {
                Some(f) => total += f,
                None => all = false,
            }
        }
        all.then_some(total)
    };

    // Stop what we started: closing stdin ends both child kinds.
    drop(keep_stdin);
    if let Some(mut c) = child {
        let deadline = Instant::now() + Duration::from_secs(2);
        while c.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if c.try_wait().ok().flatten().is_none() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    let cpu_seconds = (cpu1 - cpu0).max(0.0);
    let cpu_pct = if elapsed > 0.0 {
        cpu_seconds / elapsed * 100.0
    } else {
        0.0
    };
    Ok(IdleReport {
        target: label,
        pids,
        settle_s: window.settle.as_secs_f64(),
        window_s: elapsed,
        cpu_seconds,
        cpu_pct,
        peak_rss_bytes: peak,
        footprint_bytes,
        start_to_ready_ms,
        registry_ms,
        samples,
        cpu_idle: cpu_pct <= IDLE_CPU_MAX_PCT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cputime_formats() {
        assert_eq!(parse_cputime("0:00.02"), Some(0.02));
        assert_eq!(parse_cputime("  1:02.50"), Some(62.5));
        assert_eq!(parse_cputime("1:02:03.00"), Some(3723.0));
        assert_eq!(parse_cputime("2-01:00:00"), Some(2.0 * 86_400.0 + 3600.0));
        assert_eq!(parse_cputime("abc"), None);
    }

    #[test]
    fn footprint_lines() {
        let out = "======\nsleep [37240]: 64-bit    Footprint: 912 KB (16384 bytes per page)\n";
        assert_eq!(parse_footprint(out), Some(912 * 1024));
        assert_eq!(
            parse_footprint("x [1]: 64-bit    Footprint: 1.5 GB (16384 bytes per page)"),
            Some((1.5 * 1073741824.0) as u64)
        );
        assert_eq!(parse_footprint("no such line"), None);
    }

    #[test]
    fn windows_sample_lines() {
        assert_eq!(
            parse_windows_sample("1562500 12345678\r\n"),
            Some((0.15625, 12_345_678))
        );
        assert_eq!(parse_windows_sample(""), None);
        assert_eq!(parse_windows_sample("1 x"), None);
    }

    #[test]
    fn a_sleeping_process_is_idle() {
        let mut c = if cfg!(windows) {
            // ping.exe waits on a timer and costs nothing in between, unlike a PowerShell that
            // is still loading its runtime while the window opens. It runs far longer than the
            // measurement (which can take most of a minute on a loaded runner) and is killed after.
            Command::new("ping")
                .args(["-n", "600", "127.0.0.1"])
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            Command::new("sleep").arg("600").spawn().unwrap()
        };
        let r = measure(
            &Target::Pids(vec![c.id()]),
            Window {
                settle: Duration::from_millis(100),
                window: Duration::from_millis(600),
                interval: Duration::from_millis(200),
            },
        )
        .unwrap();
        let _ = c.kill();
        let _ = c.wait();
        assert!(r.cpu_pct <= 2.0, "cpu {}", r.cpu_pct);
        assert!(r.peak_rss_bytes > 0);
        assert!(r.samples.len() >= 2);
    }
}
