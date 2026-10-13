//! The FLOWS.md budget table as data, and the verdict for each row.
//!
//! "Budgets every module must meet" has ten rows. This harness measures some directly, some by a
//! proxy it labels as one, and says "not measured" for the rest and why. A row is never passed
//! because nothing was checked.

use panoply_core::budgets::{
    FIRST_PREVIEW_MS, IDLE_APP_MB, MODEL_IDLE_UNLOAD_S, MODULE_START_MS, PEAK_MEM_DOC_MB,
    UI_TASK_MS,
};
use serde::Serialize;

use crate::idle::{IDLE_CPU_MAX_PCT, IdleReport};
use crate::run::RunReport;
use crate::scale::ScaleReport;
use crate::stats;

/// A file this large or larger counts as a "huge file" for the Huge files row, in bytes.
pub const HUGE_BYTES: u64 = 1_000_000_000;
/// The scale run must reach this size to speak for the 2 GB PDF claim, in bytes.
pub const HUGE_SCALE_BYTES: u64 = 2_000_000_000;

/// Verdict for one budget row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Measured and within the limit.
    Pass,
    /// Measured and over the limit.
    Fail,
    /// Not measured by this run.
    NotMeasured,
}

impl Status {
    /// Fixed-width label for the table.
    pub fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::NotMeasured => "not measured",
        }
    }
}

/// How direct the measurement is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The thing the budget names.
    Direct,
    /// A stand-in, named in the detail (for example process start for module start).
    Proxy,
    /// Nothing measured.
    None,
}

/// One row of the budget table with its verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// FLOWS.md row name.
    pub budget: &'static str,
    /// The limit in words, built from `panoply_core::budgets`.
    pub limit: String,
    /// What was measured.
    pub measured: String,
    /// Verdict.
    pub status: Status,
    /// Direct, proxy or none.
    pub basis: Basis,
    /// What the number covers and what it leaves out.
    pub detail: String,
}

/// The ten FLOWS.md rows, in document order, with the limit text and the reason a row is
/// reported as not measured when no run covered it.
pub fn rows() -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "Idle cost",
            format!("0% CPU, no memory held (under {IDLE_APP_MB} MB)"),
            "run `panoply-bench idle` (host) or `idle --pid` (an app shell)",
        ),
        (
            "Module start",
            format!("under {MODULE_START_MS} ms click to interactive"),
            "run `panoply-bench idle`; in a shell it needs the shell's own launch timing (see prototypes/shell-test)",
        ),
        (
            "First preview",
            format!("under {FIRST_PREVIEW_MS} ms for instant and tuned transforms"),
            "pass --instant on a module that is an instant transform",
        ),
        (
            "UI thread",
            format!("no task over {UI_TASK_MS} ms on the UI thread"),
            "needs an app shell with a task timer; the harness drives processes, not windows",
        ),
        (
            "Frame rate",
            "120 fps, no dropped frames".to_string(),
            "needs a rendered window and a frame counter in the shell",
        ),
        (
            "Peak memory per job",
            format!(
                "under {PEAK_MEM_DOC_MB} MB for documents and images, constant for video and audio"
            ),
            "run `panoply-bench run` over a corpus slice, and `scale` for the constant claim",
        ),
        (
            "Huge files",
            "2 GB PDF or 20 GB video within the same cap".to_string(),
            "run `panoply-bench scale --sizes 2GiB` or include an input of 1 GB or more",
        ),
        (
            "Models",
            format!("load on first use, unload after {MODEL_IDLE_UNLOAD_S} s idle"),
            "no model-backed module exists yet; needs load and unload hooks to watch",
        ),
        (
            "Speed",
            "at least as fast as the native CLI tool".to_string(),
            "pass --baseline with the native command, for example 'shasum -a 256 {input}'",
        ),
        (
            "Output quality",
            "equal or better than the named competitor".to_string(),
            "judged per module on the quality corpora (SSIMULACRA2, boundary F1 and so on), not here",
        ),
    ]
}

fn check(
    budget: &'static str,
    status: Status,
    basis: Basis,
    measured: String,
    detail: String,
) -> Check {
    let limit = rows()
        .into_iter()
        .find(|(n, _, _)| *n == budget)
        .map(|(_, l, _)| l)
        .unwrap_or_default();
    Check {
        budget,
        limit,
        measured,
        status,
        basis,
        detail,
    }
}

