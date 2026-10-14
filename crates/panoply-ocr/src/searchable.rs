//! `pdf.ocr`: make a scanned PDF searchable by adding an invisible text layer.
//!
//! The file is rebuilt with the PDF engine's writer and copier, the same way Organize keeps a
//! whole catalog (bookmarks, forms, metadata, page labels all survive). Every page is copied as
//! it was; the pages that get a text layer also get three small content streams around their own
//! (`q`, then the original streams untouched, then `Q`, then the text), so the page images and
//! the original content stream bytes are copied byte for byte. Pages that already have text are
//! copied and not read.
//!
//! Memory is one page at a time: the page is drawn as 8 bit gray at 300 DPI (about 9 MB for A4),
//! read by the engine ladder, and its bitmap dropped before the next page.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;

use panoply_core::CancelToken;
use panoply_pdf::Doc;
use panoply_pdf::copy::{Copier, Plain};
use panoply_pdf::doc::PageRef;
use panoply_pdf::error::PdfError;
use panoply_pdf::filters::deflate;
use panoply_pdf::obj::{Dict, Lexer, Obj, Tok};
use panoply_pdf::writer::{PdfWriter, WriterOptions};

use crate::Ocr;
use crate::error::{OcrError, Result};
use crate::lang::Lang;
use crate::models::Fetch;
use crate::textlayer::{FONT_NAME, FontBuilder, FontIds, PageBox, page_content, write_font};
use crate::types::{EngineKind, LOW_CONFIDENCE, OcrImage, OcrPage};

/// Text shown on a page, in bytes of strings, from which the page counts as already having a
/// text layer. A page number or a stamp is shorter than this.
pub const HAS_TEXT_BYTES: usize = 24;

/// What a job does.
#[derive(Debug, Clone)]
pub struct Options {
    /// Languages to read (empty: automatic).
    pub langs: Vec<Lang>,
    /// Resolution pages are drawn at for reading.
    pub dpi: f32,
    /// Most pixels one page may have at that resolution; larger pages are drawn smaller.
    pub max_pixels: usize,
    /// Also collect the text of the read pages, for a text file.
    pub collect_text: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            langs: Vec::new(),
            dpi: 300.0,
            max_pixels: 36_000_000,
            collect_text: false,
        }
    }
}

/// How one page was treated.
#[derive(Debug, Clone, PartialEq)]
pub enum PageStatus {
    /// It already had text, so it was copied unchanged.
    HadText,
    /// It was read: words found, and the mean confidence.
    Read {
        /// Words in the text layer.
        words: usize,
        /// Letter weighted mean confidence, 0 to 1.
        confidence: f32,
        /// The engine that read it.
        engine: EngineKind,
    },
    /// It was read and nothing was found (a blank or picture only page).
    NoText,
}

/// What a job did, for the result card.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Pages in the file.
    pub pages: usize,
    /// Pages read.
    pub read: usize,
    /// Pages that already had text.
    pub had_text: usize,
    /// Pages where nothing was found.
    pub no_text: usize,
    /// Words written to the text layer.
    pub words: usize,
    /// Zero based pages that are flagged: low confidence, or nothing found where there is a
    /// picture to read.
    pub flagged: Vec<usize>,
    /// Engines used, in the order first used.
    pub engines: Vec<EngineKind>,
    /// True when the input is digitally signed (the new file is not).
    pub signed: bool,
    /// Size of the output file in bytes, 0 when no file was written.
    pub bytes: u64,
    /// The text of the read pages when it was asked for: pages joined by a form feed.
    pub text: String,
    /// Per page status.
    pub per_page: Vec<PageStatus>,
    /// True when the input was damaged and repaired in memory.
    pub repaired: bool,
}

