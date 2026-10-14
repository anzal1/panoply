//! Compress PDF.
//!
//! One pass copies the document through [`crate::copy::Copier`] with a policy that recodes
//! streams on the way:
//!
//! - Images are shrunk to the resolution the page shows and stored as JPEG at the lowest quality
//!   that scores an SSIM of at least 0.97 against the original, or stored again losslessly. An
//!   image that does not get smaller is copied untouched. JPEG 2000 and JBIG2 pictures are
//!   decoded by pdfium (`engine_decode`) and recoded by content: gray and colour as JPEG, 1-bit as
//!   CCITT Group 4 when that is smaller.
//! - Other streams are deflated at the best level, and streams stored raw get a filter.
//! - Identical streams are stored once, objects nothing reaches are dropped, and small objects
//!   are packed into object streams behind a compact cross-reference stream.
//! - Metadata, attachments and form data can be stripped.
//!
//! Targets: the smallest file that still looks good (SSIM floor), a fixed quality, or under a
//! size: the engine estimates from a sample of the images which setting of the dial fits, then
//! confirms with whole-document passes and tells you when the size cannot be reached.

mod analyze;
mod engine_decode;
mod g4;
pub mod image;

use std::collections::BTreeMap;
use std::io::Write;

use panoply_core::CancelToken;

use crate::copy::{Policy, View};
use crate::doc::{Doc, PageRef, RawStream, SignatureInfo};
use crate::error::{PdfError, Result};
use crate::filters::{self, Filter};
use crate::obj::{Dict, Obj};
use crate::pageops::{BuildOptions, CatalogMode, Input, OutPage, build_with};
use crate::writer::WriterOptions;
use analyze::Analysis;
use image::{Level, Params, Quality, Skip, Verdict};

/// The SSIM floor for the "smallest that still looks good" setting.
pub const SSIM_FLOOR: f64 = 0.97;
/// A result that saves less than this share is not worth keeping.
pub const MIN_SAVING: f64 = 0.02;

/// What size to aim for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// The smallest file whose images all keep an SSIM of at least [`SSIM_FLOOR`].
    Smallest,
    /// At most this many bytes, at the best quality that fits.
    UnderBytes(u64),
    /// A JPEG quality from 1 to 100.
    Quality(u8),
}

/// Settings for [`compress`].
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// What to aim for.
    pub target: Target,
    /// Turn colour images gray.
    pub grayscale: bool,
    /// Remove document information and XMP metadata.
    pub strip_metadata: bool,
    /// Remove embedded files and file attachment annotations.
    pub strip_attachments: bool,
    /// Remove the interactive form and its fields.
    pub strip_forms: bool,
    /// Highest image resolution to keep, in pixels per inch. `None` picks by the target.
    pub max_dpi: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            target: Target::Smallest,
            grayscale: false,
            strip_metadata: false,
            strip_attachments: false,
            strip_forms: false,
            max_dpi: None,
        }
    }
}

/// What a compression did, for the result card and the notes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Size of the input file.
    pub original_bytes: u64,
    /// Size of the output file.
    pub output_bytes: u64,
    /// Images in the document (after removing duplicates).
    pub images: usize,
    /// Images stored as a JPEG.
    pub images_lossy: usize,
    /// Images stored again without loss.
    pub images_lossless: usize,
    /// Images left as they were, by reason.
    pub images_kept: BTreeMap<String, usize>,
    /// Stored size of all images before and after.
    pub image_bytes_before: u64,
    /// Stored size of all images in the output.
    pub image_bytes_after: u64,
    /// The lowest SSIM among recoded images, against the original at the new size.
    pub min_image_ssim: Option<f64>,
    /// Non-image streams deflated better.
    pub streams_recompressed: usize,
    /// Streams that were duplicates of another and were stored once.
    pub duplicates_removed: usize,
    /// Objects in the input that nothing reached.
    pub objects_dropped: usize,
    /// Bytes of embedded fonts, and how many are the full font instead of a subset.
    pub font_bytes: u64,
    /// Bytes of fully embedded fonts.
    pub full_font_bytes: u64,
    /// Signatures the input carried (the output is not signed).
    pub signatures: usize,
    /// For an under-a-size target: whether it was reached.
    pub target_reached: Option<bool>,
    /// Whole-document passes made (1 unless the target was a size).
    pub passes: usize,
    /// The dial setting used, for the notes.
    pub setting: String,
}

