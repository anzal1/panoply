//! `pdf.redact`: true redaction. Pages that hold a match are redrawn as a picture with black
//! boxes burned into the pixels, plus an invisible text layer that lacks the redacted words.
//! Nothing of the original page (its text runs, its pictures, its annotations) is copied.
//! Everything else in the file that holds a match (annotations, form fields, document info,
//! metadata, attachments, bookmarks) is dropped, the file is written fresh with no earlier
//! revisions, and the result is checked before it is handed over.
//!
//! Why a redrawn page and not an edited content stream: see "Redaction redraws the page" in
//! `docs/DECISIONS.md`.
//!
//! Matching works on one page text: the characters pdfium reports with their boxes, or, for a
//! scan, the OCR words (their boxes shared out over their letters).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;

use panoply_core::CancelToken;
use panoply_pdf::Doc;
use panoply_pdf::copy::{Copier, Plain};
use panoply_pdf::doc::{PageGeom, PageRef};
use panoply_pdf::filters::deflate;
use panoply_pdf::obj::{Dict, Obj};
use panoply_pdf::writer::{PdfWriter, WriterOptions};

use crate::Ocr;
use crate::error::{OcrError, Result};
use crate::lang::Lang;
use crate::models::Fetch;
use crate::textlayer::{FONT_NAME, FontBuilder, FontIds, PageBox, page_content, write_font};
use crate::types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};

pub mod patterns;
pub use patterns::{Matcher, Preset};

/// Fewer non white space characters than this on a page means it is a scan.
const MIN_TEXT_CHARS: usize = 4;

/// What to look for and how to redraw.
#[derive(Debug, Clone)]
pub struct Options {
    /// Exact terms (ignoring case).
    pub terms: Vec<String>,
    /// Built in shapes.
    pub presets: Vec<Preset>,
    /// Resolution redacted pages are redrawn at.
    pub dpi: f32,
    /// Most pixels one redrawn page may have.
    pub max_pixels: usize,
    /// OCR languages for scanned pages (empty: automatic).
    pub langs: Vec<Lang>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            terms: Vec::new(),
            presets: Vec::new(),
            dpi: 200.0,
            max_pixels: 36_000_000,
            langs: Vec::new(),
        }
    }
}

/// Where a match is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// On a page (zero based).
    Page(usize),
    /// In an object outside the page content.
    Object {
        /// Object number in the source file.
        num: u32,
        /// What it is, in words: `annotation`, `form field`, `metadata`, ...
        what: &'static str,
    },
}

/// One thing found, to show in the preview.
#[derive(Debug, Clone)]
pub struct Match {
    /// Position in the list, stable for one file and one set of options.
    pub id: usize,
    /// `term` or a preset id.
    pub kind: String,
    /// The matched text.
    pub text: String,
    /// Where it is.
    pub at: Where,
    /// Boxes to black out, in points on the displayed page (origin top left). Empty for objects.
    pub rects: Vec<Rect>,
    /// False when the user unticked it in the preview.
    pub selected: bool,
}

/// The result of looking.
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// Pages in the file.
    pub pages: usize,
    /// Pages that had to be read with OCR.
    pub scanned: Vec<usize>,
    /// Everything found.
    pub matches: Vec<Match>,
}

/// What an apply did.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Pages in the file.
    pub pages: usize,
    /// Zero based pages that were redrawn.
    pub redacted_pages: Vec<usize>,
    /// Matches applied.
    pub applied: usize,
    /// Objects dropped from the file, by kind.
    pub dropped: Vec<(u32, &'static str)>,
    /// Size of the output.
    pub bytes: u64,
    /// True when the input was digitally signed (the output is not).
    pub signed: bool,
    /// True when the input was damaged and repaired in memory.
    pub repaired: bool,
    /// Strings that must not be in the output, for [`verify`].
    pub needles: Vec<String>,
}

// ---------------------------------------------------------------- geometry

fn page_box(g: &PageGeom) -> PageBox {
    PageBox {
        crop: g.crop,
        rotate: g.rotate,
    }
}

/// The inverse of [`PageBox::to_user`]: user space to displayed points, y down.
fn to_display(b: &PageBox, x: f64, y: f64) -> (f64, f64) {
    let [x0, y0, x1, y1] = b.crop;
    let (w, h) = (x1 - x0, y1 - y0);
    match b.rotate {
        90 => (y - y0, x - x0),
        180 => (x0 + w - x, y - y0),
        270 => (y0 + h - y, x0 + w - x),
        _ => (x - x0, y1 - y),
    }
}