impl Report {
    /// Mean confidence over read pages (weighted by words), 0 when none.
    pub fn mean_confidence(&self) -> f32 {
        let (mut sum, mut n) = (0f32, 0usize);
        for p in &self.per_page {
            if let PageStatus::Read {
                words, confidence, ..
            } = p
            {
                sum += confidence * *words as f32;
                n += words;
            }
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }
}

// ---------------------------------------------------------------- does a page have text

/// Count the bytes of text strings a page shows (`Tj`, `TJ`, `'`, `"`), including text drawn
/// invisibly and text inside Form XObjects, to a small depth.
pub fn text_bytes(doc: &Doc, pr: &PageRef) -> usize {
    let Ok(loaded) = doc.load(pr.id) else {
        return 0;
    };
    let Obj::Dict(page) = loaded.obj else {
        return 0;
    };
    let resources = page
        .get("Resources")
        .or(pr.inherited.resources.as_ref())
        .and_then(|r| doc.resolve_dict(r))
        .unwrap_or_default();
    let content = doc.page_content(&page);
    scan(doc, &content, &resources, 0, &mut HashSet::new())
}

fn scan(
    doc: &Doc,
    content: &[u8],
    resources: &Dict,
    depth: usize,
    seen: &mut HashSet<u32>,
) -> usize {
    let mut total = 0usize;
    let mut lx = Lexer::content(content);
    let mut last_str = 0usize;
    let mut last_name: Option<Vec<u8>> = None;
    loop {
        match lx.next_tok() {
            Ok(Tok::End) | Err(_) => break,
            Ok(Tok::Obj(o)) => match &o {
                Obj::Str(s) => last_str = s.len(),
                Obj::Array(a) => {
                    last_str = a
                        .iter()
                        .map(|x| match x {
                            Obj::Str(s) => s.len(),
                            _ => 0,
                        })
                        .sum();
                }
                Obj::Name(n) => last_name = Some(n.clone()),
                _ => {}
            },
            Ok(Tok::Word(w)) => {
                match w {
                    b"Tj" | b"TJ" | b"'" | b"\"" => total += last_str,
                    b"BI" => {
                        // Skip inline image data: find `ID`, then the next white space delimited `EI`.
                        let rest = &content[lx.pos.min(content.len())..];
                        if let Some(id) = find(rest, b"ID") {
                            let after = &rest[id + 2..];
                            let mut at = 0;
                            while let Some(p) = find(&after[at..], b"EI") {
                                let abs = at + p;
                                let before_ok = abs == 0 || panoply_pdf::obj::is_ws(after[abs - 1]);
                                let after_ok = after
                                    .get(abs + 2)
                                    .is_none_or(|&b| panoply_pdf::obj::is_ws(b));
                                if before_ok && after_ok {
                                    lx.pos += id + 2 + abs + 2;
                                    break;
                                }
                                at = abs + 2;
                            }
                        }
                    }
                    b"Do" if depth < 3 => {
                        if let Some(name) = last_name.take()
                            && let Some(n) = form_ref(doc, resources, &name)
                            && seen.insert(n)
                            && let Ok(Some((fd, data))) = doc.stream_data(n)
                        {
                            let res = fd
                                .get("Resources")
                                .and_then(|r| doc.resolve_dict(r))
                                .unwrap_or_else(|| resources.clone());
                            total += scan(doc, &data, &res, depth + 1, seen);
                        }
                    }
                    _ => {}
                }
                last_str = 0;
                last_name = None;
            }
        }
        if total >= 4 * HAS_TEXT_BYTES {
            break;
        }
    }
    total
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The object number of a named Form XObject in the resources.
fn form_ref(doc: &Doc, resources: &Dict, name: &[u8]) -> Option<u32> {
    let xo = resources.get("XObject").and_then(|x| doc.resolve_dict(x))?;
    let key = String::from_utf8_lossy(name).into_owned();
    let obj = xo.get(&key)?;
    let n = obj.as_ref_num()?;
    let d = doc.load(n).ok()?;
    match d.obj {
        Obj::Dict(d) if d.name("Subtype") == Some(b"Form") => Some(n),
        _ => None,
    }
}

// ---------------------------------------------------------------- the job

fn pages_per_node(n: usize) -> usize {
    if n <= 64 {
        n.max(1)
    } else {
        ((n as f64).sqrt().ceil() as usize).max(32)
    }
}

fn map_pdf(e: PdfError) -> OcrError {
    OcrError::from(e)
}

/// Draws or loads the picture of page `index` (zero based) for reading. The scale (pixels per
/// point) is worked out from the page's own size, so the picture may be at any resolution.
pub type PageImage<'a> = &'a mut dyn FnMut(usize) -> Result<OcrImage>;

/// Make the PDF `file` searchable, drawing its pages with pdfium at `opts.dpi`.
pub fn make_searchable<W: Write>(
    file: &File,
    out: W,
    opts: &Options,
    ocr: &Ocr,
    fetch: &mut Fetch<'_>,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<Report> {
    let mut source: Option<crate::input::PageSource> = None;
    let mut draw = |i: usize| -> Result<OcrImage> {
        if source.is_none() {
            source = Some(crate::input::PageSource::open(file.try_clone()?)?);
        }
        let s = source.as_ref().expect("opened just above");
        Ok(s.render(i, opts.dpi, opts.max_pixels, cancel)?.0)
    };
    make_searchable_with(file, out, opts, ocr, fetch, cancel, progress, &mut draw)
}

/// Make `file` searchable, writing the new PDF to `out`; `image` supplies the picture of each
/// page to read (a PDF made from a picture reads the picture itself).
///
/// Returns the report. When no page needs reading (every page already has text), nothing is
/// written to `out` and the report has `bytes == 0`; the caller discards the empty output.
#[allow(clippy::too_many_arguments)]
pub fn make_searchable_with<W: Write>(
    file: &File,
    out: W,
    opts: &Options,
    ocr: &Ocr,
    fetch: &mut Fetch<'_>,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    image: PageImage<'_>,
) -> Result<Report> {
    let opened = panoply_pdf::open(file).map_err(map_pdf)?;
    let doc = &opened.doc;
    doc.require_unencrypted().map_err(map_pdf)?;
    let pages = doc.pages().map_err(map_pdf)?;
    if pages.is_empty() {
        return Err(OcrError::BadInput("This PDF has no pages.".into()));
    }
    let mut report = Report {
        pages: pages.len(),
        signed: doc.signatures().count > 0,
        repaired: opened.repaired,
        ..Report::default()
    };
    // Which pages already have text.
    progress(0.0, "Looking for text");
    let mut needs: Vec<bool> = Vec::with_capacity(pages.len());
    for (i, pr) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        needs.push(text_bytes(doc, pr) < HAS_TEXT_BYTES);
        if i % 64 == 0 {
            progress(0.05 * i as f32 / pages.len() as f32, "Looking for text");
        }
    }
    report.had_text = needs.iter().filter(|n| !**n).count();
    if !needs.iter().any(|n| *n) {
        report.per_page = vec![PageStatus::HadText; pages.len()];
        return Ok(report);
    }

    let mut w = PdfWriter::new(out, WriterOptions::default()).map_err(map_pdf)?;
    let catalog_id = w.alloc();
    let root_pages = w.alloc();
    let per_node = pages_per_node(pages.len());
    let node_count = if pages.len() <= 64 {
        0
    } else {
        pages.len().div_ceil(per_node)
    };
    let nodes: Vec<u32> = (0..node_count).map(|_| w.alloc()).collect();
    let page_ids: Vec<u32> = pages.iter().map(|_| w.alloc()).collect();
    let font_ids = FontIds::alloc(&mut w);
    let mut copier = Copier::new(doc, Plain, cancel.clone());
    for (pr, &dst) in pages.iter().zip(&page_ids) {
        copier.preassign(pr.id, dst);
    }

    let mut fonts = FontBuilder::new();
    let mut text_parts: Vec<String> = Vec::new();
    let todo = needs.iter().filter(|n| **n).count();
    let mut done = 0usize;
    for (i, pr) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let parent = if node_count == 0 {
            root_pages
        } else {
            nodes[i / per_node]
        };
        let mut extra: Option<[u32; 2]> = None;
        let mut status = PageStatus::HadText;
        if needs[i] {
            progress(
                0.05 + 0.9 * done as f32 / todo as f32,
                &format!("Reading page {} of {}", i + 1, pages.len()),
            );
            let geom = doc.page_geom(pr);
            let img = image(i)?;
            let scale = img.width as f32 / geom.display_size().0.max(1.0) as f32;
            let page = ocr.recognize(&img, &opts.langs, fetch, cancel)?;
            drop(img);
            status = if page.word_count() == 0 {
                PageStatus::NoText
            } else {
                PageStatus::Read {
                    words: page.word_count(),
                    confidence: page.mean_confidence(),
                    engine: page.engine,
                }
            };
            let box_ = PageBox {
                crop: geom.crop,
                rotate: geom.rotate,
            };
            let content = page_content(&page, scale, &box_, &mut fonts);
            if !content.is_empty() {
                let (pre, post) = (w.alloc(), w.alloc());
                w.put_stream(pre, &Dict::new(), b"q\n").map_err(map_pdf)?;
                let mut tail = b"Q\n".to_vec();
                tail.extend_from_slice(&content);
                let mut d = Dict::new();
                d.set("Filter", Obj::name("FlateDecode"));
                w.put_stream(post, &d, &deflate(&tail, 6))
                    .map_err(map_pdf)?;
                extra = Some([pre, post]);
            }
            note_page(&page, &status, i, &mut report, opts, &mut text_parts);
            done += 1;
        }
        report.per_page.push(status);
        write_page(
            &mut w,
            &mut copier,
            pr,
            page_ids[i],
            parent,
            extra,
            font_ids,
        )?;
    }
    report.read = done;

