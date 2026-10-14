//! `pdf.compress`: the smallest PDF that still looks good, or one under a size. Never inflates:
//! when the result would save under 2% the original is kept and no new file is written.

use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::compress::{self, MIN_SAVING, Options, Target};

use super::{Opts, map_err_for, note_input, open_pdf, size_text};
use crate::{ModuleError, Outputs, RunContext};

fn options(opts: &Opts) -> Options {
    let target = match opts.num("under_mb").filter(|m| *m > 0.0) {
        Some(mb) => Target::UnderBytes((mb * 1_000_000.0) as u64),
        None if opts.str("target") == "quality" => {
            Target::Quality(opts.num("quality").unwrap_or(75.0) as u8)
        }
        None => Target::Smallest,
    };
    Options {
        target,
        grayscale: opts.bool("grayscale"),
        strip_metadata: opts.bool("strip_metadata"),
        strip_attachments: opts.bool("strip_attachments"),
        strip_forms: opts.bool("strip_forms"),
        max_dpi: opts.num("max_dpi").filter(|d| *d > 0.0),
    }
}

pub(super) fn run(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let many = inputs.len() > 1;
    let options = options(opts);
    let mut out = Outputs::new(&ctx.guard);
    for (k, path) in inputs.iter().enumerate() {
        ctx.check_cancel()?;
        let d = open_pdf(ctx, path, many)?;
        note_input(ctx, &d, many);
        let pages = d
            .opened
            .doc
            .pages()
            .map_err(|e| map_err_for(path, many, e))?;
        let name = if many {
            format!("{}: ", super::file_name(path))
        } else {
            String::new()
        };
        let mut pending = out.file(path, "compressed", "pdf")?;
        let sink = pending
            .file()
            .try_clone()
            .map_err(|source| ModuleError::Io {
                path: path.clone(),
                source,
            })?;
        let report = compress::compress(
            &d.opened.doc,
            &pages,
            &options,
            &ctx.cancel,
            &mut |f, l| ctx.progress((k as f32 + f) / inputs.len() as f32, l),
            Box::new(BufWriter::new(sink)),
        )
        .map_err(|e| map_err_for(path, many, e))?;
        let saved = report.saving();
        if saved < MIN_SAVING {
            // Dropping `pending` removes the temporary file: nothing is left next to the original.
            drop(pending);
            let how = if saved <= 0.0 {
                "it would not get smaller".to_string()
            } else {
                format!("it would save only {:.1}%", saved * 100.0)
            };
            ctx.note(format!(
                "{name}Already small: {how}, so the original ({}) was kept and no new file was written.",
                size_text(report.original_bytes)
            ));
            continue;
        }
        out.commit(pending)?;
        let mut line = format!(
            "{name}{} to {}, {:.0}% smaller.",
            size_text(report.original_bytes),
            size_text(report.output_bytes),
            saved * 100.0
        );
        if report.images_lossy + report.images_lossless > 0 {
            line.push_str(&format!(
                " {} of {} pictures recompressed",
                report.images_lossy + report.images_lossless,
                report.images
            ));
            if let Some(s) = report.min_image_ssim {
                line.push_str(&format!(", lowest similarity {s:.3}"));
            }
            line.push('.');
        }
        line.push_str(" Text is still selectable.");
        ctx.note(line);
        if options.target != Target::Smallest {
            ctx.note(format!("{name}Setting used: {}.", report.setting));
        }
        if let Some(false) = report.target_reached {
            ctx.note(format!(
                "{name}It could not reach the size you asked for without removing pictures. This is the smallest it went ({}). Try grayscale or a lower resolution.",
                size_text(report.output_bytes)
            ));
        }
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    #[test]
    fn a_pdf_with_nothing_to_shrink_is_kept_and_says_so() {
        let d = scratch("compress-small");
        let f = d.join("tiny.pdf");
        std::fs::write(&f, pdf(2)).unwrap();
        let (out, notes) = run("pdf.compress", &[f], json!({})).unwrap();
        // Text-only and tiny: either not worth keeping (no file), or smaller by 2% or more.
        if out.is_empty() {
            assert!(
                notes
                    .iter()
                    .any(|n| n.contains("original") && n.contains("kept")),
                "{notes:?}"
            );
            let left = std::fs::read_dir(&d).unwrap().count();
            assert_eq!(left, 1);
        } else {
            assert!(notes.iter().any(|n| n.contains("smaller")), "{notes:?}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn options_map_to_targets() {
        use super::*;
        use panoply_pdf::compress::Target;
        let m = &super::super::modules()[5].manifest;
        let o = |v| Opts(crate::options::resolve(m, &v).unwrap());
        assert_eq!(options(&o(json!({}))).target, Target::Smallest);
        assert_eq!(
            options(&o(json!({"target": "quality", "quality": 60}))).target,
            Target::Quality(60)
        );
        assert_eq!(
            options(&o(json!({"under_mb": 2}))).target,
            Target::UnderBytes(2_000_000)
        );
        assert_eq!(options(&o(json!({"max_dpi": 0}))).max_dpi, None);
    }
}