fn union(a: &Rect, b: &Rect) -> Rect {
    a.union(b)
}

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() - 0.25
        && b.x < a.right() - 0.25
        && a.y < b.bottom() - 0.25
        && b.y < a.bottom() - 0.25
}

// ---------------------------------------------------------------- pdfium

struct Pdfium {
    doc: Option<pdfium_render::prelude::PdfDocument<'static>>,
}

impl Drop for Pdfium {
    fn drop(&mut self) {
        let _g = panoply_pdf::pdfium::lock();
        self.doc.take();
    }
}

/// A rendered page: BGR to RGB, with its scale.
struct Drawn {
    w: u32,
    h: u32,
    rgb: Vec<u8>,
    scale: f32,
}

#[derive(Clone)]
struct Ch {
    c: char,
    r: Rect,
}

impl Pdfium {
    fn open(file: &File) -> Result<Pdfium> {
        let engine = panoply_pdf::pdfium::engine()?;
        let _g = panoply_pdf::pdfium::lock();
        let doc = engine
            .load_pdf_from_reader(file.try_clone()?, None)
            .map_err(|e| {
                OcrError::BadInput(format!("The PDF engine could not open this file ({e:?})."))
            })?;
        Ok(Pdfium { doc: Some(doc) })
    }

    fn doc(&self) -> &pdfium_render::prelude::PdfDocument<'static> {
        self.doc.as_ref().expect("held until dropped")
    }

    /// The characters of a page with their boxes in displayed points.
    fn chars(&self, index: usize, b: &PageBox) -> Result<Vec<Ch>> {
        let _g = panoply_pdf::pdfium::lock();
        let page = self.doc().pages().get(index as i32).map_err(|e| {
            OcrError::BadInput(format!("Page {} could not be opened ({e:?}).", index + 1))
        })?;
        // Not `all()`: it drops characters on rotated pages.
        let Ok(text) = page.text() else {
            return Ok(Vec::new());
        };
        let mut out: Vec<Ch> = Vec::new();
        for ch in text.chars().iter() {
            let Some(c) = ch.unicode_char() else { continue };
            let rect = ch.loose_bounds().ok().map(|r| {
                let (x0, y0) = to_display(b, f64::from(r.left().value), f64::from(r.top().value));
                let (x1, y1) =
                    to_display(b, f64::from(r.right().value), f64::from(r.bottom().value));
                Rect {
                    x: x0.min(x1) as f32,
                    y: y0.min(y1) as f32,
                    w: (x0 - x1).abs() as f32,
                    h: (y0 - y1).abs() as f32,
                }
            });
            if c.is_whitespace() || c.is_control() || c == '\u{fffe}' {
                let r = out.last().map_or(Rect::default(), |p| Rect {
                    x: p.r.right(),
                    y: p.r.y,
                    w: 0.0,
                    h: p.r.h,
                });
                out.push(Ch { c: ' ', r });
            } else if let Some(r) = rect {
                out.push(Ch { c, r });
            }
        }
        Ok(out)
    }

    /// Draw a page at `dpi` (at most `max_pixels`), with annotations and form values.
    fn render(&self, index: usize, dpi: f32, max_pixels: usize) -> Result<Drawn> {
        use pdfium_render::prelude::*;
        let _g = panoply_pdf::pdfium::lock();
        let page = self.doc().pages().get(index as i32).map_err(|e| {
            OcrError::BadInput(format!("Page {} could not be opened ({e:?}).", index + 1))
        })?;
        let (pw, ph) = (page.width().value.max(1.0), page.height().value.max(1.0));
        let mut scale = dpi / 72.0;
        let px = |k: f32| ((pw * k).round() as usize).max(1) * ((ph * k).round() as usize).max(1);
        if px(scale) > max_pixels {
            scale *= (max_pixels as f32 / px(scale) as f32).sqrt();
        }
        let cfg = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .set_format(PdfBitmapFormat::BGR)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE)
            .render_form_data(true)
            .render_annotations(true);
        let bmp = page.render_with_config(&cfg).map_err(|e| {
            OcrError::Engine(format!(
                "The PDF engine could not draw page {} ({e:?}).",
                index + 1
            ))
        })?;
        let (w, h) = (bmp.width() as usize, bmp.height() as usize);
        let raw = bmp.as_raw_bytes();
        let stride = raw.len() / h.max(1);
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for p in raw[y * stride..y * stride + w * 3].chunks_exact(3) {
                rgb.extend_from_slice(&[p[2], p[1], p[0]]);
            }
        }
        Ok(Drawn {
            w: w as u32,
            h: h as u32,
            rgb,
            scale: w as f32 / pw,
        })
    }
}

