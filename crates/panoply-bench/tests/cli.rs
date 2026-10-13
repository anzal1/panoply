//! End to end checks of the `panoply-bench` binary: the JSON it writes, the exit codes, and that
//! sparse files are gone afterwards.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bench(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_panoply-bench"))
        .args(args)
        .output()
        .unwrap()
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pb-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn can_measure() -> bool {
    Path::new("/usr/bin/time").is_file() && !cfg!(windows)
}

fn row<'a>(v: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    v["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["budget"] == name)
        .unwrap_or_else(|| panic!("no budget row {name}"))
}

#[test]
fn run_on_a_file_writes_json_and_passes_memory() {
    if !can_measure() {
        return;
    }
    let dir = scratch("run");
    let file = dir.join("input.bin");
    std::fs::write(&file, vec![7u8; 3_000_000]).unwrap();
    let out = bench(&[
        "run",
        "--files",
        file.to_str().unwrap(),
        "--runs",
        "1",
        "--instant",
        "--json",
        "-",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(v["schema"], 1);
    assert_eq!(v["command"], "run");
    assert_eq!(v["budgets"].as_array().unwrap().len(), 10);
    let f = &v["run"]["files"][0];
    assert_eq!(f["ok"], true);
    assert_eq!(f["input_bytes"], 3_000_000);
    assert!(f["peak_mem_bytes"].as_u64().unwrap() > 0);
    assert_eq!(row(&v, "Peak memory per job")["status"], "pass");
    assert_eq!(row(&v, "First preview")["status"], "pass");
    assert_eq!(row(&v, "Speed")["status"], "not_measured");
    assert_eq!(row(&v, "UI thread")["status"], "not_measured");
    assert_eq!(v["passed"], true);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn memory_over_the_cap_fails_with_exit_code_1() {
    if !can_measure() {
        return;
    }
    // A stand-in "module" that holds 400 MB: the harness must flag it. Skipped without python3.
    let py = panoply_bench::runner::resolve_program("python3");
    if !py.is_file()
        || Command::new(&py)
            .arg("--version")
            .output()
            .map_or(true, |o| !o.status.success())
    {
        return;
    }
    let dir = scratch("over");
    let file = dir.join("input.bin");
    std::fs::write(&file, b"x").unwrap();
    let out = bench(&[
        "run",
        "--runner",
        "cli",
        "--cli-bin",
        py.to_str().unwrap(),
        "--cli-args",
        "-c x=b'x'*400000000;print(len(x))",
        "--files",
        file.to_str().unwrap(),
        "--runs",
        "1",
        "--json",
        "-",
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    assert_eq!(row(&v, "Peak memory per job")["status"], "fail");
    assert_eq!(v["passed"], false);
    assert!(v["run"]["files"][0]["peak_mem_bytes"].as_u64().unwrap() > 300_000_000);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scale_is_flat_and_deletes_its_sparse_files() {
    if !can_measure() {
        return;
    }
    let tmp = scratch("scale");
    let out = Command::new(env!("CARGO_BIN_EXE_panoply-bench"))
        .args(["scale", "--sizes", "1MiB,48MiB,256MiB", "--json", "-"])
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    let s = &v["scale"];
    assert_eq!(s["constant"], true, "{s}");
    assert_eq!(s["points"].as_array().unwrap().len(), 3);
    assert_eq!(s["points"][2]["input_bytes"], 268_435_456u64);
    assert!(s["spread_bytes"].as_u64().unwrap() < 8_000_000);
    assert_eq!(row(&v, "Peak memory per job")["status"], "pass");
    let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().flatten().collect();
    assert!(left.is_empty(), "sparse files were left behind: {left:?}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn idle_host_reports_cpu_and_memory() {
    if !can_measure() {
        return;
    }
    let out = bench(&[
        "idle",
        "--settle-s",
        "0.3",
        "--window-s",
        "1",
        "--interval-ms",
        "250",
        "--json",
        "-",
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = json(&out);
    let i = &v["idle"];
    assert!(i["peak_rss_bytes"].as_u64().unwrap() > 0);
    assert!(i["cpu_pct"].as_f64().unwrap() < 5.0, "{i}");
    assert!(i["start_to_ready_ms"].as_f64().unwrap() > 0.0);
    assert_eq!(row(&v, "Module start")["basis"], "proxy");
}

#[test]
fn unknown_module_is_a_harness_error() {
    let dir = scratch("unknown");
    let file = dir.join("a");
    std::fs::write(&file, b"x").unwrap();
    let out = bench(&[
        "run",
        "--module",
        "no.such",
        "--files",
        file.to_str().unwrap(),
        "--runs",
        "1",
    ]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("unknown module no.such") && err.contains("files.hash"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn budgets_lists_all_ten_rows_and_never_passes_unmeasured_ones() {
    let out = bench(&["budgets"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for name in [
        "Idle cost",
        "Module start",
        "First preview",
        "UI thread",
        "Frame rate",
        "Peak memory per job",
        "Huge files",
        "Models",
        "Speed",
        "Output quality",
    ] {
        assert!(text.contains(name), "{name}");
    }
    assert!(text.contains("0 passed, 0 failed, 10 not measured"));
}

#[test]
fn corpus_command_reads_the_real_manifests() {
    let out = bench(&["corpus"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for kind in ["pdf", "image", "video", "audio", "office"] {
        assert!(text.lines().any(|l| l.starts_with(kind)), "{kind}\n{text}");
    }
}
