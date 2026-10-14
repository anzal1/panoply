//! End to end checks of the `panoply` binary.

use std::process::Command;

fn panoply(args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

#[test]
fn route_prints_the_chain() {
    assert_eq!(
        panoply(&["route", "docx", "jpg"]),
        ("docx -> pdf -> jpg\n".into(), true)
    );
    assert_eq!(panoply(&["route", "heic", "jpg"]).0, "heic -> jpg\n");
}

#[test]
fn route_flags_model_steps_and_missing_routes() {
    let (out, ok) = panoply(&["route", "mp3", "srt"]);
    assert!(ok && out.starts_with("mp3 -> srt"));
    let (out, ok) = panoply(&["route", "stl", "mp3"]);
    assert!(!ok && out.contains("No route"));
}

#[test]
fn formats_filters_by_family() {
    let (out, ok) = panoply(&["formats", "--family", "audio"]);
    assert!(ok);
    assert_eq!(out.lines().count(), 23);
    assert!(out.lines().all(|l| l.contains("Audio")));
}

#[test]
fn hash_matches_sha256sum_of_abc() {
    let path = std::env::temp_dir().join(format!("panoply-cli-{}.txt", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    let (out, ok) = panoply(&["hash", path.to_str().unwrap()]);
    std::fs::remove_file(&path).unwrap();
    assert!(ok);
    assert!(out.starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  "));
}

#[test]
fn modules_lists_builtin_and_examples() {
    let (out, ok) = panoply(&["modules"]);
    assert!(ok);
    assert!(out.contains("files.hash") && out.contains("built-in"));
    assert!(out.contains("pdf.compress") && out.contains("image.resize_to_kb"));
}

#[test]
fn version_prints() {
    let (out, ok) = panoply(&["version"]);
    assert!(ok && out.starts_with("panoply "));
}

// ---- `panoply run`: community modules built for wasm32-wasip2 -------------------------------

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// `cargo`, at low priority where the platform has `nice`. Windows has no `nice`; the copy that
/// Git for Windows puts on PATH eats the backslashes in the arguments it forwards.
fn low_priority_cargo() -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("nice");
        c.args(["-n", "15"]).arg(env!("CARGO"));
        c
    }
    #[cfg(not(unix))]
    {
        Command::new(env!("CARGO"))
    }
}

/// Build an example module project once per test run, at low priority on three jobs.
fn build_module(name: &str) -> PathBuf {
    static BUILD: Mutex<()> = Mutex::new(());
    let _one_at_a_time = BUILD.lock().unwrap_or_else(|p| p.into_inner());
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/examples-wasm")
        .join(name);
    let status = low_priority_cargo()
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-j3",
            "--quiet",
        ])
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(
        status.success(),
        "build failed (rustup target add wasm32-wasip2?)"
    );
    dir.join("module.toml")
}

fn panoply_full(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn run_reports_from_a_pattern_i_module() {
    let module = build_module("word-count");
    let d = scratch("wc");
    let f = d.join("poem.txt");
    std::fs::write(&f, "roses are red\nviolets are blue\n").unwrap();
    let (out, _, ok) = panoply_full(&["run", module.to_str().unwrap(), f.to_str().unwrap()]);
    assert!(ok);
    assert_eq!(out, "poem.txt: 2 lines, 6 words, 31 characters, 31 bytes\n");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn run_writes_next_to_the_original_or_into_out_and_never_overwrites() {
    let module = build_module("slugify-lines");
    let module = module.to_str().unwrap();
    let d = scratch("slug");
    let f = d.join("t.txt");
    std::fs::write(&f, "Hello There\n").unwrap();
    let f = f.to_str().unwrap();
    let (out, _, ok) = panoply_full(&["run", module, f, "--opt", "separator=_"]);
    assert!(ok && out.contains("t (slugified).txt"), "{out}");
    assert_eq!(
        std::fs::read_to_string(d.join("t (slugified).txt")).unwrap(),
        "hello_there"
    );
    let (out, _, _) = panoply_full(&["run", module, f]);
    assert!(out.contains("t (slugified 2).txt"), "{out}");
    let to = d.join("results");
    std::fs::create_dir(&to).unwrap();
    let (out, _, ok) = panoply_full(&["run", module, f, "--out", to.to_str().unwrap()]);
    assert!(ok && out.contains("results"), "{out}");
    assert!(to.join("t (slugified).txt").is_file());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn run_explains_mistakes_in_plain_words_and_fails() {
    let module = build_module("slugify-lines");
    let module = module.to_str().unwrap();
    let d = scratch("mistakes");
    let f = d.join("t.txt");
    std::fs::write(&f, "x").unwrap();
    let f = f.to_str().unwrap();
    let (_, err, ok) = panoply_full(&["run", module, f, "--opt", "colour=red"]);
    assert!(
        !ok && err.contains("not an option") && err.contains("separator"),
        "{err}"
    );
    let (_, err, ok) = panoply_full(&["run", module]);
    assert!(!ok && err.contains("at least one file"), "{err}");
    let (_, err, ok) = panoply_full(&["run", d.join("missing/module.toml").to_str().unwrap(), f]);
    assert!(!ok && err.contains("Could not read"), "{err}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn modules_lists_community_modules() {
    let (out, ok) = panoply(&["modules"]);
    assert!(ok);
    assert!(out.contains("text.slugify_lines") && out.contains("text.word_count"));
    assert!(out.contains("community"));
}