// ---------------------------------------------------------------- page text

struct PageText {
    chars: Vec<Ch>,
    scanned: bool,
}

/// Characters from OCR words: each word's box is shared out over its letters, words are
/// separated by a space.
fn chars_from_ocr(page: &OcrPage, scale: f32) -> Vec<Ch> {
    let mut out: Vec<Ch> = Vec::new();
    let k = scale.max(1e-3);
    for line in &page.lines {
        for word in &line.words {
            let n = word.text.chars().count().max(1) as f32;
            let b = word.bbox;
            if !out.is_empty() {
                let prev = out.last().expect("not empty").r;
                out.push(Ch {
                    c: ' ',
                    r: Rect {
                        x: prev.right(),
                        y: prev.y,
                        w: (b.x / k - prev.right()).max(0.0),
                        h: prev.h,
                    },
                });
            }
            for (i, c) in word.text.chars().enumerate() {
                out.push(Ch {
                    c,
                    r: Rect {
                        x: (b.x + b.w * i as f32 / n) / k,
                        y: b.y / k,
                        w: b.w / n / k,
                        h: b.h / k,
                    },
                });
            }
        }
    }
    out
}

fn text_of(chars: &[Ch]) -> (String, Vec<usize>) {
    let mut s = String::with_capacity(chars.len());
    let mut at = Vec::with_capacity(chars.len() * 2);
    for (i, c) in chars.iter().enumerate() {
        for _ in 0..c.c.len_utf8() {
            at.push(i);
        }
        s.push(c.c);
    }
    at.push(chars.len());
    (s, at)
}

/// Boxes for the characters `lo..hi`: one per run on a line, padded a little.
fn rects_for(chars: &[Ch], lo: usize, hi: usize) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    let mut cur: Option<Rect> = None;
    for ch in &chars[lo..hi] {
        if ch.c == ' ' && ch.r.w == 0.0 {
            continue;
        }
        cur = Some(match cur {
            Some(c)
                if (ch.r.y + ch.r.h / 2.0) > c.y
                    && (ch.r.y + ch.r.h / 2.0) < c.bottom()
                    && ch.r.x >= c.x - 1.0 =>
            {
                union(&c, &ch.r)
            }
            Some(c) => {
                out.push(c);
                ch.r
            }
            None => ch.r,
        });
    }
    out.extend(cur);
    for r in &mut out {
        let pad = (r.h * 0.1).max(1.0);
        r.x -= pad;
        r.y -= pad * 0.6;
        r.w += 2.0 * pad;
        r.h += pad * 1.2;
    }
    out
}

/// A page's text: pdfium's, or OCR for a scan.
#[allow(clippy::too_many_arguments)]
fn read_page(
    pdfium: &Pdfium,
    index: usize,
    b: &PageBox,
    opts: &Options,
    ocr: &Ocr,
    fetch: &mut Fetch<'_>,
    cancel: &CancelToken,
) -> Result<PageText> {
    let chars = pdfium.chars(index, b)?;
    if chars.iter().filter(|c| !c.c.is_whitespace()).count() >= MIN_TEXT_CHARS {
        return Ok(PageText {
            chars,
            scanned: false,
        });
    }
    let drawn = pdfium.render(index, 300.0, crate::input::MAX_PIXELS)?;
    let img = OcrImage::rgb(drawn.w, drawn.h, drawn.rgb)
        .ok_or_else(|| OcrError::Engine("The page could not be drawn for reading.".into()))?;
    let page = ocr.recognize(&img, &opts.langs, fetch, cancel)?;
    Ok(PageText {
        chars: chars_from_ocr(&page, drawn.scale),
        scanned: true,
    })
}

// ---------------------------------------------------------------- non page objects

