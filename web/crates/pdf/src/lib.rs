//! Panoply PDF in the browser: the desktop page-building engine (panoply-pdf pageops) behind a small wasm API.
//! Merge, split, extract, delete, reorder and rotate are one operation with a different page list.
use panoply_core::CancelToken;
mod error;
mod lang;
mod layout;
mod searchable;
mod textlayer;
mod types;
use panoply_pdf::doc::{Doc, PageRef};
use panoply_pdf::pageops::{self, BuildOptions, CatalogMode, Input, OutPage};
use panoply_pdf::ranges;
use panoply_pdf::source::Source;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Pdfs {
    docs: Vec<Doc>,
    pages: Vec<Vec<PageRef>>,
    titles: Vec<String>,
    report: String,
}

#[derive(Deserialize)]
struct Spec {
    mode: String,
    #[serde(default)]
    bookmarks: bool,
    #[serde(default)]
    drop_page_labels: bool,
    /// [input, page index, rotate degrees]
    pages: Vec<(usize, usize, i32)>,
}

#[derive(Serialize)]
struct Report {
    pages: usize,
    links_removed: usize,
    fields_kept: usize,
    fields_renamed: usize,
    bookmarks: usize,
    unreadable: usize,
    bytes: u64,
}

#[wasm_bindgen]
impl Pdfs {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Pdfs {
        Pdfs { docs: vec![], pages: vec![], titles: vec![], report: String::new() }
    }

    /// Open one PDF; returns its page count.
    pub fn add(&mut self, bytes: Vec<u8>, title: String) -> Result<u32, JsError> {
        let doc = Doc::open(Source::from_bytes(bytes)).map_err(|e| JsError::new(&e.to_string()))?;
        if doc.encryption().is_some() {
            return Err(JsError::new("This PDF is password protected. Remove the password first."));
        }
        let pages = doc.pages().map_err(|e| JsError::new(&e.to_string()))?;
        let n = pages.len() as u32;
        self.docs.push(doc);
        self.pages.push(pages);
        self.titles.push(title);
        Ok(n)
    }

    pub fn page_count(&self, input: usize) -> u32 {
        self.pages.get(input).map(|p| p.len() as u32).unwrap_or(0)
    }

    /// Build a new PDF from a JSON spec: {mode: "subset"|"full", bookmarks, pages: [[input, index, rotate], ...]}.
    pub fn build(&mut self, spec_json: &str) -> Result<Vec<u8>, JsError> {
        let spec: Spec = serde_json::from_str(spec_json).map_err(|e| JsError::new(&e.to_string()))?;
        let inputs: Vec<Input> = self
            .docs
            .iter()
            .zip(self.pages.iter())
            .zip(self.titles.iter())
            .map(|((d, p), t)| Input { doc: d, pages: std::borrow::Cow::Borrowed(&p[..]), title: t.clone() })
            .collect();
        let out_pages: Vec<OutPage> = spec.pages.iter().map(|&(input, index, rotate)| OutPage { input, index, rotate }).collect();
        let opts = BuildOptions {
            mode: if spec.mode == "full" { CatalogMode::Full } else { CatalogMode::Subset },
            bookmarks: spec.bookmarks,
            drop_page_labels: spec.drop_page_labels,
            ..BuildOptions::default()
        };
        let mut out = Vec::new();
        let cancel = CancelToken::new();
        let r = pageops::build(&mut out, &inputs, &out_pages, &opts, &cancel, &mut |_, _| {}).map_err(|e| JsError::new(&e.to_string()))?;
        self.report = serde_json::to_string(&Report { pages: r.pages, links_removed: r.links_removed, fields_kept: r.fields_kept, fields_renamed: r.fields_renamed, bookmarks: r.bookmarks, unreadable: r.unreadable, bytes: r.bytes }).unwrap();
        Ok(out)
    }

    pub fn last_report(&self) -> String {
        self.report.clone()
    }
}