impl Report {
    /// Share of the input saved, 0.0 to 1.0 (negative when the output grew).
    pub fn saving(&self) -> f64 {
        if self.original_bytes == 0 {
            0.0
        } else {
            1.0 - self.output_bytes as f64 / self.original_bytes as f64
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Stats {
    images: usize,
    lossy: usize,
    lossless: usize,
    kept: BTreeMap<Skip, usize>,
    image_before: u64,
    image_after: u64,
    min_ssim: Option<f64>,
    streams: usize,
}

#[derive(Clone, Copy)]
enum ImageMode {
    Recode(Level),
    Untouched,
}

struct CompressPolicy<'a> {
    analysis: &'a Analysis,
    opts: &'a Options,
    mode: ImageMode,
    stats: Stats,
}

impl Policy for CompressPolicy<'_> {
    fn dict(&mut self, _doc: &Doc, _view: &View, _src: u32, d: &mut Dict) {
        if self.opts.strip_metadata && matches!(d.name("Type"), Some(b"Page") | Some(b"XObject")) {
            d.remove("Metadata");
            d.remove("PieceInfo");
        }
    }

    fn alias_of(&self, src: u32) -> Option<u32> {
        self.analysis.alias.get(&src).copied()
    }

    fn keep_annot(
        &mut self,
        _doc: &Doc,
        _view: &View,
        _num: Option<u32>,
        annot: &mut Dict,
    ) -> bool {
        match annot.name("Subtype") {
            Some(b"FileAttachment") => !self.opts.strip_attachments,
            Some(b"Widget") => !self.opts.strip_forms,
            _ => true,
        }
    }

    fn stream(
        &mut self,
        doc: &Doc,
        src: u32,
        dict: &Dict,
        raw: RawStream,
    ) -> Result<Option<(Dict, Vec<u8>)>> {
        if dict.name("Subtype") == Some(b"Image") {
            self.stats.images += 1;
            self.stats.image_before += raw.len;
            let ImageMode::Recode(level) = self.mode else {
                self.stats.image_after += raw.len;
                return Ok(None);
            };
            let params = Params {
                level,
                eff_dpi: self.analysis.dpi.get(&src).copied(),
                is_smask: self.analysis.smasks.contains(&src),
                grayscale: self.opts.grayscale,
                doc,
            };
            return match image::recode(&params, dict, raw)? {
                Verdict::Recoded {
                    dict,
                    data,
                    ssim,
                    lossy,
                } => {
                    if lossy {
                        self.stats.lossy += 1;
                    } else {
                        self.stats.lossless += 1;
                    }
                    if let Some(s) = ssim {
                        self.stats.min_ssim = Some(self.stats.min_ssim.map_or(s, |m| m.min(s)));
                    }
                    self.stats.image_after += data.len() as u64;
                    Ok(Some((dict, data)))
                }
                Verdict::Kept(why) => {
                    *self.stats.kept.entry(why).or_insert(0) += 1;
                    self.stats.image_after += raw.len;
                    Ok(None)
                }
            };
        }
        self.generic(doc, dict, raw)
    }
}

impl CompressPolicy<'_> {
    /// Deflate a stream better, or add a filter to one stored raw.
    fn generic(
        &mut self,
        doc: &Doc,
        dict: &Dict,
        raw: RawStream,
    ) -> Result<Option<(Dict, Vec<u8>)>> {
        if raw.len < 96 || raw.len > 256 * 1024 * 1024 {
            return Ok(None);
        }
        if dict.name("Type") == Some(b"Metadata") || dict.name("Subtype") == Some(b"XML") {
            return Ok(None);
        }
        let chain = doc.filters(dict);
        match chain.as_slice() {
            [] => {
                let data = doc.raw(raw)?;
                let z = filters::deflate(&data, if data.len() > 4 << 20 { 6 } else { 9 });
                if z.len() + 16 < data.len() {
                    let mut nd = dict.clone();
                    nd.set("Filter", Obj::name("FlateDecode"));
                    self.stats.streams += 1;
                    return Ok(Some((nd, z)));
                }
                Ok(None)
            }
            [(Filter::Flate, parms)] if parms.int("Predictor").unwrap_or(1) <= 1 => {
                let packed = doc.raw(raw)?;
                let Ok(data) = filters::inflate(&packed, crate::doc::DECODE_LIMIT, false) else {
                    return Ok(None);
                };
                let z = filters::deflate(&data, if data.len() > 4 << 20 { 6 } else { 9 });
                if (z.len() as f64) < packed.len() as f64 * 0.97 {
                    self.stats.streams += 1;
                    return Ok(Some((dict.clone(), z)));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

struct PassOut {
    bytes: u64,
    stats: Stats,
}

#[allow(clippy::too_many_arguments)]
fn run_pass<W: Write>(
    doc: &Doc,
    pages: &[PageRef],
    analysis: &Analysis,
    opts: &Options,
    mode: ImageMode,
    out: W,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32),
) -> Result<PassOut> {
    let (maj, min) = doc.version();
    let version = if (maj, min) < (1, 5) {
        (1, 5)
    } else {
        (maj, min)
    };
    let inputs = [Input {
        doc,
        pages: pages.into(),
        title: String::new(),
    }];
    let plan: Vec<OutPage> = (0..pages.len())
        .map(|index| OutPage {
            input: 0,
            index,
            rotate: 0,
        })
        .collect();
    let build_opts = BuildOptions {
        mode: CatalogMode::Full,
        bookmarks: false,
        drop_page_labels: false,
        info_from: if opts.strip_metadata { None } else { Some(0) },
        writer: WriterOptions {
            version,
            object_streams: true,
            level: 9,
        },
    };
    let strip = |cat: &mut Dict| {
        if opts.strip_metadata {
            cat.remove("Metadata");
            cat.remove("PieceInfo");
        }
        if opts.strip_attachments {
            cat.remove("AF");
            if let Some(mut names) = cat.get("Names").and_then(|n| doc.resolve_dict(n)) {
                names.remove("EmbeddedFiles");
                cat.set("Names", Obj::Dict(names));
            }
        }
        if opts.strip_forms {
            cat.remove("AcroForm");
        }
    };
    let (report, mut policies) = build_with(
        out,
        &inputs,
        &plan,
        &build_opts,
        cancel,
        &mut |f, _| progress(f),
        |_| CompressPolicy {
            analysis,
            opts,
            mode,
            stats: Stats::default(),
        },
        &strip,
    )?;
    let policy = policies.pop().expect("one input, one policy");
    Ok(PassOut {
        bytes: report.bytes,
        stats: policy.stats,
    })
}

/// The settings for each target.
fn level_for(opts: &Options, rung: usize) -> Level {
    let cap = |default: f64| opts.max_dpi.unwrap_or(default);
    match opts.target {
        Target::Smallest => Level {
            dpi_cap: cap(150.0),
            quality: Quality::Adaptive { floor: SSIM_FLOOR },
        },
        Target::Quality(q) => Level {
            dpi_cap: cap(match q {
                90..=100 => 300.0,
                75..=89 => 220.0,
                55..=74 => 150.0,
                _ => 110.0,
            }),
            quality: Quality::Fixed(q.clamp(1, 100)),
        },
        Target::UnderBytes(_) => {
            let (dpi, q) = LADDER[rung.min(LADDER.len() - 1)];
            if rung == 0 {
                Level {
                    dpi_cap: cap(dpi),
                    quality: Quality::Adaptive { floor: SSIM_FLOOR },
                }
            } else {
                Level {
                    dpi_cap: cap(dpi).min(dpi),
                    quality: Quality::Fixed(q),
                }
            }
        }
    }
}

/// From mildest to harshest: resolution cap and JPEG quality. Rung 0 is the "smallest that
/// still looks good" setting with the SSIM floor.
const LADDER: &[(f64, u8)] = &[
    (150.0, 0),
    (130.0, 68),
    (110.0, 62),
    (96.0, 56),
    (84.0, 50),
    (72.0, 44),
    (64.0, 38),
    (54.0, 32),
    (46.0, 26),
    (40.0, 20),
];

/// Estimate the output size at each rung from a sample of the images.
fn estimate(
    doc: &Doc,
    analysis: &Analysis,
    opts: &Options,
    s0: u64,
    image_bytes: u64,
    cancel: &CancelToken,
) -> Result<Vec<f64>> {
    // The largest images first, until a quarter of the image bytes or three images are covered.
    let mut imgs = analysis.images.clone();
    imgs.sort_by_key(|i| std::cmp::Reverse(i.1));
    let total: u64 = imgs.iter().map(|i| i.1).sum();
    let mut sample = Vec::new();
    let mut covered = 0u64;
    for (n, len) in imgs {
        if analysis.alias.contains_key(&n) {
            continue;
        }
        sample.push((n, len));
        covered += len;
        if covered * 4 >= total && sample.len() >= 3 {
            break;
        }
        if sample.len() >= 12 {
            break;
        }
    }
    // Each sampled image is decoded once and tried at every rung.
    let mut before = 0u64;
    let mut after = vec![0u64; LADDER.len()];
    for &(n, len) in &sample {
        if cancel.is_cancelled() {
            return Err(PdfError::Cancelled);
        }
        let l = doc.load(n)?;
        let (Obj::Dict(d), Some(raw)) = (l.obj, l.stream) else {
            continue;
        };
        before += len;
        // Loaded as the mildest rung would, but not skipped for being unlikely to shrink: the
        // harsher rungs may want it.
        let probe = Params {
            level: Level {
                quality: Quality::Fixed(50),
                ..level_for(opts, 0)
            },
            eff_dpi: analysis.dpi.get(&n).copied(),
            is_smask: analysis.smasks.contains(&n),
            grayscale: opts.grayscale,
            doc,
        };
        let loaded = image::load(&probe, &d, raw)?.ok();
        for (rung, slot) in after.iter_mut().enumerate() {
            if cancel.is_cancelled() {
                return Err(PdfError::Cancelled);
            }
            let params = Params {
                level: level_for(opts, rung),
                ..probe
            };
            *slot += match &loaded {
                Some(loaded) => match image::encode(&params, &d, raw, loaded)? {
                    Verdict::Recoded { data, .. } => data.len() as u64,
                    Verdict::Kept(_) => len,
                },
                None => len,
            };
        }
    }
    let ratio = |a: u64| {
        if before == 0 {
            1.0
        } else {
            a as f64 / before as f64
        }
    };
    Ok(after
        .iter()
        .map(|&a| s0 as f64 - image_bytes as f64 + image_bytes as f64 * ratio(a))
        .collect())
}

/// Compress `doc` into `out`. The document must not be encrypted. `pages` is `doc.pages()`.
pub fn compress(
    doc: &Doc,
    pages: &[PageRef],
    opts: &Options,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    mut out: Box<dyn Write>,
) -> Result<Report> {
    doc.require_unencrypted()?;
    progress(0.0, "Looking at the document");
    let analysis = analyze::analyze(doc, pages, cancel)?;
    let signed: SignatureInfo = doc.signatures();
    let mut passes = 0usize;
    let mut reached = None;
    let setting: String;
    let result = match opts.target {
        Target::Smallest | Target::Quality(_) => {
            let level = level_for(opts, 0);
            setting = describe(&level);
            passes += 1;
            run_pass(
                doc,
                pages,
                &analysis,
                opts,
                ImageMode::Recode(level),
                &mut out,
                cancel,
                &mut |f| {
                    progress(0.1 + 0.9 * f, "Compressing");
                },
            )?
        }
        Target::UnderBytes(target) => {
            // 1. What the file weighs with every image left alone.
            progress(0.05, "Measuring");
            let s0 = run_pass(
                doc,
                pages,
                &analysis,
                opts,
                ImageMode::Untouched,
                std::io::sink(),
                cancel,
                &mut |_| {},
            )?;
            passes += 1;
            if s0.bytes <= target {
                setting = "images untouched, structure compacted".to_string();
                reached = Some(true);
                passes += 1;
                run_pass(
                    doc,
                    pages,
                    &analysis,
                    opts,
                    ImageMode::Untouched,
                    &mut out,
                    cancel,
                    &mut |f| {
                        progress(0.5 + 0.5 * f, "Writing");
                    },
                )?
            } else {
                // 2. Which setting should fit, from a sample of the images.
                let image_bytes = s0.stats.image_before;
                progress(0.15, "Estimating");
                let est = estimate(doc, &analysis, opts, s0.bytes, image_bytes, cancel)?;
                let want = target as f64 * 0.985;
                let mut rung = est
                    .iter()
                    .position(|e| *e <= want)
                    .unwrap_or(LADDER.len() - 1);
                // 3. Confirm with whole passes, stepping until it fits.
                let mut best: Option<usize> = None;
                let mut tried: Vec<(usize, u64)> = Vec::new();
                for attempt in 0..4 {
                    progress(0.3 + 0.1 * attempt as f32, "Checking the size");
                    let size = run_pass(
                        doc,
                        pages,
                        &analysis,
                        opts,
                        ImageMode::Recode(level_for(opts, rung)),
                        std::io::sink(),
                        cancel,
                        &mut |_| {},
                    )?
                    .bytes;
                    passes += 1;
                    tried.push((rung, size));
                    if size <= target {
                        best = Some(rung);
                        // Close enough under the target, or nothing milder left to try.
                        if size as f64 >= target as f64 * 0.93 || rung == 0 {
                            break;
                        }
                        // Try one step milder, once.
                        if tried.iter().any(|(r, _)| *r + 1 == rung) {
                            break;
                        }
                        rung -= 1;
                    } else {
                        if rung + 1 >= LADDER.len() {
                            break;
                        }
                        if best.is_some() {
                            break;
                        }
                        rung += 1;
                    }
                }
                let chosen = best.unwrap_or(LADDER.len() - 1);
                reached = Some(best.is_some());
                let level = level_for(opts, chosen);
                setting = describe(&level);
                passes += 1;
                run_pass(
                    doc,
                    pages,
                    &analysis,
                    opts,
                    ImageMode::Recode(level),
                    &mut out,
                    cancel,
                    &mut |f| {
                        progress(0.7 + 0.3 * f, "Compressing");
                    },
                )?
            }
        }
    };
    out.flush()?;
    let mut kept = BTreeMap::new();
    for (k, v) in &result.stats.kept {
        kept.insert(format!("{k:?}"), *v);
    }
    let copied_objects = result.stats.images + result.stats.streams; // not a count of objects
    let _ = copied_objects;
    Ok(Report {
        original_bytes: doc.file_len(),
        output_bytes: result.bytes,
        images: result.stats.images,
        images_lossy: result.stats.lossy,
        images_lossless: result.stats.lossless,
        images_kept: kept,
        image_bytes_before: result.stats.image_before,
        image_bytes_after: result.stats.image_after,
        min_image_ssim: result.stats.min_ssim,
        streams_recompressed: result.stats.streams,
        duplicates_removed: analysis.alias.len(),
        objects_dropped: 0,
        font_bytes: analysis.font_bytes,
        full_font_bytes: analysis.full_font_bytes,
        signatures: signed.count,
        target_reached: reached,
        passes,
        setting,
    })
}

fn describe(l: &Level) -> String {
    match l.quality {
        Quality::Adaptive { floor } => format!(
            "images at most {:.0} dpi, JPEG quality chosen per image for SSIM {floor}",
            l.dpi_cap
        ),
        Quality::Fixed(q) => format!("images at most {:.0} dpi, JPEG quality {q}", l.dpi_cap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use crate::testutil::Builder;

    /// A document with a photo-like raw image, text, and a duplicate of the image.
    fn photo_doc() -> Vec<u8> {
        let (w, h) = (400usize, 300usize);
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 31 + y * 17) % 3) as i32 - 1;
                px.push((128.0 + (x as f64 / 60.0).sin() * 60.0 + n as f64) as u8);
                px.push((128.0 + (y as f64 / 40.0).cos() * 60.0 + n as f64) as u8);
                px.push((128 + ((x + y) / 5) % 60) as u8);
            }
        }
        let mut b = Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
        b.obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.obj(
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] /Contents 4 0 R /Resources << /XObject << /A 5 0 R /B 6 0 R >> /Font << /F1 7 0 R >> >> >>",
        );
        let content = "q 200 0 0 150 0 0 cm /A Do Q BT /F1 12 Tf 10 10 Td (Hello compress) Tj ET q 20 0 0 15 0 0 cm /B Do Q\n".repeat(30);
        b.stream(4, "", content.as_bytes());
        let img = "/Type /XObject /Subtype /Image /Width 400 /Height 300 /ColorSpace /DeviceRGB /BitsPerComponent 8";
        b.stream(5, img, &px);
        b.stream(6, img, &px);
        b.obj(7, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        b.obj(8, "<< /Title (Secret title) /Author (Someone) >>");
        b.build(1, "/Info 8 0 R")
    }

    fn run(bytes: Vec<u8>, opts: Options) -> (Vec<u8>, Report) {
        let doc = Doc::open(Source::from_bytes(bytes)).unwrap();
        let pages = doc.pages().unwrap();
        struct Shared(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.borrow_mut().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let rep = compress(
            &doc,
            &pages,
            &opts,
            &CancelToken::new(),
            &mut |_, _| {},
            Box::new(Shared(buf.clone())),
        )
        .unwrap();
        let out = buf.borrow().clone();
        (out, rep)
    }

    #[test]
    fn smallest_recodes_dedupes_and_still_opens() {
        let input = photo_doc();
        let (out, rep) = run(input.clone(), Options::default());
        assert!(
            out.len() * 10 < input.len(),
            "{} -> {}",
            input.len(),
            out.len()
        );
        assert_eq!(rep.duplicates_removed, 1);
        assert!(rep.min_image_ssim.unwrap() >= SSIM_FLOOR);
        // The two identical images are stored once.
        assert_eq!(rep.images, 1);
        let back = Doc::open(Source::from_bytes(out)).unwrap();
        assert_eq!(back.pages().unwrap().len(), 1);
        assert!(back.trailer().get("Info").is_some());
    }

    #[test]
    fn strip_metadata_removes_the_information_dictionary() {
        let (out, _) = run(
            photo_doc(),
            Options {
                strip_metadata: true,
                ..Default::default()
            },
        );
        let back = Doc::open(Source::from_bytes(out)).unwrap();
        assert!(back.trailer().get("Info").is_none());
    }

    #[test]
    fn under_a_size_lands_below_the_target_and_reports_it() {
        let input = photo_doc();
        let (smallest, _) = run(input.clone(), Options::default());
        // Ask for less than the "smallest that still looks good" file: the dial goes lower.
        let target = (smallest.len() as f64 * 0.9) as u64;
        let (out, rep) = run(
            input.clone(),
            Options {
                target: Target::UnderBytes(target),
                ..Default::default()
            },
        );
        assert_eq!(rep.target_reached, Some(true), "{rep:?}");
        assert!(out.len() as u64 <= target, "{} vs {target}", out.len());
        assert!(rep.passes >= 3);
        // An impossible size is reported, not faked.
        let (_, rep) = run(
            input,
            Options {
                target: Target::UnderBytes(300),
                ..Default::default()
            },
        );
        assert_eq!(rep.target_reached, Some(false));
    }

    #[test]
    fn a_target_above_the_untouched_size_changes_no_pixels() {
        let input = photo_doc();
        let (_, rep) = run(
            input.clone(),
            Options {
                target: Target::UnderBytes(input.len() as u64 * 2),
                ..Default::default()
            },
        );
        assert_eq!(rep.target_reached, Some(true));
        assert_eq!(rep.images_lossy, 0);
    }

    #[test]
    fn grayscale_makes_colour_images_gray() {
        let (out, rep) = run(
            photo_doc(),
            Options {
                grayscale: true,
                ..Default::default()
            },
        );
        assert!(rep.images_lossy >= 1);
        let back = Doc::open(Source::from_bytes(out)).unwrap();
        let mut found = false;
        for n in back.used_objects() {
            if let Ok(l) = back.load(n)
                && let Obj::Dict(d) = &l.obj
                && d.name("Subtype") == Some(b"Image")
            {
                assert!(d.get("ColorSpace").unwrap().is_name("DeviceGray"));
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn cancel_is_honoured() {
        let doc = Doc::open(Source::from_bytes(photo_doc())).unwrap();
        let pages = doc.pages().unwrap();
        let c = CancelToken::new();
        c.cancel();
        let r = compress(
            &doc,
            &pages,
            &Options::default(),
            &c,
            &mut |_, _| {},
            Box::new(std::io::sink()),
        );
        assert!(matches!(r, Err(PdfError::Cancelled)));
    }
}
