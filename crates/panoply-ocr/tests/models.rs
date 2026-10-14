//! Pinned downloads: a file that fails its checksum is never used, and a missing one without
//! network permission says what to do. The checks run in a child process of this test binary
//! with the engines folder redirected, because the folder comes from the environment and the
//! workspace forbids the `unsafe` that setting it in-process needs.

use std::process::Command;

use panoply_ocr::models::{self, Fetch};

const CHILD: &str = "PANOPLY_OCR_MODELS_TEST_CHILD";

fn run_child(name: &str, dir: &std::path::Path, extra: Option<(&str, std::path::PathBuf)>) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", name, "--nocapture"])
        .env(CHILD, "1")
        .env("PANOPLY_ENGINES_DIR", dir)
        .env_remove("PANOPLY_ORT_LIB");
    if let Some((k, v)) = extra {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_bad_checksum_is_thrown_away_and_offline_means_a_plain_error() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("panoply-ocr-models-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    run_child("child_checksum", &dir, None);
    run_child(
        "child_runtime_path",
        &dir,
        Some(("PANOPLY_ORT_LIB", dir.join("nope.dylib"))),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn child_checksum() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let cache = models::cache_dir();
    assert!(cache.starts_with(std::env::var_os("PANOPLY_ENGINES_DIR").unwrap()));
    std::fs::create_dir_all(&cache).unwrap();
    // A file with the right name and the wrong content.
    let path = cache.join(models::DET.file);
    std::fs::write(&path, b"not the model").unwrap();
    let mut said = Vec::new();
    let mut notice = |s: &str| said.push(s.to_string());
    let mut fetch = Fetch::new(&mut notice);
    fetch.allow_download = false;
    let e = models::ensure(&models::DET, &mut fetch)
        .unwrap_err()
        .to_string();
    assert!(e.contains("may not use the network"), "{e}");
    assert!(
        !path.exists(),
        "a file that fails its checksum must not be kept"
    );
    // The runtime is not there either, and asking again offline is a plain error.
    let e = models::ensure_ort(&mut fetch).unwrap_err().to_string();
    assert!(
        e.contains("may not use the network") || e.contains("no prebuilt"),
        "{e}"
    );
    assert!(
        said.is_empty(),
        "nothing is announced for a download that does not happen"
    );
}

#[test]
fn child_runtime_path() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let mut notice = |_: &str| {};
    let mut fetch = Fetch::new(&mut notice);
    let e = models::ensure_ort(&mut fetch).unwrap_err().to_string();
    assert!(e.contains("PANOPLY_ORT_LIB"), "{e}");
}
