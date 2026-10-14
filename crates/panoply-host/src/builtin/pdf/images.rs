//! `pdf.images_to_pdf`: a PDF from pictures, JPEGs stored byte for byte.

use std::io::BufWriter;
use std::path::PathBuf;

use panoply_pdf::images_in::{ImageOptions, PageSize, images_to_pdf};

use super::{Opts, file_name, map_err, ordered, plural, size_text, stem};
use crate::{ModuleError, Outputs, RunContext};

fn page_size(s: &str) -> PageSize {
    match s {
        "a4" => PageSize::A4,
        "letter" => PageSize::Letter,
        "image" => PageSize::Image,
        _ => PageSize::Auto,
    }
}

pub(super) fn run(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let files = ordered(inputs, opts.str("order"));
    let image_opts = ImageOptions {
        page: page_size(opts.str("page_size")),
        margin_mm: opts.num("margin").unwrap_or(10.0),
    };
    let one_pdf = opts.bool("merge") || files.len() == 1;
    let groups: Vec<Vec<PathBuf>> = if one_pdf {
        vec![files.clone()]
    } else {
        files.iter().map(|f| vec![f.clone()]).collect()
    };
    let mut out = Outputs::new(&ctx.guard);
    let n_groups = groups.len();
    for (g, group) in groups.iter().enumerate() {
        ctx.check_cancel()?;
        let first = &group[0];
        let mut pending = if one_pdf && group.len() > 1 {
            out.file(first, "combined", "pdf")?
        } else {
            let dir = ctx.guard.output_dir_for(first)?;
            out.file_in(&dir, &format!("{}.pdf", stem(first)))?
        };
        let report = images_to_pdf(
            BufWriter::new(pending.file()),
            group.len(),
            &mut |i| {
                let p = &group[i];
                let plain = |e: ModuleError| panoply_pdf::PdfError::BadInput(e.plain_message());
                let mut f = ctx.guard.open_read(p).map_err(plain)?;
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut bytes).map_err(|source| {
                    plain(ModuleError::Io {
                        path: p.clone(),
                        source,
                    })
                })?;
                Ok((file_name(p), bytes))
            },
            &image_opts,
            &ctx.cancel,
            &mut |f, l| ctx.progress((g as f32 + f) / n_groups as f32, l),
        )
        .map_err(|e| {
            if group.len() == 1 && files.len() > 1 {
                super::map_err_for(first, true, e)
            } else {
                map_err(e)
            }
        })?;
        out.commit(pending)?;
        let mut line = format!(
            "{}, {}.",
            plural(report.pages, "page", "pages"),
            size_text(report.bytes)
        );
        if report.jpeg_passthrough > 0 {
            line.push_str(&format!(
                " {} stored without re-encoding.",
                plural(report.jpeg_passthrough, "JPEG", "JPEGs")
            ));
        }
        if report.png_passthrough > 0 {
            line.push_str(&format!(
                " {} reused as stored.",
                plural(report.png_passthrough, "PNG", "PNGs")
            ));
        }
        ctx.note(line);
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::super::testpdf::*;
    use serde_json::json;

    /// A real JPEG: a 64 x 48 gradient written with the PDF crate's own encoder is not public,
    /// so use a hand-made baseline JPEG header-less fixture via a PNG instead.
    fn png_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png_encoder(&mut out);
            enc.write_image_data(&[200u8; 32 * 24 * 3]).unwrap();
        }
        out
    }

    fn png_encoder(out: &mut Vec<u8>) -> png::Writer<&mut Vec<u8>> {
        let mut e = png::Encoder::new(out, 32, 24);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap()
    }

    #[test]
    fn pictures_become_pages_in_one_pdf_or_one_each() {
        let d = scratch("images");
        let files: Vec<_> = (1..=3)
            .map(|i| {
                let p = d.join(format!("scan{i}.png"));
                std::fs::write(&p, png_bytes()).unwrap();
                p
            })
            .collect();
        let (out, notes) = run("pdf.images_to_pdf", &files, json!({})).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            out[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(combined)"),
            "{out:?}"
        );
        assert_eq!(
            panoply_pdf::open(&std::fs::File::open(&out[0]).unwrap())
                .unwrap()
                .doc
                .pages()
                .unwrap()
                .len(),
            3
        );
        assert!(notes[0].starts_with("3 pages"), "{notes:?}");
        let (each, _) = run("pdf.images_to_pdf", &files, json!({"merge": false})).unwrap();
        assert_eq!(each.len(), 3);
        assert_eq!(each[0].file_name().unwrap(), "scan1.pdf");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn not_a_picture_is_refused_plainly() {
        let d = scratch("images-bad");
        let f = d.join("a.png");
        std::fs::write(&f, b"definitely not an image").unwrap();
        let err = run("pdf.images_to_pdf", &[f], json!({})).err().unwrap();
        assert!(!err.to_string().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
