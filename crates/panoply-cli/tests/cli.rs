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
