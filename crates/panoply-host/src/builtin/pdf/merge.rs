//! `pdf.merge`: put several PDFs in one file, or interleave front and back scans.

use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::pageops::{BuildOptions, CatalogMode, Input as PdfInput, OutPage, build};

use super::{Opts, map_err, note_input, open_pdf, ordered, plural, size_text, stem};
use crate::{ModuleError, Outputs, RunContext};

pub(super) fn run(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let order = ordered(inputs, opts.str("order"));
    let many = order.len() > 1;
    let mut docs = Vec::with_capacity(order.len());
    for (i, p) in order.iter().enumerate() {
        ctx.check_cancel()?;
        ctx.progress(0.1 * i as f32 / order.len() as f32, "Reading the files");
        let d = open_pdf(ctx, p, many)?;
        note_input(ctx, &d, many);
        docs.push(d);
    }
    let page_lists: Vec<Vec<panoply_pdf::doc::PageRef>> = docs
        .iter()
        .map(|d| d.opened.doc.pages())
        .collect::<Result<_, _>>()
        .map_err(map_err)?;

    // The order of the result.
    let mut plan: Vec<OutPage> = Vec::new();
    let interleave = opts.bool("interleave");
    if interleave {
        if docs.len() != 2 {
            return Err(ModuleError::Failed(
                "Interleave works on exactly two files: the front scans and the back scans.".into(),
            ));
        }
        let (a, b) = (page_lists[0].len(), page_lists[1].len());
        let second: Vec<usize> = if opts.bool("reverse_second") {
            (0..b).rev().collect()
        } else {
            (0..b).collect()
        };
        for k in 0..a.max(b) {
            if k < a {
                plan.push(OutPage {
                    input: 0,
                    index: k,
                    rotate: 0,
                });
            }
            if let Some(&index) = second.get(k) {
                plan.push(OutPage {
                    input: 1,
                    index,
                    rotate: 0,
                });
            }
        }
        if a != b {
            ctx.note(format!(
                "The files have {a} and {b} pages, so the extra pages of the longer file stand alone at the end."
            ));
        }
    } else {
        for (k, pages) in page_lists.iter().enumerate() {
            plan.extend((0..pages.len()).map(|index| OutPage {
                input: k,
                index,
                rotate: 0,
            }));
        }
    }

    let pdf_inputs: Vec<PdfInput> = docs
        .iter()
        .zip(&page_lists)
        .map(|(d, pages)| PdfInput {
            doc: &d.opened.doc,
            pages: pages.as_slice().into(),
            title: stem(&d.path),
        })
        .collect();
    let mut out = Outputs::new(&ctx.guard);
    let mut pending = out.file(&order[0], "merged", "pdf")?;
    let report = build(
        BufWriter::new(pending.file()),
        &pdf_inputs,
        &plan,
        &BuildOptions {
            mode: CatalogMode::Subset,
            bookmarks: opts.bool("bookmarks") && !interleave,
            ..Default::default()
        },
        &ctx.cancel,
        &mut |f, label| ctx.progress(0.1 + 0.9 * f, label),
    )
    .map_err(map_err)?;
    let path = out.commit(pending)?;
    let mut line = format!(
        "{}, {}, {}.",
        plural(docs.len(), "file", "files"),
        plural(report.pages, "page", "pages"),
        size_text(report.bytes)
    );
    if report.bookmarks > 0 {
        line.push_str(&format!(
            " {} kept.",
            plural(report.bookmarks, "bookmark", "bookmarks")
        ));
    }
    if report.fields_kept > 0 {
        line.push_str(&format!(
            " {} kept",
            plural(report.fields_kept, "form field", "form fields")
        ));
        if report.fields_renamed > 0 {
            line.push_str(&format!(
                ", {} renamed to avoid a clash",
                report.fields_renamed
            ));
        }
        line.push('.');
    }
    ctx.note(line);
    let _ = path;
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    fn write(dir: &std::path::Path, name: &str, pages: usize) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, pdf(pages)).unwrap();
        p
    }

    fn page_count(p: &std::path::Path) -> usize {
        let o = panoply_pdf::open(&std::fs::File::open(p).unwrap()).unwrap();
        o.doc.pages().unwrap().len()
    }

    #[test]
    fn merges_in_natural_order_and_names_the_result_after_the_first_file() {
        let d = scratch("merge");
        let b = write(&d, "part10.pdf", 2);
        let a = write(&d, "part2.pdf", 3);
        let (out, notes) = run("pdf.merge", &[b, a], json!({})).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].file_name().unwrap(), "part2 (merged).pdf");
        assert_eq!(page_count(&out[0]), 5);
        assert!(
            notes.iter().any(|n| n.starts_with("2 files, 5 pages")),
            "{notes:?}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn interleave_alternates_and_reports_unequal_lengths() {
        let d = scratch("interleave");
        let a = write(&d, "front.pdf", 3);
        let b = write(&d, "back.pdf", 2);
        let (out, notes) = run(
            "pdf.merge",
            &[a, b],
            json!({"interleave": true, "order": "given"}),
        )
        .unwrap();
        assert_eq!(page_count(&out[0]), 5);
        assert!(
            notes.iter().any(|n| n.contains("3 and 2 pages")),
            "{notes:?}"
        );
        let err = run(
            "pdf.merge",
            &[d.join("front.pdf")],
            json!({"interleave": true}),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("exactly two"), "{err}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_pdf_gets_a_plain_error_naming_it() {
        let d = scratch("merge-bad");
        let a = write(&d, "a.pdf", 1);
        let junk = d.join("junk.pdf");
        std::fs::write(&junk, b"hello there, I am not a pdf at all").unwrap();
        let err = run("pdf.merge", &[a, junk], json!({}))
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.contains("junk.pdf") && err.contains("not a PDF"),
            "{err}"
        );
        // Nothing was left behind.
        let left: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