fn decode_pdf_string(b: &[u8]) -> String {
    if b.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

fn collect_strings(o: &Obj, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match o {
        Obj::Str(s) => {
            let t = decode_pdf_string(s);
            if !t.trim().is_empty() {
                out.push(t);
            }
        }
        Obj::Array(a) => a.iter().for_each(|x| collect_strings(x, out, depth + 1)),
        Obj::Dict(d) => d
            .iter()
            .for_each(|(_, v)| collect_strings(v, out, depth + 1)),
        _ => {}
    }
}

/// The text of an object outside page content, and what it is. `None` for objects that never
/// hold a user's text (page content, pictures, fonts, ...).
fn object_text(
    doc: &Doc,
    num: u32,
    skip: &HashSet<u32>,
    info: Option<u32>,
) -> Option<(String, &'static str)> {
    if skip.contains(&num) {
        return None;
    }
    let l = doc.load(num).ok()?;
    let Obj::Dict(d) = &l.obj else { return None };
    let mut parts: Vec<String> = Vec::new();
    let mut what = "document";
    if l.stream.is_some() {
        let ty = d.name("Type");
        let sub = d.name("Subtype");
        if sub == Some(b"Image") || sub == Some(b"Form") {
            return None;
        }
        if ty == Some(b"Metadata") || sub == Some(b"XML") {
            what = "metadata";
        } else if ty == Some(b"EmbeddedFile") {
            what = "attachment";
        } else {
            return None;
        }
        let (_, data) = doc.stream_data(num).ok()??;
        parts.push(String::from_utf8_lossy(&data).into_owned());
    } else {
        if d.has("Type") && d.name("Type") == Some(b"Page") {
            return None;
        }
        if d.has("FT") || d.name("Subtype") == Some(b"Widget") {
            what = "form field";
        } else if d.has("Rect") && d.has("Subtype") {
            what = "annotation";
        } else if d.has("EF") || d.name("Type") == Some(b"Filespec") {
            what = "attachment";
        } else if d.has("Title") && (d.has("Parent") || d.has("First")) {
            what = "bookmark";
        } else if Some(num) == info {
            what = "document info";
        }
        collect_strings(&l.obj, &mut parts, 0);
        if let Some(ef) = d.get("EF").and_then(|e| doc.resolve_dict(e))
            && let Some(n) = ef.get("F").and_then(Obj::as_ref_num)
            && let Ok(Some((_, data))) = doc.stream_data(n)
        {
            parts.push(String::from_utf8_lossy(&data).into_owned());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some((parts.join("\n"), what))
    }
}

fn page_skip_set(doc: &Doc, pages: &[PageRef]) -> HashSet<u32> {
    let mut skip: HashSet<u32> = pages.iter().map(|p| p.id).collect();
    for pr in pages {
        if let Ok(l) = doc.load(pr.id)
            && let Obj::Dict(d) = l.obj
            && let Some(c) = d.get("Contents")
        {
            match doc.resolve(c) {
                Obj::Array(a) => skip.extend(a.iter().filter_map(Obj::as_ref_num)),
                _ => skip.extend(c.as_ref_num()),
            }
        }
    }
    skip
}

// ---------------------------------------------------------------- find

/// Look for matches. Nothing is written.
pub fn find(
    file: &File,
    opts: &Options,
    ocr: &Ocr,
    fetch: &mut Fetch<'_>,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<Found> {
    let matcher = Matcher::new(&opts.terms, &opts.presets).map_err(OcrError::BadInput)?;
    if matcher.is_empty() {
        return Err(OcrError::BadInput(
            "Give at least one term or choose a preset to look for.".into(),
        ));
    }
    let opened = panoply_pdf::open(file)?;
    let doc = &opened.doc;
    doc.require_unencrypted()?;
    let pages = doc.pages()?;
    if pages.is_empty() {
        return Err(OcrError::BadInput("This PDF has no pages.".into()));
    }
    let pdfium = Pdfium::open(file)?;
    let mut found = Found {
        pages: pages.len(),
        ..Found::default()
    };
    for (i, pr) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        progress(
            0.9 * i as f32 / pages.len() as f32,
            &format!("Looking at page {} of {}", i + 1, pages.len()),
        );
        let geom = doc.page_geom(pr);
        let b = page_box(&geom);
        let page = read_page(&pdfium, i, &b, opts, ocr, fetch, cancel)?;
        if page.scanned {
            found.scanned.push(i);
        }
        let (text, at) = text_of(&page.chars);
        for hit in matcher.find(&text) {
            let (lo, hi) = (at[hit.range.start], at[hit.range.end]);
            let id = found.matches.len();
            found.matches.push(Match {
                id,
                kind: hit.kind.to_string(),
                text: text[hit.range].to_string(),
                at: Where::Page(i),
                rects: rects_for(&page.chars, lo, hi),
                selected: true,
            });
        }
    }
    progress(
        0.92,
        "Looking at annotations, forms, metadata and attachments",
    );
    let skip = page_skip_set(doc, &pages);
    let info = doc.trailer().get("Info").and_then(Obj::as_ref_num);
    for num in doc.used_objects() {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let Some((text, what)) = object_text(doc, num, &skip, info) else {
            continue;
        };
        for hit in matcher.find(&text) {
            let id = found.matches.len();
            found.matches.push(Match {
                id,
                kind: hit.kind.to_string(),
                text: text[hit.range].to_string(),
                at: Where::Object { num, what },
                rects: Vec::new(),
                selected: true,
            });
        }
    }
    progress(1.0, "Done");
    Ok(found)
}

// ---------------------------------------------------------------- apply

fn needle_forms(text: &str) -> Vec<String> {
    let t = text.to_lowercase();
    let compact: String = t
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    let mut v = vec![t];
    if compact.len() >= 8 && compact != v[0] {
        v.push(compact);
    }
    v
}

fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

fn contains_any(hay: &str, needles: &[String]) -> bool {
    let low = hay.to_lowercase();
    let comp = compact(&low);
    needles.iter().any(|n| {
        low.contains(n.as_str()) || (n.len() >= 8 && !n.contains(' ') && comp.contains(n.as_str()))
    })
}

fn pages_per_node(n: usize) -> usize {
    if n <= 64 {
        n.max(1)
    } else {
        ((n as f64).sqrt().ceil() as usize).max(32)
    }
}

fn map_pdf(e: panoply_pdf::PdfError) -> OcrError {
    OcrError::from(e)
}

/// Black out the boxes (displayed points) in an RGB picture drawn at `scale` pixels a point.
fn burn(d: &mut Drawn, rects: &[Rect]) {
    for r in rects {
        let x0 = ((r.x * d.scale).floor().max(0.0) as u32).min(d.w);
        let y0 = ((r.y * d.scale).floor().max(0.0) as u32).min(d.h);
        let x1 = ((r.right() * d.scale).ceil().max(0.0) as u32).min(d.w);
        let y1 = ((r.bottom() * d.scale).ceil().max(0.0) as u32).min(d.h);
        for y in y0..y1 {
            let row = (y as usize * d.w as usize + x0 as usize) * 3;
            let n = (x1.saturating_sub(x0)) as usize * 3;
            d.rgb[row..row + n].fill(0);
        }
    }
}

/// Write a redacted copy of `file` to `out`. `all` is the list from [`find`], with the user's
/// choices in `selected`.
#[allow(clippy::too_many_arguments)]
pub fn apply<W: Write>(
    file: &File,
    out: W,
    opts: &Options,
    all: &[Match],
    ocr: &Ocr,
    fetch: &mut Fetch<'_>,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<Report> {
    let chosen: Vec<&Match> = all.iter().filter(|m| m.selected).collect();
    if chosen.is_empty() {
        return Err(OcrError::BadInput("Nothing is selected to redact.".into()));
    }
    let opened = panoply_pdf::open(file)?;
    let doc = &opened.doc;
    doc.require_unencrypted()?;
    let pages = doc.pages()?;
    let mut report = Report {
        pages: pages.len(),
        signed: doc.signatures().count > 0,
        repaired: opened.repaired,
        applied: chosen.len(),
        ..Report::default()
    };
    // What must be gone: the chosen texts, except what the user chose to keep elsewhere.
    let kept: HashSet<String> = all
        .iter()
        .filter(|m| !m.selected)
        .flat_map(|m| needle_forms(&m.text))
        .collect();
    let mut needles: Vec<String> = chosen
        .iter()
        .flat_map(|m| needle_forms(&m.text))
        .filter(|n| !kept.contains(n))
        .collect();
    needles.sort();
    needles.dedup();
    report.needles = needles.clone();

    // Boxes per page, and the objects to drop.
    let mut boxes: HashMap<usize, Vec<Rect>> = HashMap::new();
    let mut drop_objs: HashMap<u32, &'static str> = HashMap::new();
    for m in &chosen {
        match &m.at {
            Where::Page(p) => boxes.entry(*p).or_default().extend(m.rects.iter().copied()),
            Where::Object { num, what } => {
                drop_objs.insert(*num, what);
            }
        }
    }
    let kept_objs: HashSet<u32> = all
        .iter()
        .filter(|m| !m.selected)
        .filter_map(|m| match &m.at {
            Where::Object { num, .. } => Some(*num),
            Where::Page(_) => None,
        })
        .collect();
    let skip = page_skip_set(doc, &pages);
    let info = doc.trailer().get("Info").and_then(Obj::as_ref_num);
    for num in doc.used_objects() {
        if drop_objs.contains_key(&num) || kept_objs.contains(&num) {
            continue;
        }
        if let Some((text, what)) = object_text(doc, num, &skip, info)
            && contains_any(&text, &needles)
        {
            drop_objs.insert(num, what);
        }
    }

    let pdfium = Pdfium::open(file)?;
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
    copier.prune(drop_objs.keys().copied());
    let mut fonts = FontBuilder::new();
    let todo = boxes.len().max(1);
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
        if let Some(rects) = boxes.get(&i) {
            progress(
                0.05 + 0.85 * done as f32 / todo as f32,
                &format!("Redacting page {} of {}", i + 1, pages.len()),
            );
            let geom = doc.page_geom(pr);
            let b = page_box(&geom);
            let (dw, dh) = geom.display_size();
            let chars = pdfium.chars(i, &b)?;
            let has_text = chars.iter().filter(|c| !c.c.is_whitespace()).count() >= MIN_TEXT_CHARS;
            let mut drawn = pdfium.render(i, opts.dpi, opts.max_pixels)?;
            burn(&mut drawn, rects);
            // The text layer: pdfium's words minus any that touch a box, or, for a scan, the
            // OCR of the picture as it now is (the redacted words are not in it).
            let words: Vec<Word> = if has_text {
                words_from_chars(&chars, rects, drawn.scale)
            } else {
                let img = OcrImage::rgb(drawn.w, drawn.h, drawn.rgb.clone()).ok_or_else(|| {
                    OcrError::Engine("The page could not be drawn for reading.".into())
                })?;
                let read = ocr.recognize(&img, &opts.langs, fetch, cancel)?;
                read.lines.into_iter().flat_map(|l| l.words).collect()
            };
            let page = OcrPage {
                width: drawn.w,
                height: drawn.h,
                lines: words
                    .into_iter()
                    .filter_map(|w| Line::from_words(vec![w]))
                    .collect(),
                engine: EngineKind::PpOcr,
                languages: Vec::new(),
            };
            let flat = PageBox {
                crop: [0.0, 0.0, dw, dh],
                rotate: 0,
            };
            let text_layer = page_content(&page, drawn.scale, &flat, &mut fonts);
            let jpg = panoply_pdf::jpeg::encode(
                &drawn.rgb,
                drawn.w as usize,
                drawn.h as usize,
                3,
                panoply_pdf::jpeg::EncodeOptions {
                    tables: panoply_pdf::jpeg::Tables::Standard,
                    quality: 85,
                    progressive: false,
                    subsample: false,
                },
            )
            .ok_or_else(|| OcrError::Engine("The picture encoder failed.".into()))?;
            let (img_id, content_id) = (w.alloc(), w.alloc());
            let mut idict = Dict::new();
            idict.set("Type", Obj::name("XObject"));
            idict.set("Subtype", Obj::name("Image"));
            idict.set("Width", Obj::Int(i64::from(drawn.w)));
            idict.set("Height", Obj::Int(i64::from(drawn.h)));
            idict.set("ColorSpace", Obj::name("DeviceRGB"));
            idict.set("BitsPerComponent", Obj::Int(8));
            idict.set("Filter", Obj::name("DCTDecode"));
            w.put_stream(img_id, &idict, &jpg).map_err(map_pdf)?;
            let mut content = format!("q {dw:.3} 0 0 {dh:.3} 0 0 cm /Im0 Do Q\n").into_bytes();
            content.extend_from_slice(&text_layer);
            let mut cd = Dict::new();
            cd.set("Filter", Obj::name("FlateDecode"));
            w.put_stream(content_id, &cd, &deflate(&content, 6))
                .map_err(map_pdf)?;
            let mut res = Dict::new();
            let mut xo = Dict::new();
            xo.set("Im0", Obj::r(img_id));
            res.set("XObject", Obj::Dict(xo));
            let mut fd = Dict::new();
            fd.set(FONT_NAME, Obj::r(font_ids.font));
            res.set("Font", Obj::Dict(fd));
            let mut pd = Dict::new();
            pd.set("Type", Obj::name("Page"));
            pd.set("Parent", Obj::r(parent));
            pd.set(
                "MediaBox",
                Obj::Array(vec![Obj::Int(0), Obj::Int(0), Obj::Real(dw), Obj::Real(dh)]),
            );
            pd.set("Resources", Obj::Dict(res));
            pd.set("Contents", Obj::r(content_id));
            w.put(page_ids[i], &Obj::Dict(pd)).map_err(map_pdf)?;
            report.redacted_pages.push(i);
            done += 1;
        } else {
            write_page(&mut w, &mut copier, pr, page_ids[i], parent)?;
        }
    }

    // Page tree.
    let n = pages.len();
    let kids = |ids: &[u32]| Obj::Array(ids.iter().map(|&p| Obj::r(p)).collect());
    if node_count == 0 {
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set("Kids", kids(&page_ids));
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d)).map_err(map_pdf)?;
    } else {
        for (ni, &node) in nodes.iter().enumerate() {
            let (lo, hi) = (ni * per_node, ((ni + 1) * per_node).min(n));
            let mut d = Dict::new();
            d.set("Type", Obj::name("Pages"));
            d.set("Parent", Obj::r(root_pages));
            d.set("Kids", kids(&page_ids[lo..hi]));
            d.set("Count", Obj::Int((hi - lo) as i64));
            w.put(node, &Obj::Dict(d)).map_err(map_pdf)?;
        }
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set("Kids", kids(&nodes));
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d)).map_err(map_pdf)?;
    }

    // Info and catalog.
    let info_out = info.and_then(|n| copier.map_ref(&mut w, n));
    let mut src_cat = doc.catalog().map_err(map_pdf)?;
    src_cat.remove("Pages");
    let mut cat = copier.rewrite_dict(&mut w, &src_cat);
    cat.set("Pages", Obj::r(root_pages));
    // A reference to a dropped object came out as null: remove the entry, and clean arrays.
    scrub_nulls(&mut cat);
    w.put(catalog_id, &Obj::Dict(cat)).map_err(map_pdf)?;
    progress(0.95, "Copying the rest of the file");
    copier.drain(&mut w).map_err(map_pdf)?;
    write_font(&mut w, font_ids, &fonts).map_err(map_pdf)?;
    let seed: Vec<u8> = doc
        .file_len()
        .to_le_bytes()
        .into_iter()
        .chain((n as u64).to_le_bytes())
        .chain((report.applied as u64).to_le_bytes())
        .collect();
    report.bytes = w.finish(catalog_id, info_out, &seed).map_err(map_pdf)?;
    report.dropped = drop_objs.into_iter().collect();
    report.dropped.sort_by_key(|d| d.0);
    progress(1.0, "Done");
    Ok(report)
}

