//! Compress over the corpus: every output must open, keep its pages, and look like the input.

mod common;

use std::fs::File;
use std::io::BufWriter;

use panoply_core::CancelToken;
use panoply_pdf::compress::{self, Options, Target};
use panoply_pdf::render::{RenderDoc, compare};
use panoply_pdf::{PdfError, open};

struct Row {
    id: String,
    before: u64,
    after: u64,
    min_ssim: Option<f64>,
    render_min: Option<f64>,
    images: usize,
    ms: u128,
}

fn run_one(
    e: &common::Entry,
    s: &common::Scratch,
    opts: &Options,
) -> Result<(compress::Report, std::path::PathBuf, u128), PdfError> {
    let opened = open(&File::open(&e.path).unwrap())?;
    opened.doc.require_unencrypted()?;
    let pages = opened.doc.pages()?;
    let out = s.path(&format!("{}.pdf", e.id));
    let t = std::time::Instant::now();
    let rep = compress::compress(
        &opened.doc,
        &pages,
        opts,
        &CancelToken::new(),
        &mut |_, _| {},
        Box::new(BufWriter::new(File::create(&out).unwrap())),
    )?;
    Ok((rep, out, t.elapsed().as_millis()))
}

#[test]
fn smallest_over_the_corpus_opens_everywhere_and_keeps_the_look() {
    let s = common::Scratch::new("compress");
    let mut rows: Vec<Row> = Vec::new();
    for e in common::corpus() {
        if e.tier != "default" {
            continue;
        }
        let (rep, out, ms) = match run_one(&e, &s, &Options::default()) {
            Ok(x) => x,
            Err(PdfError::Encrypted) => {
                assert!(e.has("encrypted") || e.has("password"), "{}", e.id);
                continue;
            }
            Err(PdfError::NotPdf) => {
                assert!(e.has("tiny"), "{}", e.id);
                continue;
            }
            Err(err) => panic!("{}: {err}", e.id),
        };
        // pdfium opens the result, with as many pages as the engine copied.
        let copy = RenderDoc::open(File::open(&out).unwrap(), None)
            .unwrap_or_else(|err| panic!("{}: pdfium: {err}", e.id));
        let ours = open(&File::open(&out).unwrap()).unwrap();
        assert_eq!(
            copy.page_count(),
            ours.doc.pages().unwrap().len(),
            "{}",
            e.id
        );
        let mut render_min = None;
        if let Ok(orig) = RenderDoc::open(File::open(&e.path).unwrap(), None)
            && orig.page_count() == copy.page_count()
        {
            let l = compare(&orig, &copy, 100.0, 4).unwrap();
            render_min = Some(l.min);
        }
        rows.push(Row {
            id: e.id.clone(),
            before: rep.original_bytes,
            after: rep.output_bytes,
            min_ssim: rep.min_image_ssim,
            render_min,
            images: rep.images,
            ms,
        });
    }
    eprintln!(
        "{:<36}{:>10}{:>10}{:>8}{:>8}{:>9}{:>7}",
        "file", "before", "after", "saved", "images", "min ssim", "ms"
    );
    for r in &rows {
        eprintln!(
            "{:<36}{:>10}{:>10}{:>7.1}%{:>8}{:>9}{:>9}{:>7}",
            r.id,
            r.before,
            r.after,
            (1.0 - r.after as f64 / r.before as f64) * 100.0,
            r.images,
            r.min_ssim.map_or("-".into(), |v| format!("{v:.3}")),
            r.render_min.map_or("-".into(), |v| format!("{v:.3}")),
            r.ms
        );
    }
    for r in &rows {
        if let Some(v) = r.render_min {
            assert!(v > 0.95, "{}: page look dropped to {v}", r.id);
        }
    }
    assert!(rows.len() >= 30);
    let _ = Target::Smallest;
}

/// Scans stored as JPEG 2000 pictures under JBIG2 stencil masks used to gain under 2%. pdfium
/// decodes both, so now the colour layers are recoded and the masks are kept.
#[test]
fn jpeg_2000_and_jbig2_scans_shrink_and_keep_the_look() {
    let s = common::Scratch::new("compress-jpx");
    // id, least saving that must be reached, lowest page score accepted (100 dpi, pdfium).
    let want = [
        ("pdf-commons-ia-scan-public-lands", 0.40, 0.98),
        ("pdf-commons-ia-scan-mining-claims", 0.35, 0.98),
        ("pdf-commons-arabic-primer-wdl", 0.04, 0.97),
    ];
    let mut checked = 0;
    for e in common::corpus() {
        let Some(&(_, least, floor)) = want.iter().find(|w| w.0 == e.id) else {
            continue;
        };
        let (rep, out, _) = run_one(&e, &s, &Options::default()).unwrap();
        if rep.images_kept.contains_key("NoEngine") {
            eprintln!("{}: pdfium is not available here, skipped", e.id);
            continue;
        }
        assert!(
            rep.saving() >= least,
            "{}: saved {:.1}%, wanted {:.0}%",
            e.id,
            rep.saving() * 100.0,
            least * 100.0
        );
        assert!(rep.images_lossy > 0, "{}", e.id);
        let copy = RenderDoc::open(File::open(&out).unwrap(), None).unwrap();
        let orig = RenderDoc::open(File::open(&e.path).unwrap(), None).unwrap();
        let l = compare(&orig, &copy, 100.0, 6).unwrap();
        assert!(l.min >= floor, "{}: page look dropped to {}", e.id, l.min);
        checked += 1;
    }
    eprintln!("{checked} scans checked");
}