/// Parse a range expression such as "1-3, 7, 9-" for a document of `total` pages. JSON: {items: [{label, pages}], skipped, clamped}.
#[wasm_bindgen]
pub fn parse_ranges(text: &str, total: usize) -> Result<String, JsError> {
    let p = ranges::parse(text, total).map_err(|e| JsError::new(&e.to_string()))?;
    let items: Vec<serde_json::Value> = p.items.iter().map(|i| serde_json::json!({"label": i.label, "pages": i.pages})).collect();
    Ok(serde_json::json!({"items": items, "skipped": p.skipped, "clamped": p.clamped}).to_string())
}

/// Turn a page-index list into a label like "1-3, 7".
#[wasm_bindgen]
pub fn label(pages: Vec<u32>) -> String {
    ranges::label(&pages.iter().map(|&p| p as usize).collect::<Vec<_>>())
}

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

struct Sink(Rc<RefCell<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize)]
struct COpts {
    target: String,
    quality: Option<u8>,
    under_bytes: Option<u64>,
    #[serde(default)]
    grayscale: bool,
    #[serde(default)]
    strip_metadata: bool,
    #[serde(default)]
    strip_attachments: bool,
    #[serde(default)]
    strip_forms: bool,
    max_dpi: Option<f64>,
}

thread_local! { static CREPORT: RefCell<String> = RefCell::new(String::new()); }

/// Compress a PDF with the desktop engine (images recoded under an SSIM floor, streams deflated, duplicates removed).
#[wasm_bindgen]
pub fn compress(bytes: Vec<u8>, opts_json: &str) -> Result<Vec<u8>, JsError> {
    use panoply_pdf::compress::{self, Options, Target};
    let o: COpts = serde_json::from_str(opts_json).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = Doc::open(Source::from_bytes(bytes)).map_err(|e| JsError::new(&e.to_string()))?;
    let pages = doc.pages().map_err(|e| JsError::new(&e.to_string()))?;
    let target = match (o.target.as_str(), o.under_bytes) {
        (_, Some(b)) if b > 0 => Target::UnderBytes(b),
        ("quality", _) => Target::Quality(o.quality.unwrap_or(75).clamp(1, 100)),
        _ => Target::Smallest,
    };
    let opts = Options { target, grayscale: o.grayscale, strip_metadata: o.strip_metadata, strip_attachments: o.strip_attachments, strip_forms: o.strip_forms, max_dpi: o.max_dpi.filter(|d| *d > 0.0) };
    let buf = Rc::new(RefCell::new(Vec::new()));
    let r = compress::compress(&doc, &pages, &opts, &CancelToken::new(), &mut |_, _| {}, Box::new(Sink(buf.clone()))).map_err(|e| JsError::new(&e.to_string()))?;
    CREPORT.with(|c| {
        *c.borrow_mut() = serde_json::json!({
            "original_bytes": r.original_bytes, "output_bytes": r.output_bytes, "images": r.images, "images_lossy": r.images_lossy,
            "images_lossless": r.images_lossless, "images_kept": r.images_kept, "min_image_ssim": r.min_image_ssim,
            "streams_recompressed": r.streams_recompressed, "duplicates_removed": r.duplicates_removed, "signatures": r.signatures,
            "target_reached": r.target_reached, "passes": r.passes, "setting": r.setting, "saving": r.saving(),
        })
        .to_string()
    });
    let out = std::mem::take(&mut *buf.borrow_mut());
    Ok(out)
}

#[wasm_bindgen]
pub fn compress_report() -> String {
    CREPORT.with(|c| c.borrow().clone())
}

// ---------------------------------------------------------------- OCR PDF: the desktop searchable-PDF builder, fed by the browser

fn ocr_page_from_json(v: &serde_json::Value) -> Option<types::OcrPage> {
    let f = |x: &serde_json::Value, k: &str| x[k].as_f64().unwrap_or(0.0) as f32;
    let lines: Vec<types::Line> = v["lines"]
        .as_array()?
        .iter()
        .filter_map(|l| {
            let words: Vec<types::Word> = l["words"]
                .as_array()?
                .iter()
                .map(|w| types::Word { text: w["text"].as_str().unwrap_or("").to_string(), bbox: types::Rect { x: f(w, "x"), y: f(w, "y"), w: f(w, "w"), h: f(w, "h") }, confidence: f(w, "c") })
                .collect();
            types::Line::from_words(words)
        })
        .collect();
    Some(types::OcrPage { width: v["width"].as_u64()? as u32, height: v["height"].as_u64()? as u32, lines, engine: types::EngineKind::PpOcr, languages: vec![] })
}

