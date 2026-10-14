//! `pdf.to_images`: draw pages with pdfium into PNG or JPG files, or into one ZIP.
//!
//! Encrypted files open here when a password is given (pdfium reads them; nothing is saved).

use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::ranges;
use panoply_pdf::render::{ImageFormat, RenderDoc};

use super::zip::Zip;
use super::{Opts, map_err, map_err_for, plural, stem};
use crate::{ModuleError, Outputs, RunContext};

pub(super) fn run(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let many = inputs.len() > 1;
    let format = if opts.str("format") == "jpg" {
        ImageFormat::Jpg
    } else {
        ImageFormat::Png
    };
    let dpi = opts.num("dpi").unwrap_or(150.0) as f32;
    let quality = opts.num("quality").unwrap_or(90.0) as u8;
    let password = Some(opts.str("password")).filter(|p| !p.is_empty());
    let mut out = Outputs::new(&ctx.guard);
    for (k, path) in inputs.iter().enumerate() {
        ctx.check_cancel()?;
        let file = ctx.guard.open_read(path)?;
        let doc = RenderDoc::open(file, password).map_err(|e| map_err_for(path, many, e))?;
        let total = doc.page_count();
        let pages: Vec<usize> = match opts.str("pages").trim() {
            "" => (0..total).collect(),
            text => {
                let p = ranges::parse(text, total).map_err(map_err)?;
                if !p.skipped.is_empty() {
                    ctx.note(format!(
                        "Left out, past the last page: {}.",
                        p.skipped.join(", ")
                    ));
                }
                let mut v = p.flat();
                v.dedup();
                v
            }
        };
        if pages.is_empty() {
            return Err(ModuleError::Failed(format!(
                "No pages matched. This file has {}.",
                plural(total, "page", "pages")
            )));
        }
        let name = stem(path);
        let width = total.to_string().len();
        let n = pages.len();
        let progress = |i: usize, ctx: &RunContext| {
            ctx.progress(
                (k as f32 + (i + 1) as f32 / n as f32) / inputs.len() as f32,
                format!("Page {} of {n}", i + 1),
            );
        };
        if opts.bool("zip") {
            let mut pending = out.file(path, "images", "zip")?;
            let mut zip = Zip::new(BufWriter::new(pending.file()));
            for (i, &p) in pages.iter().enumerate() {
                let entry = format!("{name} p{:0width$}.{}", p + 1, format.ext());
                zip.add::<panoply_pdf::PdfError>(&entry, |w| {
                    doc.write_page_image(p, dpi, format, quality, w, &ctx.cancel)
                        .map(|_| ())
                })
                .map_err(|e| map_err_for(path, many, e))?;
                progress(i, ctx);
            }
            zip.finish().map_err(|source| ModuleError::Io {
                path: path.clone(),
                source,
            })?;
            out.commit(pending)?;
        } else {
            let dir = out.dir(path, "images")?;
            for (i, &p) in pages.iter().enumerate() {
                let mut pending =
                    out.file_in(&dir, &format!("{name} p{:0width$}.{}", p + 1, format.ext()))?;
                {
                    let mut w = BufWriter::new(pending.file());
                    doc.write_page_image(p, dpi, format, quality, &mut w, &ctx.cancel)
                        .map_err(|e| map_err_for(path, many, e))?;
                }
                out.commit(pending)?;
                progress(i, ctx);
            }
        }
        ctx.note(format!(
            "{} at {dpi:.0} DPI as {}.",
            plural(n, "picture", "pictures"),
            format.ext().to_uppercase()
        ));
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    // These tests draw pages, so they need the pdfium library. They skip (loudly) when the
    // library is not on this machine and cannot be fetched.
    fn engine_ready() -> bool {
        match panoply_pdf::pdfium::engine() {
            Ok(_) => true,
            Err(e) => {
                eprintln!("skipped, no PDF engine here: {e}");
                false
            }
        }
    }

    #[test]
    fn pages_become_pngs_in_a_folder_and_ranges_pick_pages() {
        if !engine_ready() {
            return;
        }
        let d = scratch("toimg");
        let f = d.join("a.pdf");
        std::fs::write(&f, pdf(4)).unwrap();
        let (out, notes) = run(
            "pdf.to_images",
            std::slice::from_ref(&f),
            json!({"dpi": 72, "pages": "2-3"}),
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].parent().unwrap().ends_with("a (images)"));
        assert_eq!(&std::fs::read(&out[0]).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        assert!(
            notes.last().unwrap().starts_with("2 pictures at 72 DPI"),
            "{notes:?}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_zip_holds_valid_jpgs() {
        if !engine_ready() {
            return;
        }
        let d = scratch("toimg-zip");
        let f = d.join("a.pdf");
        std::fs::write(&f, pdf(3)).unwrap();
        let (out, _) = run(
            "pdf.to_images",
            &[f],
            json!({"dpi": 72, "format": "jpg", "zip": true}),
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].extension().unwrap() == "zip");
        let bytes = std::fs::read(&out[0]).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // Structure check with the system unzip when it exists.
        if let Ok(o) = std::process::Command::new("unzip")
            .arg("-t")
            .arg(&out[0])
            .output()
        {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}
