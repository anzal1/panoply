//! Plain text tables and the JSON report.
//!
//! The text output is for people reading a terminal or a CI log: no markdown, fixed columns.
//! The JSON output is the same data for tools, one document per command.

use std::io::Write;
use std::process::Command;

use serde::Serialize;

use crate::budget::{Basis, Check, Status};
use crate::idle::IdleReport;
use crate::run::{FileResult, RunReport};
use crate::scale::ScaleReport;
use crate::stats;

/// JSON schema version. Bump when a field changes meaning.
pub const SCHEMA: u32 = 1;

/// The machine the numbers came from.
#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    /// Operating system, for example `macos`.
    pub os: String,
    /// CPU architecture, for example `aarch64`.
    pub arch: String,
    /// Logical CPUs.
    pub cpus: usize,
    /// CPU model name when the platform says.
    pub cpu_brand: Option<String>,
    /// Installed memory in bytes when the platform says.
    pub memory_bytes: Option<u64>,
    /// `release` or `debug`: speed and CPU numbers only mean something in release.
    pub build_profile: &'static str,
}

impl HostInfo {
    /// Look at this machine.
    pub fn detect() -> HostInfo {
        let sysctl = |key: &str| -> Option<String> {
            let out = Command::new("sysctl").args(["-n", key]).output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let (cpu_brand, memory_bytes) = if cfg!(target_os = "macos") {
            (
                sysctl("machdep.cpu.brand_string"),
                sysctl("hw.memsize").and_then(|s| s.parse().ok()),
            )
        } else {
            (None, None)
        };
        HostInfo {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
            cpu_brand,
            memory_bytes,
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        }
    }
}

/// The JSON document every command writes.
#[derive(Debug, Clone, Serialize)]
pub struct FullReport {
    /// Schema version.
    pub schema: u32,
    /// `panoply-bench` and its version.
    pub tool: String,
    /// Seconds since the Unix epoch when the run started.
    pub started_unix: u64,
    /// The machine.
    pub host: HostInfo,
    /// The command that was run.
    pub command: String,
    /// Corpus run, when the command ran one.
    pub run: Option<RunReport>,
    /// Scale run, when the command ran one.
    pub scale: Option<ScaleReport>,
    /// Idle measurement, when the command made one.
    pub idle: Option<IdleReport>,
    /// One verdict per FLOWS.md budget row.
    pub budgets: Vec<Check>,
    /// False when any budget failed.
    pub passed: bool,
}

/// Write `value` as JSON to a file, or to stdout for `-`.
pub fn write_json(dest: &str, value: &impl Serialize) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).expect("report serializes");
    if dest == "-" {
        let mut out = std::io::stdout().lock();
        out.write_all(text.as_bytes())?;
        out.write_all(b"\n")
    } else {
        std::fs::write(dest, text + "\n")
    }
}

/// Lay out rows in left-aligned columns separated by two spaces. Columns listed in `right` are
/// right-aligned. Width is measured in characters.
pub fn table(header: &[&str], rows: &[Vec<String>], right: &[usize]) -> String {
    let cols = header.len();
    let mut width: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate().take(cols) {
            width[i] = width[i].max(c.chars().count());
        }
        debug_assert_eq!(r.len(), cols, "a table row must have one cell per header");
    }
    let line = |cells: Vec<&str>| -> String {
        let mut s = String::new();
        for (i, c) in cells.iter().enumerate() {
            let pad = width[i].saturating_sub(c.chars().count());
            if right.contains(&i) {
                s.push_str(&" ".repeat(pad));
                s.push_str(c);
            } else {
                s.push_str(c);
                if i + 1 < cols {
                    s.push_str(&" ".repeat(pad));
                }
            }
            if i + 1 < cols {
                s.push_str("  ");
            }
        }
        s
    };
    let mut out = String::new();
    out.push_str(&line(header.to_vec()));
    out.push('\n');
    out.push_str(&"-".repeat(width.iter().sum::<usize>() + 2 * (cols - 1)));
    out.push('\n');
    for r in rows {
        out.push_str(&line(r.iter().take(cols).map(String::as_str).collect()));
        out.push('\n');
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n - 1).collect();
        format!("{head}~")
    }
}

fn file_mem_mb(f: &FileResult) -> f64 {
    stats::mb(f.peak_mem_bytes)
}

