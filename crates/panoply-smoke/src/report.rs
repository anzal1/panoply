//! The markdown table (`docs/research/beta-smoke.md`) and the JSON.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::process::Command;

use panoply_bench::report::HostInfo;
use panoply_bench::stats::{human_bytes, median};

use crate::budgets::EXCEPTIONS;
use crate::child::Outcome;
use crate::matrix::Row;

/// Where and when the run happened.
pub struct Header {
    /// `git rev-parse --short HEAD`, or `unknown`.
    pub commit: String,
    /// `YYYY-MM-DD`, or `unknown`.
    pub date: String,
    /// The machine.
    pub host: HostInfo,
    /// Files per type the plan used.
    pub per_type: usize,
}

impl Header {
    /// Look at this checkout and this machine.
    pub fn detect(per_type: usize) -> Header {
        let run = |prog: &str, args: &[&str]| {
            Command::new(prog)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".into())
        };
        Header {
            commit: run("git", &["rev-parse", "--short", "HEAD"]),
            date: run("date", &["-u", "+%Y-%m-%d"]),
            host: HostInfo::detect(),
            per_type,
        }
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1e6)
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn card(row: &Row) -> String {
    let Some(r) = &row.result else {
        return String::new();
    };
    if !r.headline.trim().is_empty() {
        return cell(&r.headline);
    }
    cell(r.notes.first().map(String::as_str).unwrap_or(""))
}

/// The full markdown report.
pub fn markdown(rows: &[Row], h: &Header) -> String {
    let mut s = String::new();
    let count = |o: Outcome| rows.iter().filter(|r| r.crash.is_none() && r.outcome() == Some(o)).count();
    let (pass, noop, declined) = (count(Outcome::Pass), count(Outcome::NoOp), count(Outcome::Declined));
    let fail = rows.len() - pass - noop - declined;
    let violations: usize = rows.iter().map(|r| r.budget_violations.len()).sum();
    let mut by_module: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        by_module.entry(&r.module).or_default().push(r);
    }

    let _ = writeln!(s, "# Beta smoke matrix\n");
    let _ = writeln!(
        s,
        "Every registered module (built-in and the example WebAssembly modules) over real corpus files, \
         through the path the desktop app takes: the catalog, the options form built from the manifest \
         with its default values, the file sniffed from its bytes, `jobs::run_blocking`, and the result \
         card text from `Done::headline`. No window is opened.\n"
    );
    let _ = writeln!(
        s,
        "- Commit `{}`, {}, {} {} with {} CPUs{}, {} build.",
        h.commit,
        h.date,
        h.host.os,
        h.host.arch,
        h.host.cpus,
        h.host
            .cpu_brand
            .as_ref()
            .map(|b| format!(" ({b})"))
            .unwrap_or_default(),
        h.host.build_profile
    );
    let _ = writeln!(
        s,
        "- {} modules, {} jobs: **{} passed, {} passed with no change, {} declined in plain words, {} failed**, {} undocumented budget violations.",
        by_module.len(),
        rows.len(),
        pass,
        noop,
        declined,
        fail,
        violations
    );
    let _ = writeln!(
        s,
        "- Pass: the job wrote what the manifest promises. Pass with no change: nothing needed doing and the \
         result card says why (already the target format, a report). Declined: the module refused with a \
         plain sentence and a next step, because the file needs a choice from the person or is a variant it \
         cannot read yet. Those are listed under Known gaps. Fail: a crash, a hang, a wrong or empty output, \
         or an error with no next step."
    );
    let _ = writeln!(
        s,
        "- Up to {} real files per accepted file type, plus one batch job per module. Each job runs in \
         its own process under `/usr/bin/time`, so peak memory is that job's own high-water mark (peak \
         footprint on macOS, resident set on Linux), and it includes the registry the process builds.",
        h.per_type
    );
    let _ = writeln!(
        s,
        "- Refused on purpose and not in this matrix: encrypted, truncated and broken files. The host's \
         corpus tests check that those fail in plain words.\n"
    );

