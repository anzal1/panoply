//! Running the cases: a scratch copy of the inputs, a child process under `time`, a verdict.
//!
//! Inputs are copied to a scratch folder first, because the app writes the result next to the
//! original and the corpus must never collect outputs. The copy happens before the child starts,
//! so it is not in the child's time or memory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use panoply_bench::measure::{self, TimeStats};
use serde::{Deserialize, Serialize};

use crate::budgets;
use crate::child::{OneResult, Outcome};
use crate::plan::{Case, ModuleInfo};

/// Longest a single job may run before the matrix kills it and calls it a hang, seconds.
pub const CASE_TIMEOUT_S: u64 = 300;

/// One finished case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    /// Module id.
    pub module: String,
    /// Display name.
    pub name: String,
    /// Flow pattern letter.
    pub pattern: String,
    /// True for the example WebAssembly modules.
    pub community: bool,
    /// File type covered, `batch of N type`, or `none`.
    pub group: String,
    /// Corpus ids of the inputs.
    pub ids: Vec<String>,
    /// What was typed into the options form first, as `option=text`.
    #[serde(default)]
    pub typed: Vec<String>,
    /// The traits of each input (corpus tags, `no-audio`), in the order of `ids`.
    #[serde(default)]
    pub traits: Vec<Vec<String>>,
    /// Input size, bytes.
    pub input_bytes: u64,
    /// What the job reported. `None` when the child died without a report.
    pub result: Option<OneResult>,
    /// Why there is no report: a crash, a hang, or a start failure.
    pub crash: Option<String>,
    /// Wall time of the whole child process, milliseconds.
    pub proc_ms: f64,
    /// Peak memory of the child in bytes (footprint on macOS, resident set elsewhere), when the
    /// platform can measure it.
    pub peak_mem_bytes: Option<u64>,
    /// Budget violations that are not covered by a documented exception.
    pub budget_violations: Vec<String>,
    /// Budget exceptions that applied, with their source.
    pub exceptions_used: Vec<String>,
}

/// What the matrix says about one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The job did what the manifest promises, or did nothing and said why.
    Pass,
    /// The module declined in plain words, and the named rule says that was right.
    Refused(&'static str),
    /// A crash, a hang, a wrong or empty output, an error with no next step, or a refusal that
    /// no rule explains.
    Fail,
}

impl Row {
    /// What the matrix says about this job, before budgets.
    pub fn verdict(&self) -> Verdict {
        if self.crash.is_some() {
            return Verdict::Fail;
        }
        match self.outcome() {
            Some(Outcome::Pass | Outcome::NoOp) => Verdict::Pass,
            Some(Outcome::Declined) => crate::expect::explain(self)
                .map(|r| Verdict::Refused(r.id))
                .unwrap_or(Verdict::Fail),
            _ => Verdict::Fail,
        }
    }

    /// True when the job worked and produced what the manifest promises.
    pub fn passed(&self) -> bool {
        self.verdict() == Verdict::Pass
    }

    /// The outcome, `None` when the child died without a report.
    pub fn outcome(&self) -> Option<Outcome> {
        self.result.as_ref().map(|r| r.outcome)
    }

    /// True when the module refused, in plain words, a file it is right to refuse.
    pub fn declined(&self) -> bool {
        matches!(self.verdict(), Verdict::Refused(_))
    }

    /// True when the job passed or was refused for a reason that holds, and no undocumented
    /// budget was missed.
    pub fn clean(&self) -> bool {
        self.verdict() != Verdict::Fail && self.budget_violations.is_empty()
    }

    /// Why it failed, in one line.
    pub fn failure(&self) -> Option<String> {
        if let Some(c) = &self.crash {
            return Some(c.clone());
        }
        let r = self.result.as_ref()?;
        if r.outcome == Outcome::Declined {
            if self.declined() {
                return None;
            }
            return Some(format!(
                "declined, and no rule explains it for this input: {} {}",
                r.error.clone().unwrap_or_default(),
                r.next_step.clone().unwrap_or_default()
            ));
        }
        if r.ok() {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(e) = &r.error {
            parts.push(e.clone());
        }
        if let Some(n) = r.next_step.as_ref().filter(|n| !n.is_empty()) {
            parts.push(n.clone());
        }
        parts.extend(r.problems.iter().cloned());
        Some(parts.join(" "))
    }

    /// The file extensions of the inputs' type, for exception lookup.
    fn exts(&self) -> Vec<String> {
        let group = self.group.rsplit(' ').next().unwrap_or("");
        vec![group.to_string()]
    }
}

/// What to run and how.
pub struct RunOptions {
    /// The `panoply-smoke` binary, which is started as the child.
    pub exe: PathBuf,
    /// Judge time budgets too. Time on a debug build or a shared runner means little, so the
    /// first preview row is only judged when this is set.
    pub judge_time: bool,
    /// Kill a job after this long.
    pub timeout: Duration,
}

/// Run every case, one at a time (so the machine is quiet while a job is measured).
pub fn run(
    mods: &[ModuleInfo],
    cases: &[Case],
    opts: &RunOptions,
    mut progress: impl FnMut(usize, usize, &Case),
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        progress(i + 1, cases.len(), case);
        let m = mods.iter().find(|m| m.id == case.module);
        rows.push(run_case(m, case, opts));
    }
    rows
}

