//! The built-in OCR modules, on the `panoply-ocr` engine: `image.to_text` (the text of a
//! picture, as plain text or Markdown, to the answer line, the clipboard or a file) and
//! `pdf.ocr` (a searchable PDF: an invisible text layer over scanned pages).
//!
//! Reads go through the capability guard, results are written through [`crate::Outputs`], and
//! the one thing these modules need the network for, the model files, is asked for through the
//! guard and told to the user before it happens.

use std::fs::File;
use std::io::{BufWriter, Read};
use std::path::{Path, PathBuf};

use panoply_manifest::Manifest;
use panoply_ocr::searchable::{self, Options as PdfOptions, page_list};
use panoply_ocr::{Fetch, Lang, Ocr, OcrError, layout};
use serde_json::Value;

use crate::{Module, ModuleError, Outputs, RunContext};

const IMAGE_TO_TEXT: &str =
    include_str!("../../../../modules/builtin/ocr-image-to-text/module.toml");
const PDF_OCR: &str = include_str!("../../../../modules/builtin/ocr-pdf/module.toml");

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `image.to_text`
    ImageToText,
    /// `pdf.ocr`
    PdfOcr,
}

/// One OCR module: its manifest and the operation it runs.
pub struct OcrModule {
    op: Op,
    manifest: Manifest,
}

/// Both OCR modules.
pub fn all() -> Vec<OcrModule> {
    [(Op::ImageToText, IMAGE_TO_TEXT), (Op::PdfOcr, PDF_OCR)]
        .into_iter()
        .map(|(op, text)| OcrModule {
            op,
            manifest: Manifest::from_toml_str(text).expect("built-in ocr manifest parses"),
        })
        .collect()
}

fn map_err(e: OcrError) -> ModuleError {
    match e {
        OcrError::Cancelled => ModuleError::Cancelled,
        other => ModuleError::Failed(other.to_string()),
    }
}

struct Opts(Value);

