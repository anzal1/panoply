//! Page operations over the whole PDF corpus, checked with pdfium as an independent reader.

mod common;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use panoply_core::CancelToken;
use panoply_pdf::pageops::{BuildOptions, CatalogMode, Input, OutPage, build};
use panoply_pdf::render::{RenderDoc, compare};
use panoply_pdf::{PdfError, open};

fn build_file(
    path: &Path,
    out: &Path,
    pick: impl Fn(usize) -> Vec<OutPage>,
    mode: CatalogMode,
) -> Result<(), PdfError> {
    let opened = open(&File::open(path).unwrap())?;
    opened.doc.require_unencrypted()?;
    let pages = opened.doc.pages()?;
    let plan = pick(pages.len());
    let inputs = [Input {
        doc: &opened.doc,
        pages: pages.into(),
        title: "t".into(),
    }];
    let w = BufWriter::new(File::create(out).unwrap());
    build(
        w,
        &inputs,
        &plan,
        &BuildOptions {
            mode,
            ..Default::default()
        },
        &CancelToken::new(),
        &mut |_, _| {},
    )?;
    Ok(())
}

fn all(n: usize) -> Vec<OutPage> {
    (0..n)
        .map(|i| OutPage {
            input: 0,
            index: i,
            rotate: 0,
        })
        .collect()
}

#[test]
fn identity_copies_open_in_pdfium_and_look_the_same() {
    let s = common::Scratch::new("identity");
    let mut checked = 0;
    for e in common::corpus() {
        if e.tier != "default" {
            continue;
        }
        let out = s.path(&format!("{}.pdf", e.id));
        match build_file(&e.path, &out, all, CatalogMode::Full) {
            Ok(()) => {}
            Err(PdfError::Encrypted) => {
                assert!(
                    e.has("encrypted") || e.has("password"),
                    "{} refused as encrypted",
                    e.id
                );
                continue;
            }
            Err(PdfError::NotPdf) => {
                assert!(e.has("tiny"), "{} refused as not a PDF", e.id);
                continue;
            }
            Err(err) => panic!("{}: {err}", e.id),
        }
        // Our own reader and pdfium agree on the page count; compare drawn pages against the original.
        let ours = open(&File::open(&out).unwrap()).unwrap();
        let n = ours.doc.pages().unwrap().len();
        let was_repaired = open(&File::open(&e.path).unwrap()).unwrap().repaired;
        eprintln!(
            "{:<34} pages {:>5}  in {:>9}  out {:>9}{}",
            e.id,
            n,
            std::fs::metadata(&e.path).unwrap().len(),
            std::fs::metadata(&out).unwrap().len(),
            if was_repaired { "  (repaired)" } else { "" }
        );
        let Ok(copy) = RenderDoc::open(File::open(&out).unwrap(), None) else {
            panic!("{}: pdfium cannot open our output", e.id);
        };
        assert_eq!(copy.page_count(), n, "{}", e.id);
        if let Ok(orig) = RenderDoc::open(File::open(&e.path).unwrap(), e.password.as_deref()) {
            if orig.page_count() == n {
                let l = compare(&orig, &copy, 40.0, 3).unwrap();
                assert!(l.min > 0.985, "{}: {l:?}", e.id);
            } else {
                eprintln!(
                    "{}: pdfium sees {} pages in the original, we kept {n}",
                    e.id,
                    orig.page_count()
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 30, "only {checked} files went through");
}

#[test]
fn first_page_only_and_rotation_on_every_openable_file() {
    let s = common::Scratch::new("subset");
    for e in common::corpus() {
        if e.tier != "default" || e.has("encrypted") || e.has("tiny") {
            continue;
        }
        let out = s.path(&format!("{}-one.pdf", e.id));
        build_file(
            &e.path,
            &out,
            |_| {
                vec![OutPage {
                    input: 0,
                    index: 0,
                    rotate: 90,
                }]
            },
            CatalogMode::Subset,
        )
        .unwrap_or_else(|err| panic!("{}: {err}", e.id));
        let copy = RenderDoc::open(File::open(&out).unwrap(), None)
            .unwrap_or_else(|err| panic!("{}: pdfium: {err}", e.id));
        assert_eq!(copy.page_count(), 1, "{}", e.id);
        // Rotated a quarter turn: sizes swap relative to the original first page (via our reader).
        let orig = open(&File::open(&e.path).unwrap()).unwrap();
        let g = orig.doc.page_geom(&orig.doc.pages().unwrap()[0]);
        let (w, h) = g.display_size();
        let (cw, ch) = copy.page_size(0).unwrap();
        assert!(
            (f64::from(cw) - h).abs() < 1.5 && (f64::from(ch) - w).abs() < 1.5,
            "{}: {cw}x{ch} vs {w}x{h}",
            e.id
        );
    }
}
