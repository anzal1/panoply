//! Download consent on the command line: nothing is downloaded without a yes, the error says
//! how to give one, `--yes` and `models allow` record it per item, and `models remove` forgets
//! it. Every run here gets its own data folder and engines folder, and never inherits
//! `PANOPLY_YES`, so no test touches the real ones or the network.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Sandbox {
    root: PathBuf,
}

struct Run {
    out: String,
    err: String,
    ok: bool,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("panoply-cli-consent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::create_dir_all(root.join("engines")).unwrap();
        Self { root }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }

    fn consent(&self) -> PathBuf {
        self.data().join("consent.json")
    }

    /// Run `panoply` with no terminal on stdin (so it cannot ask) and a sandboxed environment.
    fn run(&self, args: &[&str]) -> Run {
        let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
            .args(args)
            .stdin(Stdio::null())
            .env("PANOPLY_DATA_DIR", self.data())
            .env("PANOPLY_ENGINES_DIR", self.engines())
            .env_remove("PANOPLY_YES")
            .env_remove("PANOPLY_PDFIUM_LIB")
            .env_remove("PANOPLY_ORT_LIB")
            .env_remove("PANOPLY_FFMPEG")
            .env_remove("PANOPLY_MODEL_ISNET")
            .env_remove("PANOPLY_MODEL_MIGAN")
            .env_remove("PANOPLY_MODEL_ESRGAN")
            .env_remove("PANOPLY_YUNET_MODEL")
            // A download that is allowed must still fail here, quickly and offline.
            .env("https_proxy", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("all_proxy", "http://127.0.0.1:9")
            .output()
            .unwrap();
        Run {
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
            ok: out.status.success(),
        }
    }

    /// Files and folders under the engines folder, to prove nothing was written there.
    fn engine_files(&self) -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                out.push(e.path().display().to_string());
                if e.path().is_dir() {
                    walk(&e.path(), out);
                }
            }
        }
        let mut v = Vec::new();
        walk(&self.engines(), &mut v);
        v
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The ids `panoply models` lists on this platform.
fn listed_ids(sb: &Sandbox) -> Vec<String> {
    let r = sb.run(&["models"]);
    assert!(r.ok, "{}", r.err);
    r.out
        .lines()
        .skip_while(|l| !l.starts_with("id "))
        .skip(1)
        .filter(|l| !l.starts_with(' ') && !l.trim().is_empty() && !l.starts_with("On macOS"))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

#[test]
fn models_lists_every_item_with_size_license_and_state_and_changes_nothing() {
    let sb = Sandbox::new("list");
    let r = sb.run(&["models"]);
    assert!(r.ok, "{}", r.err);
    for want in [
        "pdfium",
        "onnxruntime",
        "isnet",
        "migan",
        "esrgan",
        "ppocr-det",
        "MIT",
        "not downloaded",
    ] {
        assert!(r.out.contains(want), "{want} in {}", r.out);
    }
    assert!(r.out.contains("only after you allow each one"), "{}", r.out);
    assert!(!sb.consent().exists(), "listing records nothing");
    assert!(sb.engine_files().is_empty(), "listing downloads nothing");
}

#[test]
fn without_a_terminal_and_without_yes_a_download_stops_with_a_plain_error() {
    let sb = Sandbox::new("noyes");
    let r = sb.run(&["models", "fetch", "isnet"]);
    assert!(!r.ok, "must fail: {}", r.out);
    assert!(r.err.contains("has not been allowed"), "{}", r.err);
    assert!(r.err.contains("--yes"), "says how to allow it: {}", r.err);
    assert!(r.err.contains("panoply models allow"), "{}", r.err);
    assert!(
        r.err.contains("44 MB") && r.err.contains("MIT"),
        "says what it is: {}",
        r.err
    );
    assert!(!sb.consent().exists(), "a refusal records nothing");
    assert!(
        sb.engine_files().is_empty(),
        "a refusal downloads nothing: {:?}",
        sb.engine_files()
    );
}

#[test]
fn every_listed_item_is_refused_without_a_yes() {
    let sb = Sandbox::new("each");
    let ids = listed_ids(&sb);
    assert!(ids.len() >= 8, "{ids:?}");
    for id in &ids {
        let r = sb.run(&["models", "fetch", id]);
        assert!(!r.ok, "{id} must be refused");
        assert!(r.err.contains("has not been allowed"), "{id}: {}", r.err);
    }
    assert!(!sb.consent().exists());
    assert!(sb.engine_files().is_empty(), "{:?}", sb.engine_files());
}

#[test]
fn a_real_job_is_refused_the_same_way_and_leaves_the_picture_alone() {
    let sb = Sandbox::new("job");
    let pic = sb.root.join("page.png");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../panoply-ocr/testdata/labelled/en-1.png"),
        &pic,
    )
    .unwrap();
    let before = std::fs::read_dir(&sb.root).unwrap().count();
    let r = sb.run(&["cutout", pic.to_str().unwrap(), "--engine", "model"]);
    assert!(!r.ok, "{}", r.out);
    assert!(r.err.contains("has not been allowed"), "{}", r.err);
    assert!(r.err.contains("--yes"), "{}", r.err);
    assert!(!sb.consent().exists());
    assert!(sb.engine_files().is_empty());
    assert_eq!(
        std::fs::read_dir(&sb.root).unwrap().count(),
        before,
        "no output file"
    );
}

