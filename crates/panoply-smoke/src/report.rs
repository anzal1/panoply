//! The markdown table (`docs/research/beta-smoke.md`) and the JSON.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::process::Command;

use panoply_bench::report::HostInfo;
use panoply_bench::stats::{human_bytes, median};

use crate::budgets::EXCEPTIONS;
use crate::child::Outcome;
use crate::expect::RULES;
use crate::matrix::{Row, Verdict};

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
    let text = if r.headline.trim().is_empty() {
        r.notes.first().map(String::as_str).unwrap_or("")
    } else {
        &r.headline
    };
    // The temporary folder is different every run, and the card of a long answer is long.
    let text = text.replace(&std::env::temp_dir().to_string_lossy().to_string(), "$TMP");
    let short: String = text.chars().take(140).collect();
    cell(&if short.len() < text.len() {
        format!("{short}...")
    } else {
        short
    })
}

/// The full markdown report.
pub fn markdown(rows: &[Row], h: &Header) -> String {
    let mut s = String::new();
    let pass = rows.iter().filter(|r| r.verdict() == Verdict::Pass).count();
    let refused = rows.iter().filter(|r| r.declined()).count();
    let fail = rows.iter().filter(|r| r.verdict() == Verdict::Fail).count();
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
        "- {} modules, {} jobs: **{} pass, {} expected refusals, {} fail**, {} budget violations that are not documented exceptions.",
        by_module.len(),
        rows.len(),
        pass,
        refused,
        fail,
        violations
    );
    let _ = writeln!(
        s,
        "- Pass: the job wrote what the manifest promises, or did nothing and the result card says why \
         (already the target format, a report, the answer on the card). Expected refusal: the module \
         declined with a plain sentence and a next step, and a rule in `crates/panoply-smoke/src/expect.rs` \
         says that was right for this input (see Expected refusals below). Fail: a crash, a hang, a wrong \
         or empty output, an error with no next step, or a refusal no rule explains."
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
        "| Module | Pattern | Source | Jobs | Pass | Expected refusal | Fail | Budget flags | Median job ms | Worst job ms | Worst peak MB |"
    );
    let _ = writeln!(s, "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for (id, rs) in &by_module {
        let n = |v: fn(&Verdict) -> bool| rs.iter().filter(|r| v(&r.verdict())).count();
        let p = n(|v| *v == Verdict::Pass);
        let rf = n(|v| matches!(v, Verdict::Refused(_)));
        let fl = n(|v| *v == Verdict::Fail);
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
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {:.0} | {:.0} | {} |",
            id,
            first.pattern,
            if first.community {
                "WASM example"
            } else {
                "built in"
            },
            rs.len(),
            p,
            rf,
            fl,
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
            let what = r
                .failure()
                .unwrap_or_else(|| r.budget_violations.join("; "));
            let _ = writeln!(
                s,
                "| `{}` | {} | {} |",
                r.module,
                cell(&r.ids.join(", ")),
                cell(&what)
            );
        }
        let _ = writeln!(s);
    }

    // Expected refusals: grouped by rule, so a reader sees each reason once.
    let _ = writeln!(s, "## Expected refusals\n");
    if gaps.is_empty() {
        let _ = writeln!(s, "None.\n");
    } else {
        let _ = writeln!(
            s,
            "Each is a refusal the app shows on its result card in plain words with a next step, \
             for an input the module is right to refuse. A refusal no rule explains is a failure.\n"
        );
        let _ = writeln!(s, "| Rule | Why it is right | Jobs | Modules |");
        let _ = writeln!(s, "|---|---|---:|---|");
        for rule in RULES {
            let hits: Vec<&&Row> = gaps
                .iter()
                .filter(|r| r.verdict() == Verdict::Refused(rule.id))
                .collect();
            if hits.is_empty() {
                continue;
            }
            let mut modules: Vec<&str> = hits.iter().map(|r| r.module.as_str()).collect();
            modules.sort_unstable();
            modules.dedup();
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} |",
                cell(rule.id),
                cell(rule.why),
                hits.len(),
                modules
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let _ = writeln!(s);
    }

    // Exceptions that were used.
    let used: Vec<&Row> = rows
        .iter()
        .filter(|r| !r.exceptions_used.is_empty())
        .collect();
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
        let _ = writeln!(
            s,
            "### `{}` ({}, pattern {})\n",
            id,
            cell(&first.name),
            first.pattern
        );
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
            let label = match r.verdict() {
                Verdict::Pass => "PASS",
                Verdict::Refused(_) => "EXPECTED REFUSAL",
                Verdict::Fail => "FAIL",
            };
            let verdict = if r.crash.is_none() && !r.budget_violations.is_empty() {
                format!("{label}, over budget")
            } else {
                label.to_string()
            };
            let text = match r.outcome() {
                Some(Outcome::Pass | Outcome::NoOp) if r.crash.is_none() => card(r),
                Some(Outcome::Declined) if !r.declined() => cell(&r.failure().unwrap_or_default()),
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
                cell(&if r.typed.is_empty() {
                    r.group.clone()
                } else {
                    format!("{} (typed {})", r.group, r.typed.join(" "))
                }),
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