    // Page tree.
    let n = pages.len();
    if node_count == 0 {
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set(
            "Kids",
            Obj::Array(page_ids.iter().map(|&p| Obj::r(p)).collect()),
        );
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d)).map_err(map_pdf)?;
    } else {
        for (ni, &node) in nodes.iter().enumerate() {
            let lo = ni * per_node;
            let hi = ((ni + 1) * per_node).min(n);
            let mut d = Dict::new();
            d.set("Type", Obj::name("Pages"));
            d.set("Parent", Obj::r(root_pages));
            d.set(
                "Kids",
                Obj::Array(page_ids[lo..hi].iter().map(|&p| Obj::r(p)).collect()),
            );
            d.set("Count", Obj::Int((hi - lo) as i64));
            w.put(node, &Obj::Dict(d)).map_err(map_pdf)?;
        }
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set(
            "Kids",
            Obj::Array(nodes.iter().map(|&p| Obj::r(p)).collect()),
        );
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d)).map_err(map_pdf)?;
    }

    // Info and catalog, with everything the catalog reaches.
    let info = doc
        .trailer()
        .get("Info")
        .and_then(Obj::as_ref_num)
        .and_then(|n| copier.map_ref(&mut w, n));
    let mut src_cat = doc.catalog().map_err(map_pdf)?;
    src_cat.remove("Pages");
    let mut cat = copier.rewrite_dict(&mut w, &src_cat);
    cat.set("Pages", Obj::r(root_pages));
    w.put(catalog_id, &Obj::Dict(cat)).map_err(map_pdf)?;
    progress(0.96, "Copying page content");
    copier.drain(&mut w).map_err(map_pdf)?;
    write_font(&mut w, font_ids, &fonts).map_err(map_pdf)?;
    let seed: Vec<u8> = doc
        .file_len()
        .to_le_bytes()
        .into_iter()
        .chain((n as u64).to_le_bytes())
        .chain((fonts.len() as u64).to_le_bytes())
        .collect();
    report.bytes = w.finish(catalog_id, info, &seed).map_err(map_pdf)?;
    report.text = text_parts.join("\u{c}");
    for p in &report.per_page {
        if let PageStatus::Read { engine, .. } = p
            && !report.engines.contains(engine)
        {
            report.engines.push(*engine);
        }
    }
    progress(1.0, "Done");
    Ok(report)
}