/// Words from pdfium characters, leaving out any word that touches a box.
fn words_from_chars(chars: &[Ch], boxes: &[Rect], scale: f32) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cur: Vec<&Ch> = Vec::new();
    let mut flush = |cur: &mut Vec<&Ch>| {
        if cur.is_empty() {
            return;
        }
        let bbox = cur.iter().skip(1).fold(cur[0].r, |a, c| union(&a, &c.r));
        let text: String = cur.iter().map(|c| c.c).collect();
        if !boxes.iter().any(|r| intersects(r, &bbox)) && bbox.w > 0.0 && bbox.h > 0.0 {
            words.push(Word {
                text,
                bbox: Rect {
                    x: bbox.x * scale,
                    y: bbox.y * scale,
                    w: bbox.w * scale,
                    h: bbox.h * scale,
                },
                confidence: 1.0,
            });
        }
        cur.clear();
    };
    for c in chars {
        let gap = cur.last().is_some_and(|p| {
            c.r.x < p.r.x - 1.0
                || (c.r.y + c.r.h / 2.0) > p.r.bottom() + 1.0
                || (c.r.y + c.r.h / 2.0) < p.r.y - 1.0
        });
        if c.c == ' ' || gap {
            flush(&mut cur);
        }
        if c.c != ' ' {
            cur.push(c);
        }
    }
    flush(&mut cur);
    words
}

