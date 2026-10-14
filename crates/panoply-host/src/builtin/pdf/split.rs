//! `pdf.split`: cut a PDF into parts, every N pages or by page ranges.

use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::pageops::{BuildOptions, CatalogMode, Input as PdfInput, OutPage, build};
use panoply_pdf::ranges;

use super::{Opts, compact_pages, map_err, note_input, open_pdf, plural, size_text, stem};
use crate::{ModuleError, Outputs, RunContext};

pub(super) fn run(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let many = inputs.len() > 1;
    let mut out = Outputs::new(&ctx.guard);
    for (k, path) in inputs.iter().enumerate() {
        ctx.check_cancel()?;
        let d = open_pdf(ctx, path, many)?;
        note_input(ctx, &d, many);
        let pages = d.opened.doc.pages().map_err(map_err)?;
        let total = pages.len();
        // The parts: lists of zero-based pages.
        let mut parts: Vec<Vec<usize>> = Vec::new();
        if opts.str("mode") == "ranges" {
            let parsed = ranges::parse(opts.str("ranges"), total).map_err(map_err)?;
            if parsed.items.is_empty() {
                return Err(ModuleError::Failed(format!(
                    "No pages matched. This file has {}. Type the pages for each part, for example 1-3, 7, 9-.",
                    plural(total, "page", "pages")
                )));
            }
            if !parsed.skipped.is_empty() {
                ctx.note(format!(
                    "Left out, past the last page: {}.",
                    parsed.skipped.join(", ")
                ));
            }
            if !parsed.clamped.is_empty() {
                ctx.note(format!(
                    "Cut at the last page ({total}): {}.",
                    parsed.clamped.join(", ")
                ));
            }
            parts.extend(parsed.items.into_iter().map(|i| i.pages));
        } else {
            let every = opts.num("every").unwrap_or(1.0).max(1.0) as usize;
            parts.extend(
                (0..total)
                    .collect::<Vec<_>>()
                    .chunks(every)
                    .map(<[usize]>::to_vec),
            );
        }
        let name = stem(path);
        let width = parts.len().to_string().len();
        let dir = out.dir(path, "split")?;
        let pdf_inputs = [PdfInput {
            doc: &d.opened.doc,
            pages: pages.as_slice().into(),
            title: name.clone(),
        }];
        let (mut largest, mut empty) = (0u64, 0usize);
        let n = parts.len();
        for (i, part) in parts.iter().enumerate() {
            ctx.check_cancel()?;
            let plan: Vec<OutPage> = part
                .iter()
                .map(|&index| OutPage {
                    input: 0,
                    index,
                    rotate: 0,
                })
                .collect();
            let file_name = {
                let pat = opts.str("name_pattern");
                let pat = if pat.is_empty() { "{name} {n}" } else { pat };
                let mut s = pat
                    .replace("{name}", &name)
                    .replace("{n}", &format!("{:0width$}", i + 1))
                    .replace("{pages}", &compact_pages(part));
                if !s.to_lowercase().ends_with(".pdf") {
                    s.push_str(".pdf");
                }
                s
            };
            let mut pending = out.file_in(&dir, &file_name)?;
            let report = build(
                BufWriter::new(pending.file()),
                &pdf_inputs,
                &plan,
                &BuildOptions {
                    mode: CatalogMode::Subset,
                    ..Default::default()
                },
                &ctx.cancel,
                &mut |_, _| {},
            )
            .map_err(map_err)?;
            largest = largest.max(report.bytes);
            if report.bytes == 0 {
                empty += 1;
            }
            out.commit(pending)?;
            let f = (k as f32 + (i + 1) as f32 / n as f32) / inputs.len() as f32;
            ctx.progress(f, format!("Part {} of {n}", i + 1));
        }
        ctx.note(format!(
            "{}, {} largest, {empty} empty.",
            plural(n, "file", "files"),
            size_text(largest)
        ));
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    fn count(p: &std::path::Path) -> usize {
        panoply_pdf::open(&std::fs::File::open(p).unwrap())
            .unwrap()
            .doc
            .pages()
            .unwrap()
            .len()
    }

    #[test]
    fn every_n_makes_a_folder_of_numbered_parts() {
        let d = scratch("split");
        let f = d.join("book.pdf");
        std::fs::write(&f, pdf(7)).unwrap();
        let (out, notes) = run("pdf.split", &[f], json!({"every": 3})).unwrap();
        assert_eq!(out.len(), 3);
        assert!(out[0].parent().unwrap().ends_with("book (split)"));
        let counts: Vec<usize> = out.iter().map(|p| count(p)).collect();
        assert_eq!(counts, vec![3, 3, 1]);
        assert!(notes.last().unwrap().starts_with("3 files"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn ranges_become_parts_and_clamping_is_said() {
        let d = scratch("split-ranges");
        let f = d.join("a.pdf");
        std::fs::write(&f, pdf(6)).unwrap();
        let (out, notes) = run(
            "pdf.split",
            std::slice::from_ref(&f),
            json!({"mode": "ranges", "ranges": "1-2, 5-99, 50", "name_pattern": "{name} pages {pages}"}),
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert!(
            out[1]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("pages 5-6")
        );
        assert!(
            notes.iter().any(|n| n.contains("50")) && notes.iter().any(|n| n.contains("5-99")),
            "{notes:?}"
        );
        let err = run("pdf.split", &[f], json!({"mode": "ranges", "ranges": "99"}))
            .err()
            .unwrap();
        assert!(err.to_string().contains("No pages matched"), "{err}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