fn note_page(
    page: &OcrPage,
    status: &PageStatus,
    index: usize,
    report: &mut Report,
    opts: &Options,
    text_parts: &mut Vec<String>,
) {
    report.words += page.word_count();
    match status {
        PageStatus::NoText => {
            report.no_text += 1;
            report.flagged.push(index);
        }
        PageStatus::Read { confidence, .. } => {
            if *confidence < LOW_CONFIDENCE {
                report.flagged.push(index);
            }
        }
        PageStatus::HadText => {}
    }
    if opts.collect_text {
        text_parts.push(crate::layout::plain_text(page, false));
    }
}

/// Copy one page, adding the text layer streams and the font to it when there are any.
fn write_page<W: Write>(
    w: &mut PdfWriter<W>,
    c: &mut Copier<'_, Plain>,
    pr: &PageRef,
    dst: u32,
    parent: u32,
    extra: Option<[u32; 2]>,
    font_ids: FontIds,
) -> Result<()> {
    let doc = c.doc;
    let mut d = match doc.load(pr.id).map_err(map_pdf)?.obj {
        Obj::Dict(d) => d,
        _ => return Err(OcrError::BadInput("A page is not a dictionary.".into())),
    };
    // Inherited attributes are written on the page, because its parent is not coming along.
    for (key, inh) in [
        ("Resources", &pr.inherited.resources),
        ("MediaBox", &pr.inherited.media_box),
        ("CropBox", &pr.inherited.crop_box),
        ("Rotate", &pr.inherited.rotate),
    ] {
        if !d.has(key)
            && let Some(v) = inh
        {
            d.set(key, v.clone());
        }
    }
    d.remove("Parent");
    d.set("Type", Obj::name("Page"));
    let original_contents = d.remove("Contents");
    let resources = d.get("Resources").and_then(|r| doc.resolve_dict(r));
    let mut out = c.rewrite_dict(w, &d);
    out.set("Parent", Obj::r(parent));
    if let Some(ids) = extra {
        // Contents: q, the original streams untouched, then Q and the text layer.
        let mut arr = vec![Obj::r(ids[0])];
        match original_contents.map(|o| c.rewrite(w, &o)) {
            Some(Obj::Array(a)) => arr.extend(a),
            Some(Obj::Null) | None => {}
            Some(o) => arr.push(o),
        }
        arr.push(Obj::r(ids[1]));
        out.set("Contents", Obj::Array(arr));
        // Resources: the page's own, with our font added.
        let mut res = resources.unwrap_or_default();
        let font_dict = res
            .get("Font")
            .and_then(|f| doc.resolve_dict(f))
            .unwrap_or_default();
        let mut font_out = c.rewrite_dict(w, &font_dict);
        let mut name = FONT_NAME.to_string();
        while font_out.has(&name) {
            name.push('_');
        }
        debug_assert_eq!(name, FONT_NAME, "font resource name clash");
        font_out.set(&name, Obj::r(font_ids.font));
        res.remove("Font");
        let mut res_out = c.rewrite_dict(w, &res);
        res_out.set("Font", Obj::Dict(font_out));
        out.set("Resources", Obj::Dict(res_out));
    } else if let Some(o) = original_contents {
        let o = c.rewrite(w, &o);
        out.set("Contents", o);
    }
    w.put(dst, &Obj::Dict(out)).map_err(map_pdf)
}