fn scrub_nulls(d: &mut Dict) {
    let null_keys: Vec<Vec<u8>> = d
        .iter()
        .filter(|(_, v)| matches!(v, Obj::Null))
        .map(|(k, _)| k.to_vec())
        .collect();
    for k in null_keys {
        d.remove(&String::from_utf8_lossy(&k));
    }
    for v in d.values_mut() {
        scrub_obj(v);
    }
}

fn scrub_obj(o: &mut Obj) {
    match o {
        Obj::Array(a) => {
            a.retain(|x| !matches!(x, Obj::Null));
            a.iter_mut().for_each(scrub_obj);
        }
        Obj::Dict(d) => scrub_nulls(d),
        _ => {}
    }
}

/// Copy a page that has nothing to redact: its own attributes, with inherited ones written on
/// it, and its annotations and everything else through the copier.
fn write_page<W: Write>(
    w: &mut PdfWriter<W>,
    c: &mut Copier<'_, Plain>,
    pr: &PageRef,
    dst: u32,
    parent: u32,
) -> Result<()> {
    let doc = c.doc;
    let mut d = match doc.load(pr.id).map_err(map_pdf)?.obj {
        Obj::Dict(d) => d,
        _ => return Err(OcrError::BadInput("A page is not a dictionary.".into())),
    };
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
    let mut out = c.rewrite_dict(w, &d);
    out.set("Parent", Obj::r(parent));
    scrub_nulls(&mut out);
    w.put(dst, &Obj::Dict(out)).map_err(map_pdf)
}