fn cap_bytes() -> u64 {
    PEAK_MEM_DOC_MB * 1_000_000
}

/// Verdicts from a corpus run.
pub fn from_run(r: &RunReport) -> Vec<Check> {
    let mut out = Vec::new();
    if r.files.is_empty() {
        return out;
    }
    let worst = r
        .files
        .iter()
        .max_by_key(|f| f.peak_mem_bytes)
        .expect("non-empty");
    let over: Vec<_> = r
        .files
        .iter()
        .filter(|f| f.peak_mem_bytes > cap_bytes())
        .collect();
    out.push(check(
        "Peak memory per job",
        if over.is_empty() {
            Status::Pass
        } else {
            Status::Fail
        },
        Basis::Direct,
        format!(
            "max {:.1} MB over {} files (worst {}, {} input)",
            stats::mb(worst.peak_mem_bytes),
            r.files.len(),
            worst.id,
            stats::human_bytes(worst.input_bytes)
        ),
        if over.is_empty() {
            "peak physical footprint of the child process, highest of the runs".to_string()
        } else {
            format!(
                "over the cap: {}",
                over.iter()
                    .map(|f| f.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    ));

    let huge: Vec<_> = r
        .files
        .iter()
        .filter(|f| f.input_bytes >= HUGE_BYTES)
        .collect();
    if let Some(h) = huge.iter().max_by_key(|f| f.peak_mem_bytes) {
        let big = huge
            .iter()
            .max_by_key(|f| f.input_bytes)
            .expect("non-empty");
        let bad = huge.iter().any(|f| f.peak_mem_bytes > cap_bytes());
        out.push(check(
            "Huge files",
            if bad { Status::Fail } else { Status::Pass },
            Basis::Direct,
            format!(
                "max {:.1} MB over {} inputs of {} or more (largest {})",
                stats::mb(h.peak_mem_bytes),
                huge.len(),
                stats::human_bytes(HUGE_BYTES),
                stats::human_bytes(big.input_bytes)
            ),
            "real files from the corpus huge tier".to_string(),
        ));
    }

    if r.instant {
        let typical: Vec<_> = r
            .files
            .iter()
            .filter(|f| f.input_bytes <= r.typical_bytes)
            .collect();
        if !typical.is_empty() {
            let walls: Vec<f64> = typical.iter().map(|f| f.wall_ms).collect();
            let worst_wall = typical
                .iter()
                .max_by(|a, b| a.wall_ms.total_cmp(&b.wall_ms))
                .expect("non-empty");
            out.push(check(
                "First preview",
                if worst_wall.wall_ms <= FIRST_PREVIEW_MS as f64 { Status::Pass } else { Status::Fail },
                Basis::Proxy,
                format!(
                    "slowest {:.0} ms, median {:.0} ms over {} files up to {}",
                    worst_wall.wall_ms,
                    stats::median(&walls),
                    typical.len(),
                    stats::human_bytes(r.typical_bytes)
                ),
                format!(
                    "whole child process wall time (spawn, registry, job, exit), worst file {}. There is no preview API yet, so this is the job's own latency",
                    worst_wall.id
                ),
            ));
        }
    }

    let compared: Vec<_> = r
        .files
        .iter()
        .filter_map(|f| f.baseline.as_ref().map(|b| (f, b)))
        .collect();
    if !compared.is_empty() {
        let worst = compared
            .iter()
            .max_by(|a, b| a.1.ratio.total_cmp(&b.1.ratio))
            .expect("non-empty");
        let ratios: Vec<f64> = compared.iter().map(|(_, b)| b.ratio).collect();
        out.push(check(
            "Speed",
            if worst.1.ratio <= r.speed_tolerance {
                Status::Pass
            } else {
                Status::Fail
            },
            Basis::Direct,
            format!(
                "ours/native median {:.2}, worst {:.2} ({}) over {} files",
                stats::median(&ratios),
                worst.1.ratio,
                worst.0.id,
                compared.len()
            ),
            format!(
                "wall time, medians of {} runs, against '{}' with a {:.0}% noise margin",
                r.runs,
                r.baseline_cmd.clone().unwrap_or_default(),
                (r.speed_tolerance - 1.0) * 100.0
            ),
        ));
    }
    out
}

/// Verdicts from a scale run.
pub fn from_scale(r: &ScaleReport) -> Vec<Check> {
    let mut out = Vec::new();
    let Some(last) = r.points.last() else {
        return out;
    };
    let peak = r.points.iter().map(|p| p.peak_mem_bytes).max().unwrap_or(0);
    let flat = r.constant;
    let under = peak <= cap_bytes();
    out.push(check(
        "Peak memory per job",
        if flat && under {
            Status::Pass
        } else {
            Status::Fail
        },
        Basis::Direct,
        format!(
            "{} from {} to {}: spread {:.1} MB, {:.4} bytes per input byte",
            if flat { "constant" } else { "NOT constant" },
            stats::human_bytes(r.points[0].input_bytes),
            stats::human_bytes(last.input_bytes),
            stats::mb(r.spread_bytes),
            r.bytes_per_input_byte
        ),
        format!(
            "peak {:.1} MB, tolerance {:.0} MB on the spread; sparse inputs read as zeros",
            stats::mb(peak),
            stats::mb(r.tolerance_bytes)
        ),
    ));
    if last.input_bytes >= HUGE_SCALE_BYTES {
        out.push(check(
            "Huge files",
            if last.ok && last.peak_mem_bytes <= cap_bytes() {
                Status::Pass
            } else {
                Status::Fail
            },
            Basis::Direct,
            format!(
                "{} input: peak {:.1} MB in {:.1} s ({:.0} MB/s)",
                stats::human_bytes(last.input_bytes),
                stats::mb(last.peak_mem_bytes),
                last.wall_ms / 1000.0,
                last.throughput_mb_s
            ),
            "sparse file made on the fly and deleted after the run".to_string(),
        ));
    }
    out
}

/// Verdicts from an idle measurement.
pub fn from_idle(r: &IdleReport) -> Vec<Check> {
    let mut out = Vec::new();
    let mem = r.footprint_bytes.unwrap_or(r.peak_rss_bytes);
    let mem_ok = mem <= IDLE_APP_MB * 1_000_000;
    out.push(check(
        "Idle cost",
        if r.cpu_idle && mem_ok {
            Status::Pass
        } else {
            Status::Fail
        },
        Basis::Direct,
        format!(
            "{:.2}% CPU over {:.1} s, {:.1} MB {}",
            r.cpu_pct,
            r.window_s,
            stats::mb(mem),
            if r.footprint_bytes.is_some() {
                "footprint"
            } else {
                "resident"
            }
        ),
        format!(
            "{}; CPU judged idle at {IDLE_CPU_MAX_PCT}% or less (ps resolution is 10 ms)",
            r.target
        ),
    ));
    if let Some(ms) = r.start_to_ready_ms {
        out.push(check(
            "Module start",
            if ms <= MODULE_START_MS as f64 { Status::Pass } else { Status::Fail },
            Basis::Proxy,
            format!("{ms:.1} ms from spawn to registry ready"),
            "process start plus registry build, with no window; an app shell adds its own launch time".to_string(),
        ));
    }
    out
}

/// Fold any number of checks into one row per FLOWS.md budget, in document order. A row with a
/// failing check fails; otherwise a row with a passing check passes; a row with none is "not
/// measured" with the reason it needs.
pub fn merge(checks: Vec<Check>) -> Vec<Check> {
    rows()
        .into_iter()
        .map(|(name, limit, why)| {
            let mine: Vec<&Check> = checks.iter().filter(|c| c.budget == name).collect();
            if mine.is_empty() {
                return Check {
                    budget: name,
                    limit,
                    measured: "-".to_string(),
                    status: Status::NotMeasured,
                    basis: Basis::None,
                    detail: why.to_string(),
                };
            }
            let status = if mine.iter().any(|c| c.status == Status::Fail) {
                Status::Fail
            } else {
                Status::Pass
            };
            let basis = if mine.iter().any(|c| c.basis == Basis::Proxy) {
                Basis::Proxy
            } else {
                Basis::Direct
            };
            Check {
                budget: name,
                limit,
                measured: mine
                    .iter()
                    .map(|c| c.measured.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
                status,
                basis,
                detail: mine
                    .iter()
                    .map(|c| c.detail.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
            }
        })
        .collect()
}

/// True when no row failed.
pub fn all_passed(checks: &[Check]) -> bool {
    !checks.iter().any(|c| c.status == Status::Fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{FileResult, RunReport};

    fn file(id: &str, bytes: u64, peak: u64, wall: f64) -> FileResult {
        FileResult {
            id: id.into(),
            kind: "pdf".into(),
            input_bytes: bytes,
            runs: 1,
            wall_ms: wall,
            wall_ms_max: wall,
            cpu_ms: 1.0,
            inner_ms: None,
            minstr: None,
            peak_mem_bytes: peak,
            max_rss_bytes: peak,
            peak_footprint_bytes: None,
            output_bytes: 0,
            ok: true,
            error: None,
            baseline: None,
        }
    }

    fn report(files: Vec<FileResult>, instant: bool) -> RunReport {
        RunReport {
            module: "m".into(),
            runner: "r".into(),
            runs: 1,
            files,
            skipped_missing: 0,
            instant,
            typical_bytes: 10_000_000,
            baseline_cmd: None,
            speed_tolerance: 1.1,
        }
    }

    #[test]
    fn the_table_has_the_ten_flows_rows() {
        let names: Vec<_> = rows().iter().map(|r| r.0).collect();
        assert_eq!(names.len(), 10);
        assert_eq!(names[0], "Idle cost");
        assert_eq!(names[9], "Output quality");
        assert!(rows()[5].1.contains("300 MB"));
        assert!(rows()[1].1.contains("50 ms"));
        assert!(rows()[2].1.contains("100 ms"));
        assert!(rows()[3].1.contains("4 ms"));
    }

    #[test]
    fn memory_over_the_cap_fails() {
        let ok = from_run(&report(vec![file("a", 5_000_000, 12_000_000, 20.0)], false));
        assert_eq!(ok[0].status, Status::Pass);
        let bad = from_run(&report(
            vec![
                file("a", 5_000_000, 12_000_000, 20.0),
                file("b", 9, 301_000_000, 5.0),
            ],
            false,
        ));
        assert_eq!(bad[0].status, Status::Fail);
        assert!(bad[0].detail.contains('b'));
    }

    #[test]
    fn first_preview_needs_instant_and_typical_files() {
        let files = vec![
            file("small", 1_000_000, 1, 30.0),
            file("big", 900_000_000, 1, 900.0),
        ];
        assert!(
            from_run(&report(files.clone(), false))
                .iter()
                .all(|c| c.budget != "First preview")
        );
        let c = from_run(&report(files, true));
        let fp = c.iter().find(|c| c.budget == "First preview").unwrap();
        assert_eq!(
            fp.status,
            Status::Pass,
            "the 900 MB file is not a typical file"
        );
        assert_eq!(fp.basis, Basis::Proxy);
        let slow = from_run(&report(vec![file("small", 1_000_000, 1, 140.0)], true));
        assert_eq!(
            slow.iter()
                .find(|c| c.budget == "First preview")
                .unwrap()
                .status,
            Status::Fail
        );
    }

    #[test]
    fn huge_row_only_for_inputs_of_a_gigabyte() {
        let c = from_run(&report(vec![file("a", 999_999_999, 1, 1.0)], false));
        assert!(c.iter().all(|c| c.budget != "Huge files"));
        let c = from_run(&report(
            vec![file("a", 2_148_403_300, 9_000_000, 1.0)],
            false,
        ));
        assert_eq!(
            c.iter().find(|c| c.budget == "Huge files").unwrap().status,
            Status::Pass
        );
    }

    #[test]
    fn merge_never_passes_what_was_not_measured() {
        let merged = merge(vec![]);
        assert_eq!(merged.len(), 10);
        assert!(merged.iter().all(|c| c.status == Status::NotMeasured));
        assert!(all_passed(&merged));

        let merged = merge(from_run(&report(vec![file("a", 1, 1_000, 1.0)], false)));
        assert_eq!(
            merged.iter().filter(|c| c.status == Status::Pass).count(),
            1
        );
        assert_eq!(merged[5].budget, "Peak memory per job");
        assert_eq!(merged[5].status, Status::Pass);
        assert_eq!(
            merged[3].status,
            Status::NotMeasured,
            "UI thread is never silently passed"
        );
    }

    #[test]
    fn a_failure_in_any_source_fails_the_row() {
        let pass = from_run(&report(vec![file("a", 1, 1_000, 1.0)], false));
        let mut fail = from_run(&report(vec![file("b", 1, 400_000_000, 1.0)], false));
        let mut all = pass;
        all.append(&mut fail);
        let merged = merge(all);
        assert_eq!(merged[5].status, Status::Fail);
        assert!(!all_passed(&merged));
    }
}