/// Pages the engine could not make sense of are listed for the result card: a short phrase
/// such as "pages 3, 7 and 9".
pub fn page_list(indices: &[usize]) -> String {
    let nums: Vec<String> = indices.iter().map(|i| (i + 1).to_string()).collect();
    match nums.as_slice() {
        [] => String::new(),
        [a] => format!("page {a}"),
        [a, b] => format!("pages {a} and {b}"),
        [rest @ .., last] => format!("pages {} and {last}", rest.join(", ")),
    }
}

/// Counts of words per engine, handy for reports and tests.
pub fn engine_counts(r: &Report) -> HashMap<EngineKind, usize> {
    let mut m = HashMap::new();
    for p in &r.per_page {
        if let PageStatus::Read { engine, .. } = p {
            *m.entry(*engine).or_insert(0) += 1;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_lists_read_naturally() {
        assert_eq!(page_list(&[]), "");
        assert_eq!(page_list(&[2]), "page 3");
        assert_eq!(page_list(&[2, 6]), "pages 3 and 7");
        assert_eq!(page_list(&[2, 6, 8]), "pages 3, 7 and 9");
    }

    #[test]
    fn a_page_of_text_is_found_and_a_blank_one_is_not() {
        let doc = Doc::open(panoply_pdf::source::Source::from_bytes(text_pdf(
            "Hello searchable world, this page has text on it",
        )))
        .unwrap();
        let pages = doc.pages().unwrap();
        assert!(text_bytes(&doc, &pages[0]) >= HAS_TEXT_BYTES);
        let blank = Doc::open(panoply_pdf::source::Source::from_bytes(text_pdf(""))).unwrap();
        let pages = blank.pages().unwrap();
        assert_eq!(text_bytes(&blank, &pages[0]), 0);
    }

    #[test]
    fn inline_image_bytes_do_not_confuse_the_scan() {
        // Binary data inside an inline image that contains text-operator lookalikes.
        let mut content =
            b"q BI /W 4 /H 1 /CS /G /BPC 8 ID Tj (zzzzzzzzzzzzzzzzzzzzzzzzzzzzzz) Tj\nEI Q "
                .to_vec();
        content.extend_from_slice(b"BT (real text that is long enough to count) Tj ET");
        let pdf = raw_pdf(&content);
        let doc = Doc::open(panoply_pdf::source::Source::from_bytes(pdf)).unwrap();
        let pages = doc.pages().unwrap();
        let n = text_bytes(&doc, &pages[0]);
        // Only the real string counts.
        assert_eq!(n, "real text that is long enough to count".len());
    }

    /// A one page PDF with a Helvetica font and `text` in its content.
    fn text_pdf(text: &str) -> Vec<u8> {
        let content = if text.is_empty() {
            String::new()
        } else {
            format!("BT /F1 12 Tf 20 100 Td ({text}) Tj ET")
        };
        raw_pdf(content.as_bytes())
    }

    fn raw_pdf(content: &[u8]) -> Vec<u8> {
        let objs: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            [
                format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
                content,
                b"\nendstream",
            ]
            .concat(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}
