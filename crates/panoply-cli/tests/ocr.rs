//! End to end checks of `panoply image ocr` and `panoply pdf ocr`. The text goes to standard
//! output and the result card to standard error, so the text pipes. A test skips, with a
//! message, when an engine file cannot be downloaded (no network).

use std::path::PathBuf;
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

fn picture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../panoply-ocr/testdata/labelled/en-1.png")
}

fn offline(stderr: &str) -> bool {
    // Only the failures that mean "the engine files are not here and could not be had" (no
    // network, or nobody allowed the download), never the network notice every run prints.
    stderr.contains("could not download") || stderr.contains("has not been allowed")
}

#[test]
fn image_ocr_prints_text_on_stdout_and_the_card_on_stderr() {
    let (out, err, ok) = panoply(&["image", "ocr", picture().to_str().unwrap(), "--lang", "en"]);
    if !ok && offline(&err) {
        eprintln!("skipped: {err}");
        return;
    }
    assert!(ok, "{err}");
    assert!(
        out.contains("quick brown fox jumps over the lazy dog"),
        "{out}"
    );
    assert!(
        !out.contains("mean confidence"),
        "the card is not on stdout: {out}"
    );
    assert!(
        err.contains("words") && err.contains("mean confidence"),
        "{err}"
    );
}

#[test]
fn pdf_ocr_writes_a_searchable_copy_and_reports_it() {
    let dir = std::env::temp_dir().join(format!("panoply-cli-ocr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pic = dir.join("page.png");
    std::fs::copy(picture(), &pic).unwrap();
    let (_, err, ok) = panoply(&["pdf", "from", pic.to_str().unwrap()]);
    assert!(ok, "{err}");
    let scan = dir.join("page.pdf");
    assert!(scan.is_file());
    let (out, err, ok) = panoply(&[
        "pdf",
        "ocr",
        scan.to_str().unwrap(),
        "--lang",
        "en",
        "--text-file",
    ]);
    if !ok && offline(&err) {
        eprintln!("skipped: {err}");
        return;
    }
    assert!(ok, "{err}");
    assert!(out.contains("1 page, English"), "{out}");
    assert!(dir.join("page (searchable).pdf").is_file());
    assert!(
        std::fs::read_to_string(dir.join("page.txt"))
            .unwrap()
            .contains("lazy dog")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_ocr_modules_are_listed_and_have_options() {
    let (out, _, ok) = panoply(&["modules"]);
    assert!(ok);
    assert!(
        out.contains("image.to_text") && out.contains("pdf.ocr"),
        "{out}"
    );
    let (_, err, ok) = panoply(&["run", "pdf.ocr", "x.pdf", "--opt", "bogus=1"]);
    assert!(!ok);
    assert!(err.contains("languages"), "{err}");
}
