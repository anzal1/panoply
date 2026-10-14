//! End to end checks of the WebAssembly host: real components built for wasm32-wasip2, run
//! through the same `Registry::run_job` path the CLI uses.
//!
//! The adversary fixture misbehaves on request. Each test pins one promise of the sandbox:
//! chosen files only, no network, bounded memory, instant cancel, no partial files, and
//! outputs that never overwrite.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use panoply_core::{CancelToken, JobEvent, JobSpec};
use panoply_host::{JobOutcome, ModuleError, Registry};
use panoply_wasm::{Limits, WasmModule};
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

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

/// Build a module project once per test process. Low priority and three jobs, to keep the
/// machine cool.
fn build(dir: &Path) -> PathBuf {
    static DONE: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    let mut done = DONE.lock().unwrap();
    let set = done.get_or_insert_with(HashSet::new);
    if set.insert(dir.to_path_buf()) {
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
            // A target folder set for the host build (CI sets one) would take the module out of the
            // project folder the tests look in.
            .env_remove("CARGO_TARGET_DIR")
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "building {} failed (rustup target add wasm32-wasip2?)",
            dir.display()
        );
    }
    dir.to_path_buf()
}

/// Build a copy of a module project in a scratch folder, so the original never grows a
/// 140 MB `target` directory that `cp -r` would then drag along.
fn build_copy(src: &Path, tag: &str) -> PathBuf {
    let dir = scratch(tag);
    for entry in ["Cargo.toml", "module.toml"] {
        std::fs::copy(src.join(entry), dir.join(entry)).unwrap();
    }
    for sub in ["src", "wit"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for f in std::fs::read_dir(src.join(sub)).unwrap() {
            let f = f.unwrap().path();
            std::fs::copy(&f, dir.join(sub).join(f.file_name().unwrap())).unwrap();
        }
    }
    build(&dir)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-wasm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Ran {
    result: Result<JobOutcome, ModuleError>,
    events: Vec<JobEvent>,
}

fn run_with(
    dir: &Path,
    inputs: &[PathBuf],
    options: Value,
    out: Option<&Path>,
    cancel: CancelToken,
    limits: Option<Limits>,
) -> Ran {
    let mut module = WasmModule::load(dir).expect("module loads");
    if let Some(l) = limits {
        module = module.with_limits(l);
    }
    let id = panoply_host::Module::manifest(&module).module.id.clone();
    let mut registry = Registry::new();
    registry.register(Arc::new(module)).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let spec = JobSpec {
        module_id: id,
        inputs: inputs.to_vec(),
        options,
    };
    let result = registry.run_job(&spec, out, cancel, move |e| sink.lock().unwrap().push(e));
    let events = events.lock().unwrap().clone();
    Ran { result, events }
}

fn run(dir: &Path, inputs: &[PathBuf], options: Value) -> Ran {
    run_with(dir, inputs, options, None, CancelToken::new(), None)
}

fn adversary(mode: &str, input: &Path, out: Option<&Path>) -> Ran {
    let dir = build(&fixture("adversary"));
    run_with(
        &dir,
        &[input.to_path_buf()],
        json!({ "mode": mode }),
        out,
        CancelToken::new(),
        None,
    )
}

fn files_in(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn input(tag: &str, name: &str, body: &str) -> (PathBuf, PathBuf) {
    let d = scratch(tag);
    let p = d.join(name);
    std::fs::write(&p, body).unwrap();
    (d, p)
}

// ---------------------------------------------------------------- the examples

#[test]
fn slugify_lines_writes_a_new_file_and_never_overwrites() {
    let dir = build(&repo().join("modules/examples-wasm/slugify-lines"));
    let (d, p) = input(
        "slug",
        "titles.txt",
        "Hello, World!\n  Rust & WebAssembly  \n",
    );
    let first = run(&dir, std::slice::from_ref(&p), json!({}))
        .result
        .unwrap();
    let second = run(&dir, std::slice::from_ref(&p), json!({"separator": "_"}))
        .result
        .unwrap();
    assert_eq!(first.outputs, vec![d.join("titles (slugified).txt")]);
    assert_eq!(second.outputs, vec![d.join("titles (slugified 2).txt")]);
    assert_eq!(
        std::fs::read_to_string(&first.outputs[0]).unwrap(),
        "hello-world\nrust-webassembly"
    );
    assert_eq!(
        std::fs::read_to_string(&second.outputs[0]).unwrap(),
        "hello_world\nrust_webassembly"
    );
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "Hello, World!\n  Rust & WebAssembly  \n"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn word_count_reports_notes_and_writes_nothing() {
    let dir = build(&repo().join("modules/examples-wasm/word-count"));
    let (d, p) = input("wc", "essay.txt", "one two\nthree\n");
    let out = run(&dir, &[p], json!({})).result.unwrap();
    assert!(out.outputs.is_empty());
    assert_eq!(
        out.notes,
        vec!["essay.txt: 2 lines, 3 words, 14 characters, 14 bytes"]
    );
    assert_eq!(files_in(&d), vec!["essay.txt"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn the_template_builds_and_runs_as_shipped() {
    let dir = build_copy(&repo().join("modules/template-rust"), "tpl-build");
    let (d, p) = input("tpl", "a.txt", "hi\nthere\n");
    let out = run(&dir, &[p], json!({"separator": "?"})).result.unwrap();
    assert_eq!(
        std::fs::read_to_string(&out.outputs[0]).unwrap(),
        "HI?\nTHERE?\n"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn files_the_manifest_does_not_accept_are_refused_in_plain_words() {
    let dir = build(&repo().join("modules/examples-wasm/word-count"));
    let d = scratch("refuse");
    let p = d.join("pic.png");
    std::fs::write(
        &p,
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0],
    )
    .unwrap();
    let err = run(&dir, &[p], json!({})).result.unwrap_err();
    assert!(
        matches!(&err, ModuleError::Failed(m) if m.contains("image/png") && m.contains("text/*")),
        "{err}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn bad_options_stop_before_the_module_runs() {
    let dir = build(&repo().join("modules/examples-wasm/slugify-lines"));
    let (d, p) = input("badopt", "a.txt", "x");
    let err = run(&dir, &[p], json!({"separator": "~"}))
        .result
        .unwrap_err();
    assert!(matches!(err, ModuleError::BadOption(_)));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_missing_build_says_how_to_build() {
    let d = scratch("nobuild");
    std::fs::copy(
        repo().join("modules/examples-wasm/word-count/module.toml"),
        d.join("module.toml"),
    )
    .unwrap();
    let err = WasmModule::load(&d).err().unwrap().to_string();
    assert!(
        err.contains("cargo build --release --target wasm32-wasip2"),
        "{err}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

// ------------------------------------------------------------------ the sandbox

#[test]
fn the_module_sees_only_chosen_files_and_no_ambient_authority() {
    let (d, p) = input("fs", "secret.txt", "chosen but unreachable by name");
    let notes = adversary("readfs", &p, None).result.unwrap().notes;
    let joined = notes.join("\n");
    assert!(!notes.is_empty());
    for line in notes
        .iter()
        .filter(|l| l.starts_with("read ") || l.starts_with("write"))
    {
        assert!(line.contains("Err"), "ambient file access worked: {line}");
    }
    assert!(joined.contains("env 0"), "environment leaked: {joined}");
    assert!(!d.join("pwned.txt").exists());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_handle_that_was_not_handed_out_is_refused() {
    let (d, p) = input("handle", "a.txt", "x");
    let notes = adversary("badhandle", &p, None).result.unwrap().notes;
    assert!(
        notes[0].contains("Err") && notes[0].contains("not one of the chosen files"),
        "{notes:?}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn reads_are_ranged_and_end_cleanly() {
    let (d, p) = input("read", "a.txt", "abcdefgh");
    let notes = adversary("read", &p, None).result.unwrap().notes;
    assert_eq!(notes, vec!["abcd|efgh|0"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn read_all_refuses_files_over_the_limit_without_loading_them() {
    let d = scratch("big");
    let p = d.join("big.bin");
    let f = std::fs::File::create(&p).unwrap();
    f.set_len(panoply_wasm::MAX_READ_ALL + 1).unwrap(); // sparse, costs no disk
    let notes = adversary("bigread", &p, None).result.unwrap().notes;
    assert!(
        notes[0].contains("too big to read in one piece"),
        "{notes:?}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_module_that_wants_the_network_is_refused_before_it_runs() {
    let dir = build(&fixture("netcaller"));
    let (d, p) = input("net", "a.txt", "x");
    let err = run(&dir, &[p], json!({})).result.unwrap_err();
    assert!(
        matches!(&err, ModuleError::BadModule(m) if m.contains("the network") && m.contains("wasi:sockets")),
        "{err}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn memory_is_capped_and_the_message_says_so() {
    let dir = build(&fixture("adversary"));
    let (d, p) = input("mem", "a.txt", "x");
    let limits = Limits {
        max_memory: 16 * 1024 * 1024,
    };
    let ran = run_with(
        &dir,
        std::slice::from_ref(&p),
        json!({"mode": "hog"}),
        None,
        CancelToken::new(),
        Some(limits),
    );
    let err = ran.result.unwrap_err();
    assert!(
        matches!(&err, ModuleError::Sandbox(m) if m.contains("16 MB")),
        "{err}"
    );
    // The same module is fine under the default 256 MB.
    let ok = run(&dir, &[p], json!({"mode": "hog"})).result.unwrap();
    assert!(ok.notes[0].contains("67108864"), "{:?}", ok.notes);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_crash_comes_back_as_a_plain_error_with_what_the_module_said() {
    let (d, p) = input("panic", "a.txt", "x");
    let err = adversary("panic", &p, None).result.unwrap_err();
    assert!(
        matches!(&err, ModuleError::Sandbox(m) if m.contains("kaboom from the module")),
        "{err}"
    );
    assert!(!err.next_step().is_empty());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn cancel_stops_a_module_that_never_polls_and_leaves_no_files() {
    let dir = build(&fixture("adversary"));
    let (d, p) = input("spin", "a.txt", "x");
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        c2.cancel();
    });
    let t = Instant::now();
    let ran = run_with(
        &dir,
        &[p],
        json!({"mode": "write_then_spin"}),
        None,
        cancel,
        None,
    );
    let took = t.elapsed();
    assert!(
        matches!(ran.result, Err(ModuleError::Cancelled)),
        "{:?}",
        ran.result.err()
    );
    eprintln!("cancel took {took:?} (cancel fired at 300 ms)");
    assert!(took < Duration::from_millis(1500), "cancel took {took:?}");
    assert_eq!(
        files_in(&d),
        vec!["a.txt"],
        "partial output was left behind"
    );
    assert!(
        matches!(ran.events.last(), Some(JobEvent::Failed { plain_message, .. }) if plain_message.contains("Cancelled"))
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn cancel_is_visible_to_a_module_that_polls() {
    let dir = build(&fixture("adversary"));
    let (d, p) = input("poll", "a.txt", "x");
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c2.cancel();
    });
    let ran = run_with(&dir, &[p], json!({"mode": "poll"}), None, cancel, None);
    assert!(matches!(ran.result, Err(ModuleError::Cancelled)));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_cancel_before_the_start_never_loads_the_module() {
    let dir = build(&fixture("adversary"));
    let (d, p) = input("pre", "a.txt", "x");
    let cancel = CancelToken::new();
    cancel.cancel();
    let ran = run_with(&dir, &[p], json!({}), None, cancel, None);
    assert!(matches!(ran.result, Err(ModuleError::Cancelled)));
    std::fs::remove_dir_all(&d).unwrap();
}

// ---------------------------------------------------------------------- outputs

#[test]
fn a_failed_job_removes_what_it_wrote() {
    let (d, p) = input("fail", "a.txt", "x");
    let err = adversary("write_then_fail", &p, None).result.unwrap_err();
    assert!(matches!(&err, ModuleError::Failed(m) if m == "boom"));
    assert_eq!(files_in(&d), vec!["a.txt"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn unfinished_outputs_are_deleted_and_forgotten_ones_are_kept() {
    let (d, p) = input("unfin", "a.txt", "x");
    let out = adversary("unfinished", &p, None).result.unwrap();
    assert!(out.outputs.is_empty());
    assert_eq!(files_in(&d), vec!["a.txt"]);
    let out = adversary("unlisted", &p, None).result.unwrap();
    assert_eq!(out.outputs, vec![d.join("a (forgot).txt")]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn reporting_a_file_it_never_wrote_is_an_error() {
    let (d, p) = input("fake", "a.txt", "x");
    let err = adversary("fake", &p, None).result.unwrap_err();
    assert!(matches!(&err, ModuleError::Failed(m) if m.contains("never-written.txt")));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn streamed_outputs_land_in_the_chosen_folder_with_safe_names() {
    let (d, p) = input("stream", "a.txt", "x");
    let out_dir = scratch("stream-out");
    let ran = adversary("stream", &p, Some(&out_dir));
    let out = ran.result.unwrap();
    assert_eq!(out.outputs.len(), 1);
    assert_eq!(
        out.outputs[0].parent().unwrap(),
        out_dir.canonicalize().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(&out.outputs[0]).unwrap(),
        "chunk 0\nchunk 1\nchunk 2\nchunk 3\n"
    );
    assert_eq!(
        files_in(&d),
        vec!["a.txt"],
        "nothing is written next to the original"
    );
    let progress: Vec<f32> = ran
        .events
        .iter()
        .filter_map(|e| match e {
            JobEvent::Progress { fraction, .. } => Some(*fraction),
            _ => None,
        })
        .collect();
    assert_eq!(progress, vec![0.25, 0.5, 0.75, 1.0]);
    assert!(matches!(ran.events.last(), Some(JobEvent::Done { .. })));
    std::fs::remove_dir_all(&d).unwrap();
    std::fs::remove_dir_all(&out_dir).unwrap();
}

#[test]
fn nothing_is_resident_between_jobs() {
    // Every run builds its own engine and instance on a scoped thread that is joined before
    // `run` returns, so a finished job leaves no thread of ours behind. Back to back runs
    // of a fresh module give the same answer, which they could not if state were shared.
    let (d, p) = input("idle", "a.txt", "x");
    for n in 1..=3 {
        let out = adversary("ok", &p, None).result.unwrap();
        let want = if n == 1 {
            "a (ok).txt".to_string()
        } else {
            format!("a (ok {n}).txt")
        };
        assert_eq!(out.outputs, vec![d.join(want)]);
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn the_wit_files_in_every_module_project_match_the_host() {
    let want = panoply_wasm::MODULE_WIT;
    for rel in [
        "modules/template-rust/wit/module.wit",
        "modules/examples-wasm/slugify-lines/wit/module.wit",
        "modules/examples-wasm/word-count/wit/module.wit",
        "crates/panoply-wasm/tests/fixtures/adversary/wit/module.wit",
        "crates/panoply-wasm/tests/fixtures/netcaller/wit/module.wit",
    ] {
        let got = std::fs::read_to_string(repo().join(rel)).unwrap();
        assert_eq!(
            got, want,
            "{rel} has drifted from crates/panoply-wasm/wit/module.wit"
        );
    }
}
