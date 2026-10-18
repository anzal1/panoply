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
use panoply_pdf::copy::{Copier, Policy, View};
use panoply_pdf::doc::{PageGeom, PageRef};
use panoply_pdf::filters::deflate;
use panoply_pdf::obj::{Dict, Lexer, Obj, Tok};
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
            max_pixels: crate::input::MAX_PIXELS,
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
    /// Pages where pdfium found characters it could not map to text, with how many. Pages with
    /// 8 or more are read as pictures (and are in `scanned`); fewer cannot be searched there.
    pub unmapped: HashMap<usize, usize>,
    /// The OCR words of those pages in displayed points, so [`apply`] does not read them twice.
    pub scan_words: HashMap<usize, Vec<Word>>,
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
    /// Texts the user kept somewhere, so the same text found elsewhere is not checked for.
    pub unverified: usize,
    /// The lowest resolution a redrawn page ended up at (it is lower than asked when the page
    /// is large and `max_pixels` forced it down), 0 when no page was redrawn.
    pub effective_dpi: f32,
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
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if w <= 0.25 || h <= 0.25 {
        return false;
    }
    let smaller = (a.w * a.h).min(b.w * b.h).max(1e-3);
    w * h >= 0.25 * smaller
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
        Ok(self.chars_counted(index, b)?.0)
    }

    /// Like [`Pdfium::chars`], and how many characters pdfium could not map to Unicode.
    fn chars_counted(&self, index: usize, b: &PageBox) -> Result<(Vec<Ch>, usize)> {
        let _g = panoply_pdf::pdfium::lock();
        let page = self.doc().pages().get(index as i32).map_err(|e| {
            OcrError::BadInput(format!("Page {} could not be opened ({e:?}).", index + 1))
        })?;
        // Not `all()`: it drops characters on rotated pages.
        let Ok(text) = page.text() else {
            return Ok((Vec::new(), 0));
        };
        let mut out: Vec<Ch> = Vec::new();
        let mut unmapped = 0usize;
        for ch in text.chars().iter() {
            let Some(c) = ch.unicode_char() else {
                unmapped += 1;
                continue;
            };
            if is_unmapped(c) {
                unmapped += 1;
            }
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
                // A run of white space (a line end is two characters) is one space, so a
                // number wrapped over two lines is still one match.
                let line_end = c == '\r' || c == '\n';
                if let Some(last) = out.last_mut()
                    && (last.c == ' ' || last.c == '\n')
                {
                    if line_end {
                        last.c = '\n';
                    }
                    continue;
                }
                let r = out.last().map_or(Rect::default(), |p| Rect {
                    x: p.r.right(),
                    y: p.r.y,
                    w: 0.0,
                    h: p.r.h,
                });
                out.push(Ch {
                    c: if line_end { '\n' } else { ' ' },
                    r,
                });
            } else if let Some(r) = rect {
                out.push(Ch { c, r });
            }
        }
        Ok((out, unmapped))
    }

    /// Draw a page at `dpi` (at most `max_pixels`) with its annotations, so the picture matches the page.
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
            // Annotation appearances are drawn into the picture (stamps, highlights, ink,
            // visual signatures stay visible). pdfium clips an appearance to its /Rect, and
            // every dropped annotation and widget has a box burned over its /Rect (see
            // `dropped_annot_rects`), so what was dropped does not show.
            .render_form_data(false)
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

/// An 8 bit gray copy for the engines (they use a third of the memory of RGB, as `pdf.ocr` does).
fn gray_of(d: &Drawn) -> Result<OcrImage> {
    let gray: Vec<u8> = d
        .rgb
        .chunks_exact(3)
        .map(|p| {
            ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8
        })
        .collect();
    OcrImage::gray(d.w, d.h, gray)
        .ok_or_else(|| OcrError::Engine("The page could not be drawn for reading.".into()))
}

/// Like [`gray_of`], taking the colour picture by value so it is freed as soon as the gray
/// copy is made.
fn gray_for_reading(d: Drawn) -> Result<OcrImage> {
    gray_of(&d)
}