/// The text report for a corpus run.
pub fn run_text(r: &RunReport, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Module {} over {} files, {} run(s) each, via {}\n",
        r.module,
        r.files.len(),
        r.runs,
        r.runner
    ));
    if r.skipped_missing > 0 {
        out.push_str(&format!(
            "{} matching corpus files are not on disk (run tests/corpus/fetch.py and generate.py)\n",
            r.skipped_missing
        ));
    }
    out.push('\n');

    if verbose || r.files.len() <= 30 {
        let rows: Vec<Vec<String>> = r
            .files
            .iter()
            .map(|f| {
                vec![
                    clip(&f.id, 44),
                    f.kind.clone(),
                    stats::human_bytes(f.input_bytes),
                    format!("{:.1}", f.wall_ms),
                    format!("{:.0}", f.cpu_ms),
                    f.minstr.map_or("-".to_string(), |m| format!("{m:.0}")),
                    format!("{:.1}", file_mem_mb(f)),
                    stats::human_bytes(f.output_bytes),
                    if f.ok {
                        "ok".to_string()
                    } else {
                        "error".to_string()
                    },
                ]
            })
            .collect();
        out.push_str(&table(
            &[
                "file", "kind", "input", "wall ms", "cpu ms", "Minstr", "peak MB", "output", "run",
            ],
            &rows,
            &[2, 3, 4, 5, 6, 7],
        ));
        out.push('\n');
    }

    // Per kind summary.
    let mut kinds: Vec<String> = r.files.iter().map(|f| f.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();
    let rows: Vec<Vec<String>> = kinds
        .iter()
        .map(|k| {
            let fs: Vec<&FileResult> = r.files.iter().filter(|f| &f.kind == k).collect();
            let walls: Vec<f64> = fs.iter().map(|f| f.wall_ms).collect();
            let peaks: Vec<f64> = fs.iter().map(|f| file_mem_mb(f)).collect();
            vec![
                k.clone(),
                fs.len().to_string(),
                stats::human_bytes(fs.iter().map(|f| f.input_bytes).sum()),
                format!("{:.1}", stats::median(&walls)),
                format!("{:.1}", stats::percentile(&walls, 95.0)),
                format!("{:.1}", stats::max(&walls)),
                format!("{:.1}", stats::median(&peaks)),
                format!("{:.1}", stats::max(&peaks)),
                fs.iter().filter(|f| !f.ok).count().to_string(),
            ]
        })
        .collect();
    out.push_str("Per kind\n");
    out.push_str(&table(
        &[
            "kind", "files", "input", "wall p50", "wall p95", "wall max", "MB p50", "MB max",
            "errors",
        ],
        &rows,
        &[1, 2, 3, 4, 5, 6, 7, 8],
    ));

    // Worst by memory and by wall time.
    let mut by_mem: Vec<&FileResult> = r.files.iter().collect();
    by_mem.sort_by_key(|f| std::cmp::Reverse(f.peak_mem_bytes));
    out.push_str("\nHighest peak memory\n");
    for f in by_mem.iter().take(5) {
        out.push_str(&format!(
            "  {:>8.1} MB  {}  ({} input)\n",
            file_mem_mb(f),
            f.id,
            stats::human_bytes(f.input_bytes)
        ));
    }
    let errors: Vec<&FileResult> = r.files.iter().filter(|f| !f.ok).collect();
    if !errors.is_empty() {
        out.push_str(&format!(
            "\nThe module reported an error on {} files (still measured, still held to the memory cap)\n",
            errors.len()
        ));
        for f in errors.iter().take(5) {
            out.push_str(&format!(
                "  {}: {}\n",
                f.id,
                f.error.clone().unwrap_or_default()
            ));
        }
    }
    let compared: Vec<&FileResult> = r.files.iter().filter(|f| f.baseline.is_some()).collect();
    if !compared.is_empty() {
        out.push_str(&format!(
            "\nSpeed against '{}' (files of {} or more)\n",
            r.baseline_cmd.clone().unwrap_or_default(),
            stats::human_bytes(compared.iter().map(|f| f.input_bytes).min().unwrap_or(0))
        ));
        let rows: Vec<Vec<String>> = compared
            .iter()
            .map(|f| {
                let b = f.baseline.as_ref().expect("filtered");
                vec![
                    clip(&f.id, 44),
                    stats::human_bytes(f.input_bytes),
                    format!("{:.1}", f.wall_ms),
                    format!("{:.1}", b.wall_ms),
                    format!("{:.2}", b.ratio),
                    format!("{:.1}", stats::mb(b.peak_mem_bytes)),
                ]
            })
            .collect();
        out.push_str(&table(
            &[
                "file",
                "input",
                "ours ms",
                "native ms",
                "ratio",
                "native MB",
            ],
            &rows,
            &[1, 2, 3, 4, 5],
        ));
    }
    out
}