// ---------------------------------------------------------------- verify

/// One place a redacted string was still found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leak {
    /// Where: `page 3 text`, `object 12 (decoded)`, `file bytes`.
    pub place: String,
}

/// Check a written file: none of the `needles` may be in the text pdfium extracts, in any
/// decoded stream or string, or in the raw bytes. An empty list means clean.
pub fn verify(file: &File, needles: &[String]) -> Result<Vec<Leak>> {
    let mut leaks = Vec::new();
    let opened = panoply_pdf::open(file)?;
    let doc = &opened.doc;
    let pages = doc.pages()?;
    let pdfium = Pdfium::open(file)?;
    for (i, pr) in pages.iter().enumerate() {
        let b = page_box(&doc.page_geom(pr));
        let chars = pdfium.chars(i, &b)?;
        let text: String = chars.iter().map(|c| c.c).collect();
        if contains_any(&text, needles) {
            leaks.push(Leak {
                place: format!("page {} text", i + 1),
            });
        }
    }
    for num in doc.used_objects() {
        let Ok(l) = doc.load(num) else { continue };
        let mut strs = Vec::new();
        collect_strings(&l.obj, &mut strs, 0);
        if strs.iter().any(|s| contains_any(s, needles)) {
            leaks.push(Leak {
                place: format!("object {num} (strings)"),
            });
        }
        if let Obj::Dict(d) = &l.obj
            && l.stream.is_some()
            && d.name("Subtype") != Some(b"Image")
            && let Ok(Some((_, data))) = doc.stream_data(num)
            && contains_any(&String::from_utf8_lossy(&data), needles)
        {
            leaks.push(Leak {
                place: format!("object {num} (decoded stream)"),
            });
        }
    }
    let mut f = file.try_clone()?;
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(0))?;
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut raw)?;
    let low = raw.to_ascii_lowercase();
    for n in needles {
        let ascii = n.as_bytes();
        let utf16: Vec<u8> = n.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let has = |h: &[u8], p: &[u8]| !p.is_empty() && h.windows(p.len()).any(|w| w == p);
        if has(&low, ascii) || has(&low, &utf16) {
            leaks.push(Leak {
                place: "file bytes".into(),
            });
            break;
        }
    }
    Ok(leaks)
}