#[test]
fn yes_allows_the_download_and_records_it_for_that_item_only() {
    let sb = Sandbox::new("yes");
    // The proxy points nowhere, so the transfer itself fails; the point is that the gate let
    // it through and wrote down the yes for this item, and only this one.
    let r = sb.run(&["--yes", "models", "fetch", "yunet"]);
    assert!(!r.err.contains("has not been allowed"), "{}", r.err);
    let saved = std::fs::read_to_string(sb.consent()).expect("the yes is recorded");
    assert!(saved.contains("yunet"), "{saved}");
    assert!(!saved.contains("isnet"), "{saved}");
    // Later, without --yes, that item no longer asks, and another one still does.
    let again = sb.run(&["models", "fetch", "yunet"]);
    assert!(!again.err.contains("has not been allowed"), "{}", again.err);
    let other = sb.run(&["models", "fetch", "isnet"]);
    assert!(other.err.contains("has not been allowed"), "{}", other.err);
}

#[test]
fn models_allow_records_without_downloading_and_remove_forgets() {
    let sb = Sandbox::new("allow");
    let r = sb.run(&["models", "allow", "isnet"]);
    assert!(r.ok, "{}", r.err);
    assert!(
        r.out.contains("isnet: allowed") && r.out.contains("44 MB"),
        "{}",
        r.out
    );
    assert!(
        std::fs::read_to_string(sb.consent())
            .unwrap()
            .contains("isnet")
    );
    assert!(sb.engine_files().is_empty(), "allow downloads nothing");
    let listed = sb.run(&["models"]);
    assert!(
        listed.out.contains("allowed, not downloaded yet"),
        "{}",
        listed.out
    );

    let gone = sb.run(&["models", "remove", "isnet"]);
    assert!(gone.ok, "{}", gone.err);
    assert!(gone.out.contains("nothing to remove"), "{}", gone.out);
    assert!(
        !std::fs::read_to_string(sb.consent())
            .unwrap()
            .contains("isnet")
    );
    let fetch = sb.run(&["models", "fetch", "isnet"]);
    assert!(
        fetch.err.contains("has not been allowed"),
        "asks again: {}",
        fetch.err
    );
}

#[test]
fn remove_deletes_the_files_reports_the_space_freed_and_forgets_the_yes() {
    let sb = Sandbox::new("remove");
    // Put the face model where the engine keeps it, the way a finished download would, and
    // a neighbour that must survive.
    let models = sb.engines().join("models");
    std::fs::create_dir_all(&models).unwrap();
    let file = models.join("face_detection_yunet_2023mar.onnx");
    let neighbour = models.join("someone-elses.onnx");
    std::fs::write(&file, vec![1u8; 2048]).unwrap();
    std::fs::write(&neighbour, b"keep").unwrap();
    assert!(sb.run(&["models", "allow", "yunet"]).ok);

    let listed = sb.run(&["models"]);
    assert!(
        listed.out.contains("on this computer, 2 KB"),
        "{}",
        listed.out
    );

    let r = sb.run(&["models", "remove", "yunet"]);
    assert!(r.ok, "{}", r.err);
    assert!(r.out.contains("yunet: removed, 2 KB freed"), "{}", r.out);
    assert!(!file.exists(), "its file is gone");
    assert!(neighbour.exists(), "nothing else is touched");
    assert!(
        !std::fs::read_to_string(sb.consent())
            .unwrap()
            .contains("yunet")
    );
    let after = sb.run(&["models"]);
    assert!(!after.out.contains("on this computer"), "{}", after.out);
}

#[test]
fn an_unknown_item_is_a_plain_error_listing_the_real_ones() {
    let sb = Sandbox::new("unknown");
    let r = sb.run(&["models", "fetch", "gpt-9"]);
    assert!(!r.ok);
    assert!(
        r.err.contains("not something Panoply downloads") && r.err.contains("isnet"),
        "{}",
        r.err
    );
    let r = sb.run(&["models", "allow", "gpt-9"]);
    assert!(!r.ok && !sb.consent().exists());
}

#[test]
fn a_module_that_downloads_says_so_before_it_runs() {
    let sb = Sandbox::new("notice");
    let pic = sb.root.join("page.png");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../panoply-ocr/testdata/labelled/en-1.png"),
        &pic,
    )
    .unwrap();
    let r = sb.run(&["cutout", pic.to_str().unwrap(), "--engine", "model"]);
    assert!(r.err.contains("Uses the network:"), "{}", r.err);
}