fn scratch() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "panoply-smoke-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Copy the inputs into `dir`, keeping file names (a clash gets an index in front).
fn stage(dir: &Path, files: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let mut staged = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let name = f.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        let mut dest = dir.join(&name);
        if dest.exists() {
            let mut n = OsString::from(format!("{i}-"));
            n.push(&name);
            dest = dir.join(n);
        }
        std::fs::copy(f, &dest)?;
        staged.push(dest);
    }
    Ok(staged)
}

fn run_case(m: Option<&ModuleInfo>, case: &Case, opts: &RunOptions) -> Row {
    let dir = scratch();
    let mut row = Row {
        module: case.module.clone(),
        name: m.map(|m| m.name.clone()).unwrap_or_default(),
        pattern: m.map(|m| m.pattern.clone()).unwrap_or_default(),
        community: m.is_some_and(|m| m.community),
        group: case.group.clone(),
        ids: case.ids.clone(),
        typed: case.typed.iter().map(|(k, v)| format!("{k}={v}")).collect(),
        traits: case.traits.clone(),
        input_bytes: case.bytes,
        result: None,
        crash: None,
        proc_ms: 0.0,
        peak_mem_bytes: None,
        budget_violations: Vec::new(),
        exceptions_used: Vec::new(),
    };
    match stage(&dir, &case.files) {
        Err(e) => row.crash = Some(format!("could not copy the input: {e}")),
        Ok(staged) => {
            let mut args: Vec<OsString> =
                vec!["one".into(), "--module".into(), (&case.module).into()];
            for (k, v) in &case.typed {
                args.push("--set".into());
                args.push(format!("{k}={v}").into());
            }
            args.extend(staged.iter().map(|p| p.as_os_str().to_owned()));
            match spawn(&opts.exe, &args, &dir, opts.timeout) {
                Err(e) => row.crash = Some(e),
                Ok(done) => {
                    row.proc_ms = done.wall.as_secs_f64() * 1000.0;
                    row.peak_mem_bytes = done.stats.as_ref().map(TimeStats::peak_mem_bytes);
                    match parse_report(&done.stdout) {
                        Some(r) => row.result = Some(r),
                        None => {
                            row.crash = Some(format!(
                                "the job process {} without a report: {}",
                                match done.code {
                                    Some(c) => format!("exited with {c}"),
                                    None => "was killed".into(),
                                },
                                first_line(&done.stderr)
                            ));
                        }
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    judge(&mut row, opts.judge_time);
    row
}

fn first_line(s: &str) -> String {
    let line = s
        .lines()
        .find(|l| l.contains("panicked") || l.contains("error") || l.contains("Error"))
        .or_else(|| s.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("");
    line.trim().chars().take(240).collect()
}

fn parse_report(stdout: &str) -> Option<OneResult> {
    stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<OneResult>(l.trim()).ok())
}

/// Fill in the budget verdicts of a row.
fn judge(row: &mut Row, judge_time: bool) {
    let Some(r) = row.result.as_ref().filter(|_| row.crash.is_none()) else {
        return;
    };
    if !matches!(r.outcome, Outcome::Pass | Outcome::NoOp) {
        return;
    }
    let exts = row.exts();
    let (cap, exception) = budgets::peak_cap(&row.module, &exts, &row.ids);
    if let Some(peak) = row.peak_mem_bytes {
        if peak > cap {
            row.budget_violations.push(format!(
                "peak memory {:.0} MB is over the {:.0} MB cap",
                peak as f64 / 1e6,
                cap as f64 / 1e6
            ));
        } else if let Some(e) = exception
            && peak > panoply_core::budgets::PEAK_MEM_DOC_MB * 1_000_000
        {
            row.exceptions_used.push(format!(
                "peak memory {:.0} MB, above 300 MB: {}",
                peak as f64 / 1e6,
                e.source
            ));
        }
    }
    if budgets::first_preview_applies(&row.module, &row.pattern, row.input_bytes, &exts, &row.ids)
        && r.job_ms > budgets::first_preview_ms()
        && judge_time
    {
        row.budget_violations.push(format!(
            "an instant transform took {:.0} ms, the first preview budget is {:.0} ms",
            r.job_ms,
            budgets::first_preview_ms()
        ));
    }
}

/// What a finished child left behind.
struct Finished {
    stdout: String,
    stderr: String,
    wall: Duration,
    stats: Option<TimeStats>,
    code: Option<i32>,
}

/// Start the child under `time` when the platform has it, else plain. Output goes to files, so a
/// chatty child cannot fill a pipe while this waits, and a hung child is killed at the deadline.
fn spawn(exe: &Path, args: &[OsString], dir: &Path, timeout: Duration) -> Result<Finished, String> {
    let out_path = dir.join(".smoke-stdout");
    let err_path = dir.join(".smoke-stderr");
    let out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let err = std::fs::File::create(&err_path).map_err(|e| e.to_string())?;
    let measured = measure::available();
    let mut cmd = if measured {
        let mut c = Command::new("/usr/bin/time");
        c.arg(if cfg!(target_os = "macos") {
            "-l"
        } else {
            "-v"
        })
        .arg(exe);
        c
    } else {
        Command::new(exe)
    };
    cmd.args(args).stdin(Stdio::null()).stdout(out).stderr(err);
    let started = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the job ran for more than {} s and was stopped",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let wall = started.elapsed();
    let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
    let raw_err = std::fs::read_to_string(&err_path).unwrap_or_default();
    let (stats, stderr) = if measured {
        match measure::parse_time_output(&raw_err) {
            Some((s, e)) => (Some(s), e),
            None => (None, raw_err),
        }
    } else {
        (None, raw_err)
    };
    Ok(Finished {
        stdout,
        stderr,
        wall,
        stats,
        code: status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::{OneResult, Outcome};
    use serde_json::Value;

    fn ok_result() -> OneResult {
        OneResult {
            module: "pdf.compress".into(),
            outcome: Outcome::Pass,
            error: None,
            next_step: None,
            headline: "4.2 MB to 880 KB, 79% smaller".into(),
            notes: vec![],
            outputs: vec![],
            input_bytes: 10,
            output_bytes: 5,
            job_ms: 20.0,
            catalog_ms: 1.0,
            options: Value::Null,
            problems: vec![],
        }
    }

    fn row(module: &str, group: &str, peak_mb: u64, job_ms: f64) -> Row {
        let mut r = ok_result();
        r.job_ms = job_ms;
        Row {
            module: module.into(),
            name: String::new(),
            pattern: "A".into(),
            community: false,
            group: group.into(),
            ids: vec![],
            typed: vec![],
            traits: vec![],
            input_bytes: 1_000_000,
            result: Some(r),
            crash: None,
            proc_ms: job_ms,
            peak_mem_bytes: Some(peak_mb * 1_000_000),
            budget_violations: vec![],
            exceptions_used: vec![],
        }
    }

    #[test]
    fn peak_memory_over_the_cap_is_a_violation_unless_the_spec_grants_it() {
        let mut a = row("pdf.compress", "pdf", 350, 20.0);
        judge(&mut a, false);
        assert_eq!(a.budget_violations.len(), 1, "{:?}", a.budget_violations);

        let mut b = row("image.remove_background", "png", 900, 20.0);
        judge(&mut b, false);
        assert!(b.budget_violations.is_empty());
        assert_eq!(b.exceptions_used.len(), 1);
    }

    #[test]
    fn the_first_preview_row_needs_the_time_flag_and_an_instant_transform() {
        let mut slow = row("pdf.merge", "pdf", 50, 400.0);
        judge(&mut slow, false);
        assert!(
            slow.budget_violations.is_empty(),
            "time is not judged by default"
        );
        judge(&mut slow, true);
        assert_eq!(slow.budget_violations.len(), 1);

        let mut tuned = row("pdf.compress", "pdf", 50, 400.0);
        tuned.pattern = "B".into();
        judge(&mut tuned, true);
        assert!(tuned.budget_violations.is_empty());
    }

    #[test]
    fn a_report_is_found_on_the_last_json_line() {
        let r = ok_result();
        let line = serde_json::to_string(&r).unwrap();
        let out = format!("noise\n{line}\n");
        assert_eq!(parse_report(&out).unwrap().headline, r.headline);
        assert!(parse_report("no json here").is_none());
    }

    #[test]
    fn staging_keeps_names_and_breaks_clashes() {
        let d = scratch();
        let a = d.join("a/x.txt");
        let b = d.join("b/x.txt");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        std::fs::write(&a, b"1").unwrap();
        std::fs::write(&b, b"2").unwrap();
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let staged = stage(&out, &[a, b]).unwrap();
        assert_eq!(staged[0].file_name().unwrap(), "x.txt");
        assert_eq!(staged[1].file_name().unwrap(), "1-x.txt");
        let _ = std::fs::remove_dir_all(d);
    }
}
