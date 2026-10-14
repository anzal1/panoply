//! The PDF modules over real corpus files, through the registry like the CLI and the bench do.

use std::path::{Path, PathBuf};

use panoply_core::{CancelToken, JobSpec};
use panoply_host::Registry;
use serde_json::{Value, json};

fn corpus(file: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache/pdf")
        .join(file);
    assert!(
        p.is_file(),
        "{file} is not on disk: run `python3 -I tests/corpus/fetch.py --kind pdf`"
    );
    p
}

/// Copy the input to a scratch folder so outputs land there, not in the corpus.
fn scratch_copy(tag: &str, file: &str) -> (PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("panoply-host-corpus-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(Path::new(file).file_name().unwrap());
    std::fs::copy(corpus(file), &dest).unwrap();
    (dir, dest)
}

fn run(id: &str, input: &Path, options: Value) -> panoply_host::JobOutcome {
    let spec = JobSpec {
        module_id: id.into(),
        inputs: vec![input.to_path_buf()],
        options,
    };
    Registry::with_builtins()
        .run_job(&spec, None, CancelToken::new(), |_| {})
        .unwrap_or_else(|e| panic!("{id}: {}", e.plain_message()))
}

fn pages(p: &Path) -> usize {
    panoply_pdf::open(&std::fs::File::open(p).unwrap())
        .unwrap()
        .doc
        .pages()
        .unwrap()
        .len()
}

#[test]
fn compress_shrinks_a_photo_pdf_and_keeps_the_original_of_a_scan_it_cannot_shrink() {
    let (dir, f) = scratch_copy("compress", "qpdf/image-streams.pdf");
    let out = run("pdf.compress", &f, json!({}));
    assert_eq!(out.outputs.len(), 1);
    let (before, after) = (
        std::fs::metadata(&f).unwrap().len(),
        std::fs::metadata(&out.outputs[0]).unwrap().len(),
    );
    assert!(after * 10 < before * 8, "{before} -> {after}");
    assert_eq!(pages(&out.outputs[0]), pages(&f));
    assert!(
        out.notes.iter().any(|n| n.contains("smaller")),
        "{:?}",
        out.notes
    );
    std::fs::remove_dir_all(&dir).unwrap();

    // JPEG 2000 and JBIG2 scans: pictures are decoded through pdfium and recoded,
    // so the scan shrinks by more than half with the page count unchanged.
    let (dir, f) = scratch_copy("compress-scan", "commons/ia-scan-mining-claims.pdf");
    let out = run("pdf.compress", &f, json!({}));
    assert_eq!(out.outputs.len(), 1, "{:?}", out.notes);
    let (before, after) = (
        std::fs::metadata(&f).unwrap().len(),
        std::fs::metadata(&out.outputs[0]).unwrap().len(),
    );
    assert!(after * 2 < before, "{before} -> {after}");
    assert_eq!(pages(&out.outputs[0]), pages(&f));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn page_modules_agree_on_page_counts_on_a_long_tagged_document() {
    let (dir, f) = scratch_copy("pages", "gov/nist-sp800-53r5.pdf");
    let n = pages(&f);
    assert!(n > 400);
    let reversed = run("pdf.organize", &f, json!({"reverse": true}));
    assert_eq!(pages(&reversed.outputs[0]), n);
    let extract = run("pdf.organize", &f, json!({"keep": "10-19"}));
    assert_eq!(pages(&extract.outputs[0]), 10);
    let parts = run("pdf.split", &f, json!({"every": 100}));
    let counts: Vec<usize> = parts.outputs.iter().map(|p| pages(p)).collect();
    assert_eq!(counts.iter().sum::<usize>(), n);
    assert_eq!(counts.len(), n.div_ceil(100));
    let merged = run("pdf.merge", &f, json!({}));
    assert_eq!(pages(&merged.outputs[0]), n);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn encrypted_and_broken_files_get_plain_errors() {
    for (file, want) in [
        ("qpdf/enc-r3-rc4-128.pdf", "password-protected"),
        ("qpdf/not-a-pdf.pdf", "not a PDF"),
    ] {
        let (dir, f) = scratch_copy("errs", file);
        for id in ["pdf.merge", "pdf.compress", "pdf.split"] {
            let spec = JobSpec {
                module_id: id.into(),
                inputs: vec![f.clone()],
                options: Value::Null,
            };
            let err = Registry::with_builtins()
                .run_job(&spec, None, CancelToken::new(), |_| {})
                .err()
                .unwrap();
            assert!(
                err.plain_message().contains(want),
                "{id} {file}: {}",
                err.plain_message()
            );
        }
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no leftovers for {file}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