struct PageText {
    chars: Vec<Ch>,
    scanned: bool,
    /// Characters pdfium could not map to Unicode.
    unmapped: usize,
    /// For a scan: the OCR words in displayed points.
    words: Vec<Word>,
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
        if ch.c.is_whitespace() && ch.r.w == 0.0 {
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

/// False when a meaningful share of a page's characters have no Unicode mapping, so the page
/// has to be read as a picture.
fn is_mostly_mapped(_shown: usize, unmapped: usize) -> bool {
    unmapped < UNMAPPED_LIMIT
}

/// This many characters without a Unicode mapping, however small a share of the page, send
/// the page to the picture reader: a short number in such a font is exactly what would be
/// missed.
const UNMAPPED_LIMIT: usize = 8;

/// True for a character pdfium could not map to text: the replacement character, private use
/// code points (icon fonts), and control codes other than line and tab marks (what pdfium
/// returns for glyph numbers of an Identity font with no `ToUnicode` map).
fn is_unmapped(c: char) -> bool {
    c == '\u{fffd}'
        || ('\u{e000}'..='\u{f8ff}').contains(&c)
        || (c.is_control() && !matches!(c, '\r' | '\n' | '\t' | '\u{b}' | '\u{c}'))
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
    let (chars, unmapped) = pdfium.chars_counted(index, b)?;
    let shown = chars.iter().filter(|c| !c.c.is_whitespace()).count();
    // Text in a font with no character map cannot be matched: when a meaningful share of the
    // page is like that, read the page as a picture instead.
    if shown >= MIN_TEXT_CHARS && is_mostly_mapped(shown, unmapped) {
        return Ok(PageText {
            chars,
            scanned: false,
            unmapped,
            words: Vec::new(),
        });
    }
    let drawn = pdfium.render(index, 300.0, crate::input::MAX_PIXELS)?;
    let scale = drawn.scale;
    let img = gray_for_reading(drawn)?;
    let page = ocr.recognize(&img, &opts.langs, fetch, cancel)?;
    drop(img);
    let words = page
        .lines
        .iter()
        .flat_map(|l| l.words.iter())
        .map(|w| Word {
            text: w.text.clone(),
            bbox: Rect {
                x: w.bbox.x / scale,
                y: w.bbox.y / scale,
                w: w.bbox.w / scale,
                h: w.bbox.h / scale,
            },
            confidence: w.confidence,
        })
        .collect();
    Ok(PageText {
        chars: chars_from_ocr(&page, scale),
        scanned: true,
        unmapped,
        words,
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

/// The strings a content stream shows (`Tj`, `TJ`), one run per string, for annotation
/// appearances.
fn content_text(data: &[u8]) -> String {
    let mut lx = Lexer::content(data);
    let mut out = String::new();
    loop {
        match lx.next_tok() {
            Ok(Tok::End) | Err(_) => break,
            Ok(Tok::Obj(Obj::Str(s))) => {
                out.extend(s.iter().map(|&c| c as char));
                // Runs on one line are joined with a space.
                out.push(' ');
            }
            Ok(Tok::Obj(Obj::Array(a))) => {
                for x in &a {
                    match x {
                        Obj::Str(s) => out.extend(s.iter().map(|&c| c as char)),
                        // A big negative kern is a gap between words.
                        other => {
                            if other.as_f64().is_some_and(|k| k < -200.0) {
                                out.push(' ');
                            }
                        }
                    }
                }
                out.push(' ');
            }
            // A move to another line.
            Ok(Tok::Word(b"Td" | b"TD" | b"T*" | b"Tm" | b"ET" | b"'" | b"\"")) => {
                out.push('\n');
            }
            Ok(_) => {}
        }
    }
    out
}

/// The text drawn by an annotation's appearance streams (`/AP` normal, down and rollover,
/// including the state dictionaries of check boxes and radio buttons).
fn ap_text(doc: &Doc, d: &Dict) -> Vec<String> {
    let mut out = Vec::new();
    let Some(ap) = d.get("AP").and_then(|a| doc.resolve_dict(a)) else {
        return out;
    };
    for key in ["N", "D", "R"] {
        let Some(n) = ap.get(key) else { continue };
        let mut refs: Vec<u32> = Vec::new();
        if let Some(r) = n.as_ref_num() {
            refs.push(r);
        } else if let Some(states) = n.as_dict() {
            refs.extend(states.iter().filter_map(|(_, v)| v.as_ref_num()));
        }
        for r in refs {
            match doc.stream_data(r) {
                Ok(Some((_, data))) => out.push(content_text(&data)),
                _ => {
                    // A reference to a dictionary of states.
                    if let Ok(l) = doc.load(r)
                        && let Obj::Dict(sd) = l.obj
                    {
                        for (_, v) in sd.iter() {
                            if let Some(x) = v.as_ref_num()
                                && let Ok(Some((_, data))) = doc.stream_data(x)
                            {
                                out.push(content_text(&data));
                            }
                        }
                    }
                }
            }
        }
    }
    out
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
        // Appearance streams of annotations and fields, and script or rich text held in a
        // stream, are text sources too.
        if d.has("Subtype") || d.has("FT") {
            parts.extend(ap_text(doc, d));
        }
        for key in ["JS", "RC"] {
            if let Some(n) = d.get(key).and_then(Obj::as_ref_num)
                && let Ok(Some((_, data))) = doc.stream_data(n)
            {
                parts.push(String::from_utf8_lossy(&data).into_owned());
            }
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
        if page.unmapped > 0 {
            found.unmapped.insert(i, page.unmapped);
        }
        if page.scanned {
            found.scanned.push(i);
            found.scan_words.insert(i, page.words.clone());
        }
        let (text, at) = text_of(&page.chars);
        // Text read by OCR gets the looser patterns (stray spaces around @ and dots).
        let hits = if page.scanned {
            matcher.find_loose(&text)
        } else {
            matcher.find(&text)
        };
        for hit in hits {
            let (lo, hi) = (at[hit.range.start], at[hit.range.end]);
            let id = found.matches.len();
            found.matches.push(Match {
                id,
                kind: hit.kind.to_string(),
                // A number wrapped over lines is shown with single spaces.
                text: text[hit.range]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
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

/// References to dropped objects are removed from every copied dictionary and array, instead
/// of being left as nulls (a null in a field list, an outline chain or a kids array).
struct Scrub;

impl Policy for Scrub {
    fn dict(&mut self, doc: &Doc, view: &View, _src: u32, dict: &mut Dict) {
        inherit_field_keys(doc, view.pruned, dict);
        relink_outline(doc, view.pruned, dict);
        drop_pruned_dict(dict, view.pruned);
    }
}

/// A kept widget whose parent field was dropped becomes a field of its own: it takes the
/// parent's field keys (never the value, which is what was dropped).
fn inherit_field_keys(doc: &Doc, pruned: &HashSet<u32>, dict: &mut Dict) {
    if dict.name("Subtype") != Some(b"Widget") {
        return;
    }
    let Some(mut cur) = dict.get("Parent").and_then(Obj::as_ref_num) else {
        return;
    };
    if !pruned.contains(&cur) {
        return;
    }
    for _ in 0..8 {
        let Ok(l) = doc.load(cur) else { return };
        let Obj::Dict(p) = l.obj else { return };
        for key in ["FT", "Ff", "T", "DA", "Q"] {
            if !dict.has(key)
                && let Some(v) = p.get(key)
            {
                dict.set(key, v.clone());
            }
        }
        match p.get("Parent").and_then(Obj::as_ref_num) {
            Some(n) => cur = n,
            None => return,
        }
    }
}

/// Outline items around dropped ones are joined up again (`/Prev`, `/Next`, `/First`,
/// `/Last`), and `/Count` is worked out again from what is left.
fn relink_outline(doc: &Doc, pruned: &HashSet<u32>, dict: &mut Dict) {
    let is_outline = dict.name("Type") == Some(b"Outlines") || dict.has("Title");
    if !is_outline || !(dict.has("First") || dict.has("Next") || dict.has("Prev")) {
        return;
    }
    for (key, step) in [
        ("Next", "Next"),
        ("Prev", "Prev"),
        ("First", "Next"),
        ("Last", "Prev"),
    ] {
        let Some(start) = dict.get(key).and_then(Obj::as_ref_num) else {
            continue;
        };
        if !pruned.contains(&start) {
            continue;
        }
        let mut cur = Some(start);
        let mut guard = 0;
        while let Some(c) = cur {
            guard += 1;
            if !pruned.contains(&c) || guard > 100_000 {
                break;
            }
            cur = doc.load(c).ok().and_then(|l| match l.obj {
                Obj::Dict(d) => d.get(step).and_then(Obj::as_ref_num),
                _ => None,
            });
        }
        match cur {
            Some(c) if !pruned.contains(&c) => dict.set(key, Obj::r(c)),
            _ => {
                dict.remove(key);
            }
        }
    }
    if let Some(count) = dict.get("Count").and_then(|o| doc.resolve(o).as_i64()) {
        let n = outline_visible(doc, pruned, dict.get("First").and_then(Obj::as_ref_num), 0);
        dict.set("Count", Obj::Int(if count < 0 { -n } else { n }));
    }
}

/// How many outline items under a parent are left and showing, given its first child.
fn outline_visible(doc: &Doc, pruned: &HashSet<u32>, first: Option<u32>, depth: usize) -> i64 {
    let mut n = 0i64;
    let mut cur = first;
    let mut guard = 0;
    while let Some(c) = cur {
        guard += 1;
        if guard > 100_000 || depth > 32 {
            break;
        }
        let d = match doc.load(c) {
            Ok(l) => match l.obj {
                Obj::Dict(d) => d,
                _ => break,
            },
            Err(_) => break,
        };
        if !pruned.contains(&c) {
            n += 1;
            let open = d
                .get("Count")
                .and_then(|o| doc.resolve(o).as_i64())
                .is_some_and(|k| k > 0);
            if open {
                n += outline_visible(
                    doc,
                    pruned,
                    d.get("First").and_then(Obj::as_ref_num),
                    depth + 1,
                );
            }
        }
        cur = d.get("Next").and_then(Obj::as_ref_num);
    }
    n
}

fn drop_pruned_dict(d: &mut Dict, pruned: &HashSet<u32>) {
    // Name and number trees are lists of pairs: a dropped value takes its key with it, so the
    // pairs after it do not shift.
    for key in ["Names", "Nums"] {
        match d.remove(key) {
            Some(Obj::Array(a)) => {
                let mut kept: Vec<Obj> = Vec::with_capacity(a.len());
                for pair in a.chunks(2) {
                    let dead = pair
                        .iter()
                        .any(|x| matches!(x, Obj::Ref(n, _) if pruned.contains(n)));
                    if !dead {
                        kept.extend(pair.iter().cloned());
                    }
                }
                d.set(key, Obj::Array(kept));
            }
            Some(other) => d.set(key, other),
            None => {}
        }
    }
    let dead: Vec<String> = d
        .iter()
        .filter(|(_, v)| matches!(v, Obj::Ref(n, _) if pruned.contains(n)))
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect();
    for k in dead {
        d.remove(&k);
    }
    for v in d.values_mut() {
        drop_pruned_obj(v, pruned);
    }
}

fn drop_pruned_obj(o: &mut Obj, pruned: &HashSet<u32>) {
    match o {
        Obj::Array(a) => {
            a.retain(|x| !matches!(x, Obj::Ref(n, _) if pruned.contains(n)));
            a.iter_mut().for_each(|x| drop_pruned_obj(x, pruned));
        }
        Obj::Dict(d) => drop_pruned_dict(d, pruned),
        _ => {}
    }
}

/// The kids of dropped form fields go with them, and so does a parent field whose other
/// kids hold no value, because a kid widget's appearance stream still draws the value.
fn expand_form_tree(
    doc: &Doc,
    drop_objs: &mut HashMap<u32, &'static str>,
    kept_objs: &HashSet<u32>,
) {
    loop {
        let mut add: Vec<(u32, &'static str)> = Vec::new();
        for &num in drop_objs.keys() {
            let Ok(l) = doc.load(num) else { continue };
            let Obj::Dict(d) = l.obj else { continue };
            // Kids go down with their field.
            if let Some(kids) = d.get("Kids").and_then(|k| doc.resolve_array(k)) {
                add.extend(
                    kids.iter()
                        .filter_map(Obj::as_ref_num)
                        .map(|k| (k, "form field")),
                );
            }
            // The pop-up note of a dropped annotation goes with it.
            if let Some(pop) = d.get("Popup").and_then(Obj::as_ref_num) {
                add.push((pop, "annotation"));
            }
            // A dropped widget takes its parent when no other kid has a value.
            if let Some(parent) = d.get("Parent").and_then(Obj::as_ref_num)
                && (d.has("FT") || d.name("Subtype") == Some(b"Widget"))
                && !drop_objs.contains_key(&parent)
                && let Ok(pl) = doc.load(parent)
                && let Obj::Dict(pd) = pl.obj
                && pd.has("FT")
            {
                let others_valued = pd
                    .get("Kids")
                    .and_then(|k| doc.resolve_array(k))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Obj::as_ref_num)
                    .filter(|k| !drop_objs.contains_key(k))
                    .any(|k| {
                        matches!(doc.load(k), Ok(kl) if matches!(&kl.obj, Obj::Dict(kd) if kd.has("V")))
                    });
                if !others_valued && !pd.has("V") {
                    add.push((parent, "form field"));
                }
            }
        }
        add.retain(|(n, _)| !drop_objs.contains_key(n) && !kept_objs.contains(n));
        if add.is_empty() {
            break;
        }
        for (n, what) in add {
            drop_objs.insert(n, what);
        }
    }
}

/// Boxes, in displayed points, over the `/Rect` of every dropped annotation or widget on a
/// page, so a kept appearance could never show what was dropped.
fn dropped_annot_rects(doc: &Doc, pr: &PageRef, b: &PageBox, dropped: &HashSet<u32>) -> Vec<Rect> {
    let mut out = Vec::new();
    let Ok(l) = doc.load(pr.id) else { return out };
    let Obj::Dict(d) = l.obj else { return out };
    let Some(items) = d.get("Annots").and_then(|a| doc.resolve_array(a)) else {
        return out;
    };
    for item in items {
        let Some(n) = item.as_ref_num() else { continue };
        if !dropped.contains(&n) {
            continue;
        }
        let Some(ad) = doc.resolve_dict(&item) else {
            continue;
        };
        let Some(r) = ad.get("Rect").and_then(|r| doc.rect(r)) else {
            continue;
        };
        let (x0, y0) = to_display(b, r[0], r[1]);
        let (x1, y1) = to_display(b, r[2], r[3]);
        out.push(Rect {
            x: x0.min(x1) as f32,
            y: y0.min(y1) as f32,
            w: (x0 - x1).abs() as f32,
            h: (y0 - y1).abs() as f32,
        });
    }
    out
}

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
/// choices in `selected`. `scan_words` is [`Found::scan_words`] (an empty map reads scans again).
#[allow(clippy::too_many_arguments)]
pub fn apply<W: Write>(
    file: &File,
    out: W,
    opts: &Options,
    all: &[Match],
    scan_words: &HashMap<usize, Vec<Word>>,
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
    report.unverified = all
        .iter()
        .filter(|m| !m.selected)
        .map(|m| m.text.to_lowercase())
        .collect::<HashSet<_>>()
        .len();
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
    expand_form_tree(doc, &mut drop_objs, &kept_objs);
    let drop_set: HashSet<u32> = drop_objs.keys().copied().collect();

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
    let mut copier = Copier::new(doc, Scrub, cancel.clone());
    for (pr, &dst) in pages.iter().zip(&page_ids) {
        copier.preassign(pr.id, dst);
    }
    copier.prune(drop_set.iter().copied());
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
            // A page that was read as a picture while looking (a scan, or text with no
            // character map) is read that way again.
            let has_text = !scan_words.contains_key(&i)
                && chars.iter().filter(|c| !c.c.is_whitespace()).count() >= MIN_TEXT_CHARS;
            let mut drawn = pdfium.render(i, opts.dpi, opts.max_pixels)?;
            let eff = drawn.scale * 72.0;
            if report.effective_dpi == 0.0 || eff < report.effective_dpi {
                report.effective_dpi = eff;
            }
            // The matches, and the place of every dropped annotation or field on the page.
            let rects: Vec<Rect> = {
                let mut v = rects.clone();
                v.extend(dropped_annot_rects(doc, pr, &b, &drop_set));
                v
            };
            let rects = &rects;
            burn(&mut drawn, rects);
            // The text layer: pdfium's words minus any that touch a box, or, for a scan, the
            // OCR of the picture as it now is (the redacted words are not in it).
            let words: Vec<Word> = if has_text {
                words_from_chars(&chars, rects, drawn.scale)
            } else if let Some(read) = scan_words.get(&i) {
                // The words found while looking, minus any that touch a box.
                read.iter()
                    .filter(|w| !rects.iter().any(|r| intersects(r, &w.bbox)))
                    .map(|w| Word {
                        text: w.text.clone(),
                        bbox: Rect {
                            x: w.bbox.x * drawn.scale,
                            y: w.bbox.y * drawn.scale,
                            w: w.bbox.w * drawn.scale,
                            h: w.bbox.h * drawn.scale,
                        },
                        confidence: w.confidence,
                    })
                    .collect()
            } else {
                // Not looked at here: read the picture as it now is.
                let img = gray_of(&drawn)?;
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
    drop_pruned_dict(&mut src_cat, &drop_set);
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
        if c.c.is_whitespace() || gap {
            flush(&mut cur);
        }
        if !c.c.is_whitespace() {
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
fn write_page<W: Write, P: Policy>(
    w: &mut PdfWriter<W>,
    c: &mut Copier<'_, P>,
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
    let pruned = c.view().pruned.clone();
    drop_pruned_dict(&mut d, &pruned);
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
        {
            match doc.stream_data(num) {
                Ok(Some((_, data))) => {
                    if contains_any(&String::from_utf8_lossy(&data), needles) {
                        leaks.push(Leak {
                            place: format!("object {num} (decoded stream)"),
                        });
                    }
                }
                Ok(None) => {}
                // A stream that cannot be read cannot be shown clean.
                Err(_) => leaks.push(Leak {
                    place: format!("could not check object {num}"),
                }),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_with_no_character_map_sends_the_page_to_the_picture_reader() {
        // A few unmapped characters (a bullet, a symbol) are fine.
        assert!(is_mostly_mapped(500, 3));
        assert!(is_mostly_mapped(500, 7));
        // Eight or more, even among a thousand mapped characters, is not: a short number in
        // such a font is what would be missed.
        assert!(!is_mostly_mapped(1000, 8));
        assert!(!is_mostly_mapped(60, 20));
        assert!(!is_mostly_mapped(0, 40));
        assert!(!is_mostly_mapped(1000, 100));
    }

    #[test]
    fn control_codes_of_an_unmapped_font_count_but_line_ends_do_not() {
        assert!(is_unmapped('\u{3}'));
        assert!(is_unmapped('\u{0}'));
        assert!(is_unmapped('\u{fffd}'));
        assert!(is_unmapped('\u{e001}'));
        assert!(!is_unmapped('\n'));
        assert!(!is_unmapped('\r'));
        assert!(!is_unmapped('\t'));
        assert!(!is_unmapped('a'));
        assert!(!is_unmapped('7'));
        assert!(!is_unmapped(' '));
    }

    #[test]
    fn name_tree_pairs_stay_together() {
        let mut ef = Dict::new();
        ef.set(
            "Names",
            Obj::Array(vec![
                Obj::Str(b"a.txt".to_vec()),
                Obj::r(5),
                Obj::Str(b"b.txt".to_vec()),
                Obj::r(9),
            ]),
        );
        let mut d = Dict::new();
        d.set("EmbeddedFiles", Obj::Dict(ef));
        let pruned: HashSet<u32> = [5].into_iter().collect();
        drop_pruned_dict(&mut d, &pruned);
        let ef = d.get("EmbeddedFiles").and_then(Obj::as_dict).unwrap();
        let names = ef.get("Names").and_then(Obj::as_array).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].as_str_bytes(), Some(&b"b.txt"[..]));
        assert_eq!(names[1].as_ref_num(), Some(9));
    }

    #[test]
    fn references_to_dropped_objects_are_removed_not_nulled() {
        let mut d = Dict::new();
        d.set("Next", Obj::r(5));
        d.set("Keep", Obj::r(6));
        d.set(
            "Fields",
            Obj::Array(vec![Obj::r(5), Obj::r(6), Obj::Array(vec![Obj::r(5)])]),
        );
        let pruned: HashSet<u32> = [5].into_iter().collect();
        drop_pruned_dict(&mut d, &pruned);
        assert!(!d.has("Next") && d.has("Keep"));
        let f = d.get("Fields").and_then(Obj::as_array).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].as_ref_num(), Some(6));
        assert_eq!(f[1].as_array().map(<[Obj]>::len), Some(0));
    }

    #[test]
    fn a_content_stream_gives_back_its_strings() {
        let t = content_text(
            b"/Tx BMC q BT /Helv 12 Tf 2 2 Td (2345 6789 0124) Tj [(ab) -20 (cd)] TJ ET Q EMC",
        );
        assert!(t.contains("2345 6789 0124"), "{t:?}");
        assert!(t.contains("abcd"), "{t:?}");
        // A wide gap in a kerned array is a space, and runs on a line are joined by one.
        let t = content_text(b"BT [(ab) -300 (cd)] TJ (ef) Tj 0 -14 Td (gh) Tj ET");
        assert!(t.contains("ab cd"), "{t:?}");
        assert!(t.contains("ef \ngh"), "{t:?}");
    }
}
