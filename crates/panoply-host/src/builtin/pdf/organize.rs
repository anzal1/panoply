//! `pdf.organize`: keep, delete, reorder, rotate and reverse pages.

use std::collections::HashSet;
use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::pageops::{BuildOptions, CatalogMode, Input as PdfInput, OutPage, build};
use panoply_pdf::ranges;

use super::{Opts, compact_pages, map_err, note_input, open_pdf, plural, size_text};
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
        let keep_text = opts.str("keep").trim();
        let angle: i32 = opts.str("rotate").parse().unwrap_or(0);
        let reverse = opts.bool("reverse");
        let delete_text = opts.str("delete").trim();
        if keep_text.is_empty() && delete_text.is_empty() && angle == 0 && !reverse {
            return Err(ModuleError::Failed(
                "Nothing to do. Say which pages to keep or delete, a rotation, or turn on reverse."
                    .into(),
            ));
        }
        let past_end = std::cell::RefCell::new(Vec::<String>::new());
        let note_parsed = |p: &ranges::Parsed| {
            if !p.skipped.is_empty() {
                past_end.borrow_mut().extend(p.skipped.iter().cloned());
                ctx.note(format!(
                    "Left out, past the last page: {}.",
                    p.skipped.join(", ")
                ));
            }
            if !p.clamped.is_empty() {
                ctx.note(format!(
                    "Cut at the last page ({total}): {}.",
                    p.clamped.join(", ")
                ));
            }
        };
        let mut kept: Vec<usize> = if keep_text.is_empty() {
            (0..total).collect()
        } else {
            let p = ranges::parse(keep_text, total).map_err(map_err)?;
            note_parsed(&p);
            p.flat()
        };
        if !delete_text.is_empty() {
            let p = ranges::parse(delete_text, total).map_err(map_err)?;
            note_parsed(&p);
            let gone: HashSet<usize> = p.flat().into_iter().collect();
            kept.retain(|i| !gone.contains(i));
        }
        if kept.is_empty() {
            let past = past_end.borrow();
            return Err(ModuleError::Failed(if past.is_empty() {
                "Every page would be removed. Keep at least one page.".into()
            } else {
                format!(
                    "This PDF has {total} {}, and {} {} past the last one. Choose pages from 1 to {total}.",
                    if total == 1 { "page" } else { "pages" },
                    past.join(", "),
                    if past.len() == 1 { "is" } else { "are" },
                )
            }));
        }
        if reverse {
            kept.reverse();
        }
        let rotate_set: Option<HashSet<usize>> = match opts.str("rotate_pages").trim() {
            "" => None,
            t => {
                let p = ranges::parse(t, total).map_err(map_err)?;
                note_parsed(&p);
                Some(p.flat().into_iter().collect())
            }
        };
        let plan: Vec<OutPage> = kept
            .iter()
            .map(|&index| OutPage {
                input: 0,
                index,
                rotate: if rotate_set.as_ref().is_none_or(|s| s.contains(&index)) {
                    angle
                } else {
                    0
                },
            })
            .collect();

        // Keeping the whole catalog (forms, outlines, labels) is only possible when every page
        // stays exactly once.
        let unique: HashSet<usize> = kept.iter().copied().collect();
        let permutation = kept.len() == total && unique.len() == total;
        let identity = permutation && kept.iter().enumerate().all(|(i, &p)| i == p);
        let pdf_inputs = [PdfInput {
            doc: &d.opened.doc,
            pages: pages.as_slice().into(),
            title: String::new(),
        }];
        let label = if permutation
            && keep_text.is_empty()
            && delete_text.is_empty()
            && angle != 0
            && !reverse
        {
            "rotated".to_string()
        } else if permutation
            && keep_text.is_empty()
            && delete_text.is_empty()
            && angle == 0
            && reverse
        {
            "reversed".to_string()
        } else if !keep_text.is_empty() && delete_text.is_empty() && angle == 0 && !reverse {
            let mut l = format!("pages {}", compact_pages(&kept));
            l.truncate(60);
            l
        } else {
            "edited".to_string()
        };
        let mut pending = out.file(path, &label, "pdf")?;
        let report = build(
            BufWriter::new(pending.file()),
            &pdf_inputs,
            &plan,
            &BuildOptions {
                mode: if permutation {
                    CatalogMode::Full
                } else {
                    CatalogMode::Subset
                },
                drop_page_labels: !identity,
                ..Default::default()
            },
            &ctx.cancel,
            &mut |f, l| ctx.progress((k as f32 + f) / inputs.len() as f32, l),
        )
        .map_err(map_err)?;
        out.commit(pending)?;
        let deleted = total - unique.len();
        let rotated = plan.iter().filter(|p| p.rotate != 0).count();
        let mut sorted = kept.clone();
        sorted.sort_unstable();
        let moved = kept.iter().zip(&sorted).filter(|(a, b)| a != b).count();
        let mut line = format!(
            "{} to {}, {}.",
            plural(total, "page", "pages"),
            kept.len(),
            size_text(report.bytes)
        );
        let mut bits = Vec::new();
        if deleted > 0 {
            bits.push(format!("{deleted} deleted"));
        }
        if rotated > 0 {
            bits.push(format!("{rotated} rotated"));
        }
        if moved > 0 {
            bits.push(format!("{moved} moved"));
        }
        if report.links_removed > 0 {
            bits.push(format!(
                "{} removed because their page is gone",
                plural(report.links_removed, "link", "links")
            ));
        }
        if !bits.is_empty() {
            line.push_str(&format!(" {}.", bits.join(", ")));
        }
        ctx.note(line);
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    fn text_of_page(p: &std::path::Path, i: usize) -> String {
        let o = panoply_pdf::open(&std::fs::File::open(p).unwrap()).unwrap();
        let pages = o.doc.pages().unwrap();
        let page = o.doc.load(pages[i].id).unwrap();
        let panoply_pdf::obj::Obj::Dict(dict) = page.obj else {
            panic!()
        };
        String::from_utf8_lossy(&o.doc.page_content(&dict)).into_owned()
    }

    #[test]
    fn keep_reverse_rotate_and_delete() {
        let d = scratch("organize");
        let f = d.join("a.pdf");
        std::fs::write(&f, pdf(5)).unwrap();
        let (out, notes) = run(
            "pdf.organize",
            std::slice::from_ref(&f),
            json!({"keep": "5, 1-2"}),
        )
        .unwrap();
        assert!(
            out[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(pages 1-2, 5)"),
            "{out:?}"
        );
        assert!(
            text_of_page(&out[0], 0).contains("Page 5")
                && text_of_page(&out[0], 1).contains("Page 1")
        );
        assert!(
            notes.last().unwrap().starts_with("5 pages to 3"),
            "{notes:?}"
        );

        let (out, _) = run(
            "pdf.organize",
            std::slice::from_ref(&f),
            json!({"reverse": true}),
        )
        .unwrap();
        assert!(
            out[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(reversed)")
        );
        assert!(text_of_page(&out[0], 0).contains("Page 5"));

        let (out, notes) = run(
            "pdf.organize",
            std::slice::from_ref(&f),
            json!({"rotate": "90", "rotate_pages": "2"}),
        )
        .unwrap();
        let o = panoply_pdf::open(&std::fs::File::open(&out[0]).unwrap()).unwrap();
        let pages = o.doc.pages().unwrap();
        assert_eq!(o.doc.page_geom(&pages[1]).rotate, 90);
        assert_eq!(o.doc.page_geom(&pages[0]).rotate, 0);
        assert!(notes.last().unwrap().contains("1 rotated"));

        let (out, _) = run(
            "pdf.organize",
            std::slice::from_ref(&f),
            json!({"delete": "2-4"}),
        )
        .unwrap();
        assert_eq!(
            panoply_pdf::open(&std::fs::File::open(&out[0]).unwrap())
                .unwrap()
                .doc
                .pages()
                .unwrap()
                .len(),
            2
        );

        assert!(run("pdf.organize", std::slice::from_ref(&f), json!({})).is_err());
        let all_gone = run("pdf.organize", &[f], json!({"delete": "1-5"}))
            .err()
            .unwrap();
        assert!(all_gone.to_string().contains("Every page"), "{all_gone}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