/// The text report for a scale run.
pub fn scale_text(r: &ScaleReport) -> String {
    let rows: Vec<Vec<String>> = r
        .points
        .iter()
        .map(|p| {
            vec![
                stats::human_bytes(p.input_bytes),
                p.allocated_bytes
                    .map_or("?".to_string(), stats::human_bytes),
                format!("{:.0}", p.wall_ms),
                format!("{:.0}", p.throughput_mb_s),
                format!("{:.2}", stats::mb(p.peak_mem_bytes)),
                format!("{:.2}", stats::mb(p.max_rss_bytes)),
                if p.ok {
                    "ok".to_string()
                } else {
                    p.error.clone().unwrap_or("error".into())
                },
            ]
        })
        .collect();
    let mut out = format!(
        "Scale: module {} over sparse inputs, via {}\n\n",
        r.module, r.runner
    );
    out.push_str(&table(
        &[
            "input", "on disk", "wall ms", "MB/s", "peak MB", "rss MB", "run",
        ],
        &rows,
        &[0, 1, 2, 3, 4, 5],
    ));
    out.push_str(&format!(
        "\nPeak memory spread {:.2} MB (tolerance {:.0} MB), {:.5} bytes per input byte: {}\n",
        stats::mb(r.spread_bytes),
        stats::mb(r.tolerance_bytes),
        r.bytes_per_input_byte,
        if r.constant {
            "CONSTANT"
        } else {
            "NOT CONSTANT"
        }
    ));
    out
}

/// The text report for an idle measurement.
pub fn idle_text(r: &IdleReport) -> String {
    let mut out = format!("Idle: {}\n", r.target);
    out.push_str(&format!("  processes        {:?}\n", r.pids));
    out.push_str(&format!(
        "  settle / window  {:.1} s / {:.1} s\n",
        r.settle_s, r.window_s
    ));
    out.push_str(&format!(
        "  CPU              {:.3} s used, {:.2}% of one core  ({})\n",
        r.cpu_seconds,
        r.cpu_pct,
        if r.cpu_idle { "idle" } else { "NOT idle" }
    ));
    out.push_str(&format!(
        "  peak resident    {:.1} MB\n",
        stats::mb(r.peak_rss_bytes)
    ));
    if let Some(f) = r.footprint_bytes {
        out.push_str(&format!("  footprint        {:.1} MB\n", stats::mb(f)));
    }
    if let Some(ms) = r.start_to_ready_ms {
        out.push_str(&format!(
            "  start to ready   {ms:.1} ms (registry build {:.2} ms)\n",
            r.registry_ms.unwrap_or(0.0)
        ));
    }
    out
}

/// The budget table, then one line of detail per measured row.
pub fn budgets_text(checks: &[Check]) -> String {
    let rows: Vec<Vec<String>> = checks
        .iter()
        .map(|c| {
            vec![
                c.budget.to_string(),
                c.status.label().to_string(),
                match c.basis {
                    Basis::Direct => "direct",
                    Basis::Proxy => "proxy",
                    Basis::None => "-",
                }
                .to_string(),
                c.limit.clone(),
            ]
        })
        .collect();
    let mut out = String::from("Budgets (FLOWS.md, \"Budgets every module must meet\")\n");
    out.push_str(&table(&["budget", "status", "basis", "limit"], &rows, &[]));
    out.push_str("\nWhat was measured\n");
    for c in checks {
        match c.status {
            Status::NotMeasured => {
                out.push_str(&format!("  {:<20} not measured: {}\n", c.budget, c.detail));
            }
            _ => {
                out.push_str(&format!("  {:<20} {}\n", c.budget, c.measured));
                out.push_str(&format!("  {:<20} {}\n", "", c.detail));
            }
        }
    }
    let failed = checks.iter().filter(|c| c.status == Status::Fail).count();
    let passed = checks.iter().filter(|c| c.status == Status::Pass).count();
    let skipped = checks
        .iter()
        .filter(|c| c.status == Status::NotMeasured)
        .count();
    out.push_str(&format!(
        "\n{passed} passed, {failed} failed, {skipped} not measured\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_aligns_columns() {
        let t = table(
            &["name", "n"],
            &[
                vec!["alpha".into(), "5".into()],
                vec!["b".into(), "1234".into()],
            ],
            &[1],
        );
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "name      n");
        assert_eq!(lines[2], "alpha     5");
        assert_eq!(lines[3], "b      1234");
        assert!(lines[1].chars().all(|c| c == '-'));
    }

    #[test]
    fn clip_marks_truncation() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghij", 5), "abcd~");
    }

    #[test]
    fn json_round_trips_through_serde_value() {
        let report = FullReport {
            schema: SCHEMA,
            tool: "panoply-bench test".into(),
            started_unix: 1,
            host: HostInfo::detect(),
            command: "run".into(),
            run: None,
            scale: None,
            idle: None,
            budgets: crate::budget::merge(vec![]),
            passed: true,
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["budgets"].as_array().unwrap().len(), 10);
        assert_eq!(v["budgets"][3]["status"], "not_measured");
        assert!(v["host"]["cpus"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn budget_text_names_every_row() {
        let text = budgets_text(&crate::budget::merge(vec![]));
        for row in crate::budget::rows() {
            assert!(text.contains(row.0), "{}", row.0);
        }
        assert!(text.contains("0 passed, 0 failed, 10 not measured"));
    }
}
