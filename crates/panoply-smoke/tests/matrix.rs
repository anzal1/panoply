//! The smoke matrix as a test: every registered module over real corpus files, through the path
//! the desktop app takes (see the crate docs). Each job is a pass, an expected refusal (a plain
//! "no" that a rule in `expect.rs` says is right for that input) or a fail, and the test fails on
//! any fail, on any memory budget missed without a written exception, and on a module that never
//! once passed on a real file.
//!
//! It runs on every file that is on disk, and only when the whole corpus is (pictures, PDFs, video
//! and sound: the media modules need `ffmpeg`, so CI, which fetches pictures and PDFs, skips it).
//! Without the whole corpus it skips, unless
//! `PANOPLY_REQUIRE_CORPUS` is set, in which case it fails, the same rule the host's corpus tests
//! follow. `PANOPLY_SMOKE_REPORT=docs/research/beta-smoke.md` writes the table.
//!
//! Memory budgets are always judged. The first preview time budget is judged only in a release
//! build, because a debug build on a shared CI runner says nothing about speed.

use std::path::PathBuf;

use panoply_smoke::{Settings, report};

fn corpus_ready(s: &Settings) -> bool {
    if panoply_smoke::corpus_present(&s.corpus) && panoply_smoke::corpus_complete(&s.corpus) {
        return true;
    }
    assert!(
        std::env::var_os("PANOPLY_REQUIRE_CORPUS").is_none(),
        "the corpus is missing or partial and PANOPLY_REQUIRE_CORPUS is set: run `python3 -I tests/corpus/fetch.py` (all kinds)"
    );
    eprintln!(
        "skipping: the matrix needs the whole corpus on disk (python3 -I tests/corpus/fetch.py), \
         not only pictures and PDFs"
    );
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

    // A module that only ever refused has not been shown to work.
    let never_passed: Vec<&str> = mods
        .iter()
        .map(|m| m.id.as_str())
        .filter(|id| !rows.iter().any(|r| r.module == *id && r.passed()))
        .collect();
    assert!(
        never_passed.is_empty(),
        "no job passed for: {}",
        never_passed.join(", ")
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
