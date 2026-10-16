//! End to end checks of `panoply disk` on a small temp tree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn panoply(args: &[&str]) -> (String, String, bool) {
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

/// A temp tree: a/ holds the two biggest files, b/ a small one, and c.bin sits at the top.
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("panoply-cli-disk-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/big.bin"), vec![1u8; 400_000]).unwrap();
    fs::write(root.join("a/mid.bin"), vec![2u8; 150_000]).unwrap();
    fs::write(root.join("b/small.bin"), vec![3u8; 40_000]).unwrap();
    fs::write(root.join("c.bin"), vec![4u8; 60_000]).unwrap();
    root
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn lists_the_biggest_entries_and_files() {
    let root = tree("human");
    let (out, _, ok) = panoply(&["disk", s(&root)]);
    assert!(ok, "{out}");
    assert!(out.contains("on disk"), "{out}");
    assert!(
        out.contains("4 files") && out.contains("3 folders"),
        "{out}"
    );
    assert!(out.contains("Largest entries at the top level"), "{out}");
    let a = out.find("a/\n").expect("a/ listed");
    let c = out.find("c.bin\n").expect("c.bin listed");
    let b = out.find("b/\n").expect("b/ listed");
    assert!(a < c && c < b, "biggest first: {out}");
    let files = out
        .split("Largest files anywhere")
        .nth(1)
        .expect("files section");
    assert!(
        files.find("big.bin").unwrap() < files.find("mid.bin").unwrap(),
        "{files}"
    );
    assert!(!out.contains('\u{2014}'));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn top_files_depth_and_json_are_honoured() {
    let root = tree("json");
    let (out, _, ok) = panoply(&[
        "disk",
        s(&root),
        "--json",
        "--top",
        "2",
        "--files",
        "1",
        "--depth",
        "2",
    ]);
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert!(v["total_bytes"].as_u64().unwrap() >= 650_000);
    assert_eq!(v["files"], 4);
    assert_eq!(v["folders"], 3);
    let top = v["top"].as_array().unwrap();
    assert_eq!(top.len(), 2, "--top 2");
    assert_eq!(top[0]["name"], "a");
    assert_eq!(top[0]["kind"], "folder");
    let kids = top[0]["children"]
        .as_array()
        .expect("--depth 2 lists children");
    assert_eq!(kids[0]["name"], "big.bin");
    let files = v["largest_files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "--files 1");
    assert!(files[0]["path"].as_str().unwrap().ends_with("big.bin"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_missing_folder_is_a_plain_error() {
    let (out, err, ok) = panoply(&["disk", "/this/folder/does/not/exist"]);
    assert!(!ok);
    assert!(out.is_empty());
    assert!(err.contains("Could not scan"), "{err}");
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let root = tree("locked");
    let locked = root.join("b");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&locked).is_ok(); // running as root, nothing to test
    let (out, _, ok) = panoply(&["disk", s(&root)]);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ok, "{out}");
    if !readable {
        assert!(out.contains("1 folder could not be read"), "{out}");
        assert!(
            out.contains("no permission") || out.contains("Full Disk Access"),
            "{out}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_hard_link_is_counted_once_and_the_line_says_so() {
    let root = tree("link");
    fs::hard_link(root.join("a/big.bin"), root.join("big-link.bin")).unwrap();
    let (out, _, ok) = panoply(&["disk", s(&root)]);
    assert!(ok, "{out}");
    assert!(
        out.contains("Counted once, not twice: 1 extra name"),
        "{out}"
    );
    let (json, _, _) = panoply(&["disk", s(&root), "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["counted_once"]["hard_link_entries"], 1);
    assert!(v["counted_once"]["hard_link_bytes_saved"].as_u64().unwrap() >= 400_000);
    fs::remove_dir_all(root).unwrap();
}
