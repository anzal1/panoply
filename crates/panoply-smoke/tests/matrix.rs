//! The smoke matrix as a test: every registered module over real corpus files, through the path
//! the desktop app takes (see the crate docs).
//!
//! It runs on every file that is on disk. Without the corpus it skips, unless
//! `PANOPLY_REQUIRE_CORPUS` is set, in which case it fails, the same rule the host's corpus tests
//! follow. `PANOPLY_SMOKE_REPORT=docs/research/beta-smoke.md` writes the table.
//!
//! Memory budgets are always judged. The first preview time budget is judged only in a release
//! build, because a debug build on a shared CI runner says nothing about speed.

use std::path::PathBuf;

use panoply_smoke::{Settings, report};

fn corpus_ready(s: &Settings) -> bool {
    if panoply_smoke::corpus_present(&s.corpus) {
        return true;
    }
    assert!(
        std::env::var_os("PANOPLY_REQUIRE_CORPUS").is_none(),
        "the corpus is missing and PANOPLY_REQUIRE_CORPUS is set: run `python3 -I tests/corpus/fetch.py`"
    );
    eprintln!("skipping: corpus not fetched (python3 -I tests/corpus/fetch.py)");
    false
}

#[test]
fn every_module_runs_on_real_files_within_its_budgets() {
    let mut s = Settings::new(PathBuf::from(env!("CARGO_BIN_EXE_panoply-smoke")));
    if !corpus_ready(&s) {
        return;
    }
    s.judge_time = !cfg!(debug_assertions);
    for problem in panoply_smoke::build_wasm_examples() {
        eprintln!("warning: {problem}");
    }

    let (_, mods) = panoply_smoke::catalog_modules();
    let (cases, rows) = panoply_smoke::run_all(&s, |i, n, c| {
        eprintln!("[{i}/{n}] {} {} {}", c.module, c.group, c.ids.join(" "));
    });

    if let Some(path) = std::env::var_os("PANOPLY_SMOKE_REPORT") {
        let header = report::Header::detect(s.per_type);
        std::fs::write(&path, report::markdown(&rows, &header)).expect("write the report");
    }

    let bad: Vec<String> = rows
        .iter()
        .filter(|r| !r.clean())
        .map(|r| {
            format!(
                "{} [{}] {}: {}",
                r.module,
                r.group,
                r.ids.join(","),
                r.failure()
                    .unwrap_or_else(|| r.budget_violations.join("; "))
            )
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} jobs are not clean:\n{}",
        bad.len(),
        rows.len(),
        bad.join("\n")
    );

    // Coverage: every module that takes files ran on at least one file when the corpus has a
    // file it accepts. A module with no case at all means the plan lost it, not that it passed.
    let ran: std::collections::BTreeSet<&str> = cases.iter().map(|c| c.module.as_str()).collect();
    let missing: Vec<&str> = mods
        .iter()
        .map(|m| m.id.as_str())
        .filter(|id| !ran.contains(id))
        .collect();
    assert!(
        missing.is_empty(),
        "no job was planned for: {}",
        missing.join(", ")
    );
}