/// Which pages of a PDF need reading. JSON {pages, needs: [{i, w, h}], signed}.
#[wasm_bindgen]
pub fn ocr_pdf_plan(bytes: Vec<u8>) -> Result<String, JsError> {
    let doc = Doc::open(Source::from_bytes(bytes)).map_err(|e| JsError::new(&e.to_string()))?;
    if doc.encryption().is_some() {
        return Err(JsError::new("This PDF is password protected. Remove the password first."));
    }
    let n = doc.pages().map_err(|e| JsError::new(&e.to_string()))?.len();
    let needs = searchable::plan(&doc).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(serde_json::json!({"pages": n, "signed": doc.signatures().count > 0, "needs": needs.iter().map(|(i, w, h)| serde_json::json!({"i": i, "w": w, "h": h})).collect::<Vec<_>>()}).to_string())
}

#[wasm_bindgen]
pub struct OcrPdf {
    bytes: Vec<u8>,
    read: searchable::ReadPages,
    report: String,
}

#[wasm_bindgen]
impl OcrPdf {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>) -> OcrPdf { OcrPdf { bytes, read: Default::default(), report: String::new() } }
    /// `page_json`: the OCR worker's page JSON; `img_width`: the width in pixels of the picture it read.
    pub fn add_page(&mut self, index: usize, img_width: f32, page_json: &str) -> Result<(), JsError> {
        let v: serde_json::Value = serde_json::from_str(page_json).map_err(|e| JsError::new(&e.to_string()))?;
        let p = ocr_page_from_json(&v).ok_or_else(|| JsError::new("The recognized page could not be read."))?;
        self.read.insert(index, (img_width, p));
        Ok(())
    }
    pub fn finish(&mut self, collect_text: bool) -> Result<Vec<u8>, JsError> {
        let opts = searchable::Options { collect_text, ..Default::default() };
        let mut out = Vec::new();
        let r = searchable::make_searchable_pages(std::mem::take(&mut self.bytes), &mut out, &opts, &self.read, &CancelToken::new(), &mut |_, _| {}).map_err(|e| JsError::new(&e.to_string()))?;
        self.report = serde_json::json!({"pages": r.pages, "read": r.read, "had_text": r.had_text, "no_text": r.no_text, "words": r.words, "flagged": r.flagged, "signed": r.signed, "bytes": r.bytes, "mean_confidence": r.mean_confidence(), "text": r.text}).to_string();
        Ok(out)
    }
    pub fn report(&self) -> String { self.report.clone() }
}

// ---------------------------------------------------------------- pictures to PDF (the desktop images_to_pdf)

#[wasm_bindgen]
pub struct ImagesPdf { names: Vec<String>, files: Vec<Vec<u8>>, report: String }

#[wasm_bindgen]
impl ImagesPdf {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ImagesPdf { ImagesPdf { names: vec![], files: vec![], report: String::new() } }
    pub fn add(&mut self, name: String, bytes: Vec<u8>) { self.names.push(name); self.files.push(bytes); }
    /// page: "image" (each page as big as its picture), "a4", "letter" or "auto". Pictures must be JPEG or PNG.
    pub fn build(&mut self, page: &str, margin_mm: f64) -> Result<Vec<u8>, JsError> {
        use panoply_pdf::images_in::{ImageOptions, PageSize, images_to_pdf};
        let opts = ImageOptions { page: match page { "image" => PageSize::Image, "a4" => PageSize::A4, "letter" => PageSize::Letter, _ => PageSize::A4 }, margin_mm };
        let mut out = Vec::new();
        let (names, files) = (&self.names, &self.files);
        let r = images_to_pdf(&mut out, files.len(), &mut |i| Ok((names[i].clone(), files[i].clone())), &opts, &CancelToken::new(), &mut |_, _| {}).map_err(|e| JsError::new(&e.to_string()))?;
        self.report = format!("{r:?}");
        Ok(out)
    }
    pub fn report(&self) -> String { self.report.clone() }
}
