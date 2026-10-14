//! `panoply-smoke`: the smoke matrix and its one-job child.
//!
//! ```text
//! panoply-smoke matrix [--module ID]... [--per-type N] [--batch N] [--judge-time]
//!                      [--write docs/research/beta-smoke.md] [--json out.json] [--list]
//! panoply-smoke one --module ID [--set option=text]... FILE...      (the child; prints one JSON line)
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use panoply_smoke::{Settings, child, report};

fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  panoply-smoke matrix [--module ID]... [--per-type N] [--batch N] [--judge-time] \
         [--write FILE.md] [--json FILE.json] [--corpus DIR] [--list]\n  panoply-smoke one --module ID [--set option=text]... FILE..."
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return usage();
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "one" => one(args),
        "matrix" => matrix(args),
        _ => usage(),
    }
}

fn one(args: Vec<String>) -> ExitCode {
    let mut module = None;
    let mut files = Vec::new();
    let mut typed = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--module" {
            module = it.next();
        } else if a == "--set" {
            if let Some((k, v)) = it.next().and_then(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            }) {
                typed.push((k, v));
            }
        } else {
            files.push(PathBuf::from(a));
        }
    }
    let Some(module) = module else {
        return usage();
    };
    let result = child::run_one(&module, &files, &typed);
    println!(
        "{}",
        serde_json::to_string(&result).expect("a result serializes")
    );
    ExitCode::from(u8::from(!result.ok()))
}

fn matrix(args: Vec<String>) -> ExitCode {
    let exe = std::env::current_exe().expect("the path of this program");
    let mut s = Settings::new(exe);
    let (mut write, mut json, mut list) = (None, None, false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--module" => s.only.extend(it.next()),
            "--per-type" => s.per_type = it.next().and_then(|v| v.parse().ok()).unwrap_or(2),
            "--batch" => s.batch = it.next().and_then(|v| v.parse().ok()).unwrap_or(3),
            "--judge-time" => s.judge_time = true,
            "--corpus" => s.corpus = it.next().map(PathBuf::from).unwrap_or(s.corpus),
            "--write" => write = it.next().map(PathBuf::from),
            "--json" => json = it.next().map(PathBuf::from),
            "--list" => list = true,
            _ => return usage(),
        }
    }
    if !panoply_smoke::corpus_present(&s.corpus) {
        eprintln!(
            "the corpus is not on disk at {}: run `python3 -I tests/corpus/fetch.py`",
            s.corpus.display()
        );
        return ExitCode::from(2);
    }
    for p in panoply_smoke::build_wasm_examples() {
        eprintln!("warning: {p}");
    }
    if list {
        let (catalog, mut mods) = panoply_smoke::catalog_modules();
        if !s.only.is_empty() {
            mods.retain(|m| s.only.contains(&m.id));
        }
        let cands = panoply_smoke::plan::candidates(&s.corpus);
        let cases = panoply_smoke::plan::plan(
            &mods,
            &catalog,
            &cands,
            panoply_smoke::plan::Options {
                per_type: s.per_type,
                batch: s.batch,
            },
        );
        for c in &cases {
            println!("{:28} {:16} {}", c.module, c.group, c.ids.join(" "));
        }
        println!("{} modules, {} jobs", mods.len(), cases.len());
        return ExitCode::SUCCESS;
    }
    let started = std::time::Instant::now();
    let (_, rows) = panoply_smoke::run_all(&s, |i, n, c| {
        eprintln!("[{i}/{n}] {} {} {}", c.module, c.group, c.ids.join(" "));
    });
    let header = report::Header::detect(s.per_type);
    if let Some(path) = write {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, report::markdown(&rows, &header)) {
            eprintln!("could not write {}: {e}", path.display());
            return ExitCode::from(2);
        }
    }
    if let Some(path) = json {
        let _ = std::fs::write(path, report::json(&rows));
    }
    let bad = rows.iter().filter(|r| !r.clean()).count();
    for r in rows.iter().filter(|r| !r.clean()) {
        println!(
            "FAIL {} {} {}: {}",
            r.module,
            r.group,
            r.ids.join(","),
            r.failure()
                .unwrap_or_else(|| r.budget_violations.join("; "))
        );
    }
    println!(
        "{} jobs, {} not clean, {:.0} s",
        rows.len(),
        bad,
        started.elapsed().as_secs_f64()
    );
    ExitCode::from(u8::from(bad > 0))
}
