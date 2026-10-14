//! Shared helpers for the integration tests.
#![allow(dead_code)]

use std::fs::File;
use std::path::PathBuf;

use panoply_core::CancelToken;
use panoply_ocr::searchable::{Options, Report, make_searchable};
use panoply_ocr::{Fetch, Lang, Ocr, OcrImage};

pub fn labelled(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/labelled/{id}"))
}

pub fn label(id: &str) -> String {
    std::fs::read_to_string(labelled(&format!("{id}.txt"))).expect("label file")
}

pub fn image(id: &str) -> OcrImage {
    panoply_ocr::input::load_image(
        File::open(labelled(&format!("{id}.png"))).expect("image"),
        Some("png"),
    )
    .expect("decodes")
}

pub fn langs(s: &str) -> Vec<Lang> {
    Lang::parse_list(s).expect("languages")
}

/// Run `f` with a downloading `Fetch`. A failure to set up an engine because there is no network
/// (models not yet downloaded) is reported and returns `None`, so offline runs skip instead of
/// failing.
pub fn with_fetch<T>(f: impl FnOnce(&mut Fetch<'_>) -> panoply_ocr::Result<T>) -> Option<T> {
    let mut notice = |s: &str| eprintln!("{s}");
    let mut fetch = Fetch::new(&mut notice);
    match f(&mut fetch) {
        Ok(v) => Some(v),
        Err(panoply_ocr::OcrError::Engine(m))
            if m.contains("download") || m.contains("network") =>
        {
            eprintln!("skipped, engine files unavailable: {m}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

pub fn read(ocr: &Ocr, id: &str, langs_s: &str) -> Option<panoply_ocr::OcrPage> {
    let img = image(id);
    with_fetch(|f| ocr.recognize(&img, &langs(langs_s), f, &CancelToken::new()))
}

/// A one page PDF holding the picture, written to a temp file.
pub fn picture_pdf(png: &PathBuf, tag: &str) -> (PathBuf, File) {
    let path =
        std::env::temp_dir().join(format!("panoply-ocr-test-{tag}-{}.pdf", std::process::id()));
    let out = File::create(&path).unwrap();
    let bytes = std::fs::read(png).unwrap();
    panoply_pdf::images_in::images_to_pdf(
        std::io::BufWriter::new(out),
        1,
        &mut |_| Ok(("scan.png".into(), bytes.clone())),
        &panoply_pdf::images_in::ImageOptions {
            page: panoply_pdf::images_in::PageSize::Image,
            margin_mm: 0.0,
        },
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    (path.clone(), File::open(path).unwrap())
}

pub fn run_searchable(file: &File, ocr: &Ocr, langs_s: &str) -> Option<(Report, Vec<u8>)> {
    let mut out = Vec::new();
    let opts = Options {
        langs: langs(langs_s),
        collect_text: true,
        ..Options::default()
    };
    let report = with_fetch(|f| {
        make_searchable(
            file,
            &mut out,
            &opts,
            ocr,
            f,
            &CancelToken::new(),
            &mut |_, _| {},
        )
    })?;
    Some((report, out))
}