    // Summary: modules x pass or fail.
    let _ = writeln!(s, "## Summary\n");
    let _ = writeln!(
        s,
        "| Module | Pattern | Source | Jobs | Pass | No change | Declined | Fail | Budget flags | Median job ms | Worst job ms | Worst peak MB |"
    );
    let _ = writeln!(s, "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (id, rs) in &by_module {
        let n = |o: Outcome| rs.iter().filter(|r| r.crash.is_none() && r.outcome() == Some(o)).count();
        let (p, nc, dc) = (n(Outcome::Pass), n(Outcome::NoOp), n(Outcome::Declined));
        let flags: usize = rs.iter().map(|r| r.budget_violations.len()).sum();
        let jobs: Vec<f64> = rs
            .iter()
            .filter_map(|r| {
                r.result
                    .as_ref()
                    .filter(|x| matches!(x.outcome, Outcome::Pass | Outcome::NoOp))
                    .map(|x| x.job_ms)
            })
            .collect();
        let worst_job = jobs.iter().copied().fold(0.0, f64::max);
        let peak = rs.iter().filter_map(|r| r.peak_mem_bytes).max();
        let first = rs[0];
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {:.0} | {:.0} | {} |",
            id,
            first.pattern,
            if first.community { "WASM example" } else { "built in" },
            rs.len(),
            p,
            nc,
            dc,
            rs.len() - p - nc - dc,
            flags,
            median(&jobs),
            worst_job,
            peak.map(mb).unwrap_or_else(|| "n/a".into())
        );
    }
    let _ = writeln!(s);

    // Failures first, because they are what a reader wants.
    let bad: Vec<&Row> = rows.iter().filter(|r| !r.clean()).collect();
    let gaps: Vec<&Row> = rows.iter().filter(|r| r.declined()).collect();
    let _ = writeln!(s, "## Failures and budget violations\n");
    if bad.is_empty() {
        let _ = writeln!(s, "None. Every job passed and every budget held.\n");
    } else {
        let _ = writeln!(s, "| Module | Files | Problem |");
        let _ = writeln!(s, "|---|---|---|");
        for r in bad {
            let what = r.failure().unwrap_or_else(|| r.budget_violations.join("; "));
            let _ = writeln!(s, "| `{}` | {} | {} |", r.module, cell(&r.ids.join(", ")), cell(&what));
        }
        let _ = writeln!(s);
    }

    // Known gaps: what the modules decline, grouped so a reader sees the reason once.
    let _ = writeln!(s, "## Known gaps (declined in plain words)\n");
    if gaps.is_empty() {
        let _ = writeln!(s, "None.\n");
    } else {
        let _ = writeln!(
            s,
            "Each of these is a refusal the app shows on its result card, with the next step. They are not \
             crashes, but they are places where a person's drop and Return does not finish.\n"
        );
        let mut grouped: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for r in &gaps {
            let res = r.result.as_ref();
            let msg = res.and_then(|x| x.error.clone()).unwrap_or_default();
            // File names differ per case, so key on the sentence without the name in front.
            let key = msg
                .split_once(" is ")
                .filter(|(name, _)| name.contains('.'))
                .map(|(_, rest)| format!("... is {rest}"))
                .unwrap_or(msg);
            let next = res.and_then(|x| x.next_step.clone()).unwrap_or_default();
            grouped
                .entry((r.module.clone(), format!("{key} {next}").trim().to_string()))
                .or_default()
                .push(r.ids.join("+"));
        }
        let _ = writeln!(s, "| Module | What it says | Files |");
        let _ = writeln!(s, "|---|---|---|");
        for ((module, msg), files) in grouped {
            let _ = writeln!(s, "| `{}` | {} | {} |", module, cell(&msg), cell(&files.join(", ")));
        }
        let _ = writeln!(s);
    }

    // Exceptions that were used.
    let used: Vec<&Row> = rows.iter().filter(|r| !r.exceptions_used.is_empty()).collect();
    let _ = writeln!(s, "## Budget exceptions in use\n");
    let _ = writeln!(
        s,
        "These are written down in DECISIONS.md (\"Exceptions to the budgets\") and on the Budgets line \
         of the module's spec. The matrix honours only these:\n"
    );
    let _ = writeln!(s, "| Module | Cap | Source |");
    let _ = writeln!(s, "|---|---|---|");
    for e in EXCEPTIONS {
        let cap = match (e.peak_mb, e.no_first_preview) {
            (Some(mb), true) => format!("{mb} MB peak, no 100 ms first preview"),
            (Some(mb), false) => format!("{mb} MB peak"),
            (None, true) => "no 100 ms first preview".to_string(),
            (None, false) => String::new(),
        };
        let _ = writeln!(s, "| `{}` | {} | {} |", e.module, cap, cell(e.source));
    }
    let _ = writeln!(s);
    if !used.is_empty() {
        let _ = writeln!(s, "Jobs in this run that needed an exception:\n");
        for r in used {
            let _ = writeln!(
                s,
                "- `{}` on {}: {}",
                r.module,
                cell(&r.ids.join(", ")),
                r.exceptions_used.join("; ")
            );
        }
        let _ = writeln!(s);
    }

    // The full table, one section per module.
    let _ = writeln!(s, "## Full table\n");
    let _ = writeln!(
        s,
        "Job ms is the time inside the job (the worker thread, as the result card measures it). \
         Process ms is the whole child, with process start and the registry. Peak MB is the child's \
         high-water mark.\n"
    );
    for (id, rs) in &by_module {
        let first = rs[0];
        let _ = writeln!(s, "### `{}` ({}, pattern {})\n", id, cell(&first.name), first.pattern);
        let _ = writeln!(
            s,
            "| Files | Type | Input | Result | Job ms | Process ms | Peak MB | Output | Result card |"
        );
        let _ = writeln!(s, "|---|---|---:|---|---:|---:|---:|---:|---|");
        for r in rs {
            let (job, out) = match &r.result {
                Some(x) if matches!(x.outcome, Outcome::Pass | Outcome::NoOp) => (
                    format!("{:.0}", x.job_ms),
                    if x.outputs.is_empty() {
                        "report".to_string()
                    } else {
                        human_bytes(x.output_bytes)
                    },
                ),
                _ => ("".into(), "".into()),
            };
            let label = r.outcome().map(Outcome::label).unwrap_or("FAIL");
            let verdict = if r.crash.is_none() && !r.budget_violations.is_empty() {
                format!("{label}, over budget")
            } else {
                label.to_string()
            };
            let text = match r.outcome() {
                Some(Outcome::Pass | Outcome::NoOp) if r.crash.is_none() => card(r),
                _ => cell(
                    &r.result
                        .as_ref()
                        .and_then(|x| x.error.clone())
                        .or_else(|| r.failure())
                        .unwrap_or_default(),
                ),
            };
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {:.0} | {} | {} | {} |",
                cell(&r.ids.join(", ")),
                cell(&if r.typed.is_empty() { r.group.clone() } else { format!("{} (typed {})", r.group, r.typed.join(" ")) }),
                human_bytes(r.input_bytes),
                verdict,
                job,
                r.proc_ms,
                r.peak_mem_bytes.map(mb).unwrap_or_else(|| "n/a".into()),
                out,
                text
            );
        }
        let _ = writeln!(s);
    }
    s
}

/// The rows as JSON, for tools.
pub fn json(rows: &[Row]) -> String {
    serde_json::to_string_pretty(rows).unwrap_or_default()
}
