//! The smoke matrix: every registered module over real corpus files, through the same path the
//! desktop app takes.
//!
//! The app builds its catalog from the host registry plus the community modules, renders each
//! module's options form from its manifest, sniffs the dropped file, and runs
//! [`panoply_desktop::jobs::run_blocking`] with the form's default options. [`child::run_one`] does
//! exactly that with no window, so what passes here is what a person gets from a drop and Return.
//!
//! The pieces:
//!
//! - [`plan`] picks the files: for every module and every file type it accepts, a few real corpus
//!   files, plus one batch job per module.
//! - [`child`] is the process that runs one job and prints one JSON line. Every job gets a process
//!   of its own so the kernel can report that job's peak memory.
//! - [`matrix`] copies the inputs to a scratch folder, starts the child under `time`, and judges
//!   the result against the module's manifest and the FLOWS.md budgets.
//! - [`budgets`] holds the memory caps, with the documented exceptions from the specs.
//! - [`report`] writes the markdown table and the JSON.

pub mod budgets;
pub mod child;
pub mod matrix;
pub mod plan;
pub mod report;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use panoply_bench::corpus::Corpus;

/// What [`run_all`] needs.
pub struct Settings {
    /// The `panoply-smoke` binary, started once per job.
    pub exe: PathBuf,
    /// The corpus folder (`tests/corpus`).
    pub corpus: PathBuf,
    /// Only these module ids, all when empty.
    pub only: Vec<String>,
    /// Files per accepted type.
    pub per_type: usize,
    /// Files in the batch job.
    pub batch: usize,
    /// Judge the first preview time budget.
    pub judge_time: bool,
}

impl Settings {
    /// The defaults: the corpus of this source tree (or `PANOPLY_CORPUS`), every module, two files
    /// per type, three in the batch, time not judged.
    pub fn new(exe: PathBuf) -> Settings {
        Settings {
            exe,
            corpus: Corpus::default_root(),
            only: Vec::new(),
            per_type: 2,
            batch: 3,
            judge_time: false,
        }
    }
}

/// Build the example WebAssembly modules that are not built yet, so the matrix covers them. This
/// needs the `wasm32-wasip2` target and the network the first time (for `wit-bindgen`). A module
/// that cannot be built is left out of the catalog and shows up as missing in the report.
pub fn build_wasm_examples() -> Vec<String> {
    let mut problems = Vec::new();
    let dir = child::examples_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return problems;
    };
    let mut modules: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    modules.sort();
    for m in modules {
        if m.join("target/wasm32-wasip2/release/module.wasm").is_file() {
            continue;
        }
        let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["build", "--release", "--target", "wasm32-wasip2", "--manifest-path"])
            .arg(m.join("Cargo.toml"))
            // The host's own build settings must not leak into a module build.
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_TARGET_DIR")
            .output();
        match status {
            Ok(o) if o.status.success() => {}
            Ok(o) => problems.push(format!(
                "{}: cargo build failed: {}",
                m.display(),
                String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
            )),
            Err(e) => problems.push(format!("{}: could not run cargo: {e}", m.display())),
        }
    }
    problems
}

/// Every module the app lists, as the plan sees them.
pub fn catalog_modules() -> (panoply_desktop::catalog::Catalog, Vec<plan::ModuleInfo>) {
    let catalog = child::catalog();
    let mods = plan::modules(&catalog);
    (catalog, mods)
}

/// Plan and run the whole matrix.
pub fn run_all(
    s: &Settings,
    progress: impl FnMut(usize, usize, &plan::Case),
) -> (Vec<plan::Case>, Vec<matrix::Row>) {
    let (catalog, mut mods) = catalog_modules();
    if !s.only.is_empty() {
        mods.retain(|m| s.only.contains(&m.id));
    }
    let cands = plan::candidates(&s.corpus);
    let cases = plan::plan(
        &mods,
        &catalog,
        &cands,
        plan::Options {
            per_type: s.per_type,
            batch: s.batch,
        },
    );
    let rows = matrix::run(
        &mods,
        &cases,
        &matrix::RunOptions {
            exe: s.exe.clone(),
            judge_time: s.judge_time,
            timeout: Duration::from_secs(matrix::CASE_TIMEOUT_S),
        },
        progress,
    );
    (cases, rows)
}

/// True when `dir` holds the corpus manifests and cached files.
pub fn corpus_present(dir: &Path) -> bool {
    plan::corpus_present(dir)
}