impl Opts {
    fn str(&self, key: &str) -> &str {
        self.0.get(key).and_then(Value::as_str).unwrap_or("")
    }
    fn bool(&self, key: &str) -> bool {
        self.0.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
    fn num(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(Value::as_f64)
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn languages(opts: &Opts) -> Result<Vec<Lang>, ModuleError> {
    Lang::parse_list(opts.str("languages")).map_err(|e| ModuleError::BadOption(e.to_string()))
}

impl Module for OcrModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        if inputs.is_empty() {
            return Err(ModuleError::Failed(
                "Choose at least one file first.".into(),
            ));
        }
        let opts = Opts(crate::options::resolve(&self.manifest, options)?);
        match self.op {
            Op::ImageToText => image_to_text(ctx, inputs, &opts),
            Op::PdfOcr => pdf_ocr(ctx, inputs, &opts),
        }
    }
}

/// Run `f` with a [`Fetch`] that says what it downloads through the job's progress and notes.
fn with_fetch<T>(ctx: &RunContext, f: impl FnOnce(&mut Fetch<'_>) -> T) -> T {
    let mut notice = |s: &str| {
        ctx.progress(0.0, s.to_string());
        ctx.note(s.to_string());
    };
    let mut fetch = Fetch::new(&mut notice);
    fetch.allow_download = ctx.guard.require_network().is_ok();
    f(&mut fetch)
}

// ---------------------------------------------------------------- image.to_text

fn image_to_text(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Opts,
) -> Result<Vec<PathBuf>, ModuleError> {
    let langs = languages(opts)?;
    let markdown = opts.str("format") == "markdown";
    let keep_breaks = opts.bool("keep_line_breaks");
    let many = inputs.len() > 1;
    let ocr = Ocr::new();
    let mut out = Outputs::new(&ctx.guard);
    let (mut words, mut low, mut conf_sum, mut conf_n) = (0usize, 0usize, 0f32, 0usize);
    let mut engines: Vec<&'static str> = Vec::new();
    let mut all_text: Vec<String> = Vec::new();
    for (k, path) in inputs.iter().enumerate() {
        ctx.check_cancel()?;
        ctx.progress(
            k as f32 / inputs.len() as f32,
            format!("Reading {}", file_name(path)),
        );
        let file = ctx.guard.open_read(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_string);
        let img = panoply_ocr::input::load_image(file, ext.as_deref()).map_err(|e| {
            if many {
                ModuleError::Failed(format!("{}: {e}", file_name(path)))
            } else {
                map_err(e)
            }
        })?;
        let page = with_fetch(ctx, |fetch| ocr.recognize(&img, &langs, fetch, &ctx.cancel))
            .map_err(|e| match e {
                OcrError::Cancelled => ModuleError::Cancelled,
                other if many => ModuleError::Failed(format!("{}: {other}", file_name(path))),
                other => ModuleError::Failed(other.to_string()),
            })?;
        drop(img);
        let text = if markdown {
            layout::markdown(&page)
        } else {
            layout::plain_text(&page, keep_breaks)
        };
        if page.word_count() == 0 {
            ctx.note(format!(
                "{}No text found. If the picture has text in another language, choose the language, for example --opt languages=hi.",
                if many { format!("{}: ", file_name(path)) } else { String::new() }
            ));
        } else {
            if many {
                ctx.note(format!("== {} ==", file_name(path)));
            }
            ctx.note(text.clone());
        }
        words += page.word_count();
        low += page.low_confidence_words();
        let n: usize = page.words().map(|w| w.text.chars().count()).sum();
        conf_sum += page.mean_confidence() * n as f32;
        conf_n += n;
        if !engines.contains(&page.engine.name()) {
            engines.push(page.engine.name());
        }
        if opts.bool("save") && page.word_count() > 0 {
            let dir = ctx.guard.output_dir_for(path)?;
            let ext = if markdown { "md" } else { "txt" };
            let mut pending = out.file_in(&dir, &format!("{}.{ext}", stem(path)))?;
            std::io::Write::write_all(pending.file(), text.as_bytes())
                .and_then(|()| std::io::Write::write_all(pending.file(), b"\n"))
                .map_err(|source| ModuleError::Io {
                    path: path.clone(),
                    source,
                })?;
            out.commit(pending)?;
        }
        all_text.push(text);
    }
    if opts.bool("copy") && words > 0 {
        panoply_ocr::clipboard::copy(&all_text.join("\n\n")).map_err(map_err)?;
        ctx.note("Copied to the clipboard.");
    }
    // The last note is the result card; the command line prints it apart from the text.
    let conf = if conf_n == 0 {
        0.0
    } else {
        conf_sum / conf_n as f32
    };
    ctx.note(format!(
        "{}, {:.0}% mean confidence, {} (read with {}).",
        plural(words, "word", "words"),
        conf * 100.0,
        plural(low, "low-confidence word", "low-confidence words"),
        engines.join(" and ")
    ));
    ctx.progress(1.0, "Done");
    Ok(out.finish())
}

// ---------------------------------------------------------------- pdf.ocr

fn is_pdf(path: &Path, ctx: &RunContext) -> Result<bool, ModuleError> {
    let mut f = ctx.guard.open_read(path)?;
    let mut head = [0u8; 1024];
    let n = f.read(&mut head).map_err(|source| ModuleError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(head[..n].windows(5).any(|w| w == b"%PDF-"))
}

fn pdf_ocr(ctx: &RunContext, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>, ModuleError> {
    let langs = languages(opts)?;
    let options = PdfOptions {
        langs: langs.clone(),
        dpi: opts.num("dpi").unwrap_or(300.0) as f32,
        collect_text: opts.bool("text_file"),
        ..PdfOptions::default()
    };
    let many = inputs.len() > 1;
    let ocr = Ocr::new();
    let mut out = Outputs::new(&ctx.guard);
    for (k, path) in inputs.iter().enumerate() {
        ctx.check_cancel()?;
        let prefix = if many {
            format!("{}: ", file_name(path))
        } else {
            String::new()
        };
        let share = |f: f32| (k as f32 + f) / inputs.len() as f32;
        let mut pending = out.file(path, "searchable", "pdf")?;
        let sink = pending
            .file()
            .try_clone()
            .map_err(|source| ModuleError::Io {
                path: path.clone(),
                source,
            })?;
        let mut progress = |f: f32, l: &str| ctx.progress(share(f), l.to_string());
        let report = if is_pdf(path, ctx)? {
            let file = ctx.guard.open_read(path)?;
            with_fetch(ctx, |fetch| {
                searchable::make_searchable(
                    &file,
                    BufWriter::new(sink),
                    &options,
                    &ocr,
                    fetch,
                    &ctx.cancel,
                    &mut progress,
                )
            })
        } else {
            // A picture: make a one page PDF of it, then read the picture itself.
            let tmp = picture_pdf(ctx, path)?;
            let result = (|| {
                let img_file = ctx.guard.open_read(path)?;
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_string);
                let mut cached: Option<panoply_ocr::OcrImage> = None;
                let mut draw = |_: usize| -> panoply_ocr::Result<panoply_ocr::OcrImage> {
                    if cached.is_none() {
                        cached = Some(panoply_ocr::input::load_image(
                            img_file.try_clone()?,
                            ext.as_deref(),
                        )?);
                    }
                    Ok(cached.clone().expect("loaded just above"))
                };
                Ok::<_, ModuleError>(with_fetch(ctx, |fetch| {
                    searchable::make_searchable_with(
                        tmp.file(),
                        BufWriter::new(sink),
                        &options,
                        &ocr,
                        fetch,
                        &ctx.cancel,
                        &mut progress,
                        &mut draw,
                    )
                }))
            })();
            drop(tmp);
            result?
        }
        .map_err(|e| match e {
            OcrError::Cancelled => ModuleError::Cancelled,
            other if many => ModuleError::Failed(format!("{}: {other}", file_name(path))),
            other => ModuleError::Failed(other.to_string()),
        })?;
        if report.read == 0 || report.words == 0 {
            // Dropping `pending` removes the temporary file: nothing is left next to the original.
            drop(pending);
            if report.read == 0 {
                ctx.note(format!(
                    "{prefix}All {} already have text. Nothing was changed and no new file was written.",
                    plural(report.pages, "page", "pages")
                ));
            } else {
                ctx.note(format!(
                    "{prefix}No text was found on {}, so nothing was changed and no new file was written. If the text is in another language, choose it, for example --lang hi.",
                    plural(report.read, "page", "pages")
                ));
            }
            continue;
        }
        out.commit(pending)?;
        if report.repaired {
            ctx.note(format!(
                "{prefix}This PDF was damaged and was repaired in memory. The original is untouched."
            ));
        }
        if report.signed {
            ctx.note(format!(
                "{prefix}This PDF is digitally signed. The new file is not signed, because adding text after signing breaks the signature. The original is untouched."
            ));
        }
        if report.had_text > 0 {
            ctx.note(format!(
                "{prefix}{} already had text and {} left as they were.",
                plural(report.had_text, "page", "pages"),
                if report.had_text == 1 { "was" } else { "were" }
            ));
        }
        let engines: Vec<&str> = report.engines.iter().map(|e| e.name()).collect();
        let langs_phrase = if langs.is_empty() {
            String::new()
        } else {
            format!(", {}", panoply_ocr::lang::phrase(&langs))
        };
        let mut card = format!(
            "{prefix}{}{langs_phrase}, {} words",
            plural(report.pages, "page", "pages"),
            report.words
        );
        if report.flagged.is_empty() {
            card.push('.');
        } else {
            card.push_str(&format!(
                ", {} flagged for low confidence ({}).",
                plural(report.flagged.len(), "page", "pages"),
                page_list(&report.flagged)
            ));
        }
        ctx.note(card);
        ctx.note(format!(
            "{prefix}Read {} with {}, {:.0}% mean confidence.",
            plural(report.read, "page", "pages"),
            engines.join(" and "),
            report.mean_confidence() * 100.0
        ));
        if opts.bool("text_file") {
            let dir = ctx.guard.output_dir_for(path)?;
            let mut p = out.file_in(&dir, &format!("{}.txt", stem(path)))?;
            std::io::Write::write_all(p.file(), report.text.replace('\u{c}', "\n\n").as_bytes())
                .map_err(|source| ModuleError::Io {
                    path: path.clone(),
                    source,
                })?;
            out.commit(p)?;
        }
    }
    Ok(out.finish())
}

/// A scratch PDF made of one picture, deleted when dropped.
struct TempPdf {
    path: PathBuf,
    file: File,
}

impl TempPdf {
    fn file(&self) -> &File {
        &self.file
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// True for the two picture formats a PDF page can carry as they are.
fn is_pdf_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// A picture file of any format the image pipeline reads, as an upright sRGB PNG with any
/// transparency flattened onto white (what the reader sees is what the engine reads).
fn as_png(file: File, path: &Path) -> Result<Vec<u8>, String> {
    let ext = path.extension().and_then(|e| e.to_str());
    let p = panoply_image::preview::preview(file, ext, panoply_ocr::input::MAX_SIDE, false)
        .map_err(|e| e.to_string())?;
    let mut rgb = Vec::with_capacity(p.width as usize * p.height as usize * 3);
    for px in p.rgba.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        for &c in &px[..3] {
            rgb.push(((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, p.width, p.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let sorry =
        |e: png::EncodingError| format!("This picture could not be prepared for reading: {e}.");
    let mut w = enc.write_header().map_err(sorry)?;
    w.write_image_data(&rgb).map_err(sorry)?;
    w.finish().map_err(sorry)?;
    Ok(out)
}

fn picture_pdf(ctx: &RunContext, path: &Path) -> Result<TempPdf, ModuleError> {
    let tmp = std::env::temp_dir().join(format!(
        "panoply-ocr-{}-{}.pdf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let io = |source| ModuleError::Io {
        path: path.to_path_buf(),
        source,
    };
    let out = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(io)?;
    let guard = TempPdf {
        path: tmp,
        file: out,
    };
    let sink = guard.file.try_clone().map_err(io)?;
    panoply_pdf::images_in::images_to_pdf(
        BufWriter::new(sink),
        1,
        &mut |_| {
            let plain = |e: ModuleError| panoply_pdf::PdfError::BadInput(e.plain_message());
            let mut f = ctx.guard.open_read(path).map_err(plain)?;
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes)
                .map_err(panoply_pdf::PdfError::from)?;
            // A PDF page holds a JPEG or a PNG. Any other picture the OCR can read (HEIC,
            // AVIF, WebP, GIF, TIFF, camera RAW) is drawn once and kept as a PNG.
            if !is_pdf_picture(&bytes) {
                drop(bytes);
                let again = ctx.guard.open_read(path).map_err(plain)?;
                bytes = as_png(again, path).map_err(panoply_pdf::PdfError::BadInput)?;
            }
            Ok((file_name(path), bytes))
        },
        &panoply_pdf::images_in::ImageOptions {
            page: panoply_pdf::images_in::PageSize::Image,
            margin_mm: 0.0,
        },
        &ctx.cancel,
        &mut |_, _| {},
    )
    .map_err(|e| match e {
        panoply_pdf::PdfError::Cancelled => ModuleError::Cancelled,
        other => ModuleError::Failed(other.to_string()),
    })?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;
    use panoply_core::{CancelToken, JobSpec};
    use serde_json::json;

    fn picture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../panoply-ocr/testdata/labelled/en-1.png")
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-host-ocr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Run a job; `None` when the engine files could not be fetched (no network), so offline
    /// test runs skip instead of failing.
    fn run(id: &str, input: &Path, options: serde_json::Value) -> Option<crate::JobOutcome> {
        let spec = JobSpec {
            module_id: id.into(),
            inputs: vec![input.to_path_buf()],
            options,
        };
        match Registry::with_builtins().run_job(&spec, None, CancelToken::new(), |_| {}) {
            Ok(o) => Some(o),
            Err(e) if e.to_string().contains("download") || e.to_string().contains("network") => {
                eprintln!("skipped: {e}");
                None
            }
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn image_to_text_answers_in_notes_and_can_save_a_file() {
        let d = scratch("img");
        let pic = d.join("shot.png");
        std::fs::copy(picture(), &pic).unwrap();
        let Some(o) = run(
            "image.to_text",
            &pic,
            json!({"languages": "en", "save": true}),
        ) else {
            return;
        };
        assert!(o.notes[0].contains("quick brown fox"), "{:?}", o.notes);
        // Lines are joined into paragraphs by default.
        assert!(!o.notes[0].contains('\n'), "{:?}", o.notes);
        let card = o.notes.last().unwrap();
        assert!(
            card.contains("words") && card.contains("mean confidence"),
            "{card}"
        );
        assert_eq!(o.outputs.len(), 1);
        assert_eq!(o.outputs[0].file_name().unwrap(), "shot.txt");
        let saved = std::fs::read_to_string(&o.outputs[0]).unwrap();
        assert!(saved.contains("lazy dog"));
        // Markdown with kept line breaks.
        let o = run(
            "image.to_text",
            &pic,
            json!({"languages": "en", "keep_line_breaks": true}),
        )
        .unwrap();
        assert!(o.notes[0].contains('\n'));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_scan_with_no_text_writes_no_new_file() {
        let d = scratch("blank");
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 64, 64);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[255u8; 64 * 64 * 3])
                .unwrap();
        }
        let pic = d.join("blank.png");
        std::fs::write(&pic, png).unwrap();
        let Some(o) = run("pdf.ocr", &pic, json!({"languages": "en"})) else {
            return;
        };
        assert!(o.outputs.is_empty(), "{:?}", o.outputs);
        assert!(o.notes[0].contains("No text was found"), "{:?}", o.notes);
        let names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "nothing but the picture: {names:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A 2 x 2 24-bit BMP: red, green, blue, white.
    fn tiny_bmp() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&(54u32 + 16).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&[0; 24]);
        // Rows are bottom up, BGR, padded to 4 bytes.
        b.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0]);
        b.extend_from_slice(&[0, 0, 255, 255, 255, 255, 0, 0]);
        b
    }

    #[test]
    fn a_picture_a_pdf_cannot_hold_is_kept_as_a_png_for_reading() {
        let d = scratch("as-png");
        let bmp = d.join("pic.bmp");
        std::fs::write(&bmp, tiny_bmp()).unwrap();
        assert!(!is_pdf_picture(&std::fs::read(&bmp).unwrap()));
        let png = as_png(File::open(&bmp).unwrap(), &bmp).unwrap();
        assert!(is_pdf_picture(&png), "the result is a PNG");
        let dec = png::Decoder::new(std::io::Cursor::new(&png));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bad_language_is_a_plain_option_error() {
        let d = scratch("lang");
        let pic = d.join("a.png");
        std::fs::copy(picture(), &pic).unwrap();
        let spec = JobSpec {
            module_id: "image.to_text".into(),
            inputs: vec![pic],
            options: json!({"languages": "klingon"}),
        };
        let e = Registry::with_builtins()
            .run_job(&spec, None, CancelToken::new(), |_| {})
            .err()
            .unwrap();
        assert!(matches!(e, ModuleError::BadOption(_)), "{e:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn pdf_ocr_makes_a_searchable_copy_and_leaves_searchable_files_alone() {
        let d = scratch("pdf");
        let scan = d.join("scan.pdf");
        let png = std::fs::read(picture()).unwrap();
        panoply_pdf::images_in::images_to_pdf(
            std::io::BufWriter::new(std::fs::File::create(&scan).unwrap()),
            1,
            &mut |_| Ok(("scan.png".into(), png.clone())),
            &panoply_pdf::images_in::ImageOptions {
                page: panoply_pdf::images_in::PageSize::Image,
                margin_mm: 0.0,
            },
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap();
        let Some(o) = run(
            "pdf.ocr",
            &scan,
            json!({"languages": "en", "text_file": true}),
        ) else {
            return;
        };
        let names: Vec<String> = o.outputs.iter().map(|p| file_name(p)).collect();
        assert_eq!(
            names,
            vec!["scan (searchable).pdf", "scan.txt"],
            "{:?}",
            o.notes
        );
        assert!(
            o.notes
                .iter()
                .any(|n| n.starts_with("1 page") && n.contains("words")),
            "{:?}",
            o.notes
        );
        let txt = std::fs::read_to_string(&o.outputs[1]).unwrap();
        assert!(txt.contains("lazy dog"));
        // The original is untouched, and running again on the result writes nothing.
        assert!(scan.exists());
        let again = run("pdf.ocr", &o.outputs[0], json!({"languages": "en"})).unwrap();
        assert!(again.outputs.is_empty());
        assert!(
            again.notes[0].contains("already have text"),
            "{:?}",
            again.notes
        );
        // A picture goes straight to a searchable PDF too.
        let pic = d.join("photo.png");
        std::fs::write(&pic, &png).unwrap();
        let from_pic = run("pdf.ocr", &pic, json!({"languages": "en"})).unwrap();
        assert_eq!(file_name(&from_pic.outputs[0]), "photo (searchable).pdf");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn both_manifests_are_valid() {
        let ms = all();
        assert_eq!(ms.len(), 2);
        for m in &ms {
            m.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", m.manifest.module.id));
        }
        let ids: Vec<_> = ms.iter().map(|m| m.manifest.module.id.as_str()).collect();
        assert_eq!(ids, ["image.to_text", "pdf.ocr"]);
    }
}
