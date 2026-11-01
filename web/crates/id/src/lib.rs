//! Panoply ID tools in the browser: the desktop signature cleaner pipeline.
mod color;
mod error;
mod image;
mod sheet;
mod face;
mod finder;
mod passport;
mod resample;
mod rules;
mod segment;
mod signature;
mod warp;
mod yunet;

use image::Image;
use wasm_bindgen::prelude::*;

/// Clean a photo of a signature. `ink`: keep, black or blue. `size`: 0 x 0 keeps the signature's own size.
/// Returns RGBA of the finished picture (transparent, or on white); `signature_notes()` has what was done.
#[wasm_bindgen]
pub struct Signature { notes: Vec<String>, w: u32, h: u32, stroke_px: f64, lines_found: usize }

#[wasm_bindgen]
impl Signature {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Signature { Signature { notes: vec![], w: 0, h: 0, stroke_px: 0.0, lines_found: 0 } }

    pub fn run(&mut self, rgba: &[u8], w: u32, h: u32, ink: &str, size_w: u32, size_h: u32, white: bool, remove_lines: bool, thicken: bool) -> Result<Vec<u8>, JsError> {
        let img = Image::rgba(w, h, rgba.to_vec());
        let p = signature::Params { ink: ink.to_string(), size: if size_w > 0 && size_h > 0 { Some((size_w, size_h)) } else { None }, white, remove_lines, thicken };
        let c = signature::clean(&img, &p).map_err(|e| JsError::new(&e.to_string()))?;
        self.stroke_px = c.stroke_px; self.lines_found = c.lines_found;
        let made = signature::finish(&c, &p).map_err(|e| JsError::new(&e.to_string()))?;
        self.notes = made.notes; self.w = made.image.width; self.h = made.image.height;
        let image::Samples::U8(v) = made.image.samples else { return Err(JsError::new("internal")) };
        Ok(match made.image.layout { image::Layout::Rgb => { let mut o = Vec::with_capacity(v.len() / 3 * 4); for p in v.chunks_exact(3) { o.extend_from_slice(&[p[0], p[1], p[2], 255]); } o } _ => v })
    }
    pub fn width(&self) -> u32 { self.w }
    pub fn height(&self) -> u32 { self.h }
    pub fn notes(&self) -> String { serde_json::json!({"notes": self.notes, "stroke_px": self.stroke_px, "lines_found": self.lines_found}).to_string() }
}

/// A print sheet of passport-style photos.
#[wasm_bindgen]
pub struct SheetMaker { photos: Vec<Image>, info: String, w: u32, h: u32 }

#[wasm_bindgen]
impl SheetMaker {
    #[wasm_bindgen(constructor)]
    pub fn new() -> SheetMaker { SheetMaker { photos: vec![], info: String::new(), w: 0, h: 0 } }
    pub fn add_photo(&mut self, rgba: &[u8], w: u32, h: u32) { self.photos.push(Image::rgba(w, h, rgba.to_vec())); }
    /// Paper: 4x6, 5x7, 6x8, a4, a5, a6, letter. `copies` 0 fills the sheet. Returns RGBA of the sheet.
    pub fn make(&mut self, paper: &str, cell_w: f64, cell_h: f64, margin: f64, gutter: f64, copies: u32, cut_marks: bool, rotate: bool, dpi: u32) -> Result<Vec<u8>, JsError> {
        let fail = |e: error::ImageError| JsError::new(&e.to_string());
        let paper = sheet::paper(paper).map_err(fail)?;
        let spec = sheet::SheetSpec { paper, cell_mm: (cell_w, cell_h), margin_mm: margin, gutter_mm: gutter, copies: if copies == 0 { None } else { Some(copies) }, cut_marks, rotate, dpi };
        let s = sheet::make(&self.photos, &spec).map_err(fail)?;
        self.w = s.image.width; self.h = s.image.height;
        self.info = serde_json::json!({"paper": s.paper_name, "paper_mm": [s.paper_mm.0, s.paper_mm.1], "photos": s.placements.len(), "cell_mm": [s.cell_mm.0, s.cell_mm.1], "size_error_mm": s.size_error_mm, "notes": s.notes}).to_string();
        let image::Samples::U8(v) = s.image.samples else { return Err(JsError::new("internal")) };
        Ok(match s.image.layout { image::Layout::Rgb => { let mut o = Vec::with_capacity(v.len() / 3 * 4); for p in v.chunks_exact(3) { o.extend_from_slice(&[p[0], p[1], p[2], 255]); } o } _ => v })
    }
    pub fn width(&self) -> u32 { self.w }
    pub fn height(&self) -> u32 { self.h }
    pub fn info(&self) -> String { self.info.clone() }
}

/// Passport and visa photos: the desktop pipeline (rules, face geometry, warp, exposure, background, checks) with the model runs
/// (YuNet faces, an optional IS-Net cut-out) done by the browser between steps.
#[wasm_bindgen]
pub struct Passport {
    base: Option<Image>,
    finder: Option<finder::Finder>,
    pre: Option<passport::Prepared>,
    params: passport::Params,
    rule: Option<&'static rules::Rule>,
    report: String,
    w: u32,
    h: u32,
}

#[wasm_bindgen]
pub fn passport_rules() -> String {
    serde_json::to_string(rules::all()).unwrap_or_else(|_| "[]".into())
}

#[wasm_bindgen]
impl Passport {
    /// The photo, flattened on white and brought to at most 4096 px on the long side.
    #[wasm_bindgen(constructor)]
    pub fn new(rgba: &[u8], w: u32, h: u32) -> Result<Passport, JsError> {
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        for p in rgba.chunks_exact(4) { let a = u32::from(p[3]); for k in 0..3 { rgb.push(((u32::from(p[k]) * a + 255 * (255 - a) + 127) / 255) as u8); } }
        let mut img = Image { width: w, height: h, layout: image::Layout::Rgb, samples: image::Samples::U8(rgb), icc: None };
        let long = w.max(h);
        if long > 4096 { let k = 4096.0 / f64::from(long); img = image::resample_image(&img, ((f64::from(w) * k).round() as u32).max(1), ((f64::from(h) * k).round() as u32).max(1)).map_err(|e| JsError::new(&e.to_string()))?; }
        let finder = finder::Finder::new(img.clone()).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Passport { base: Some(img), finder: Some(finder), pre: None, params: passport::Params::default(), rule: None, report: String::new(), w: 0, h: 0 })
    }
    pub fn size(&self) -> Vec<u32> { self.base.as_ref().map(|b| vec![b.width, b.height]).or_else(|| self.pre.as_ref().map(|p| vec![p.base.width, p.base.height])).unwrap_or_default() }

    /// Pixels of the output size for a rule: the portal's own pixels, or millimetres at `dpi` (0 uses the rule's).
    pub fn rule_pixels(rule_id: &str, dpi: u32) -> Result<Vec<u32>, JsError> {
        let r = rules::find(rule_id, None).map_err(|e| JsError::new(&e.to_string()))?;
        let (w, h) = r.pixels(if dpi == 0 { None } else { Some(dpi) });
        Ok(vec![w, h])
    }

    pub fn face_next(&mut self) -> Option<Vec<f32>> { self.finder.as_mut().and_then(|f| f.next_input()) }
    pub fn face_put(&mut self, stride: usize, cls: &[f32], obj: &[f32], bbox: &[f32], kps: &[f32]) { if let Some(f) = self.finder.as_mut() { f.put(stride, cls, obj, bbox, kps); } }
    pub fn face_pass_done(&mut self) { if let Some(f) = self.finder.as_mut() { f.pass_done(); } }
    /// The faces found, JSON [{x, y, w, h, confidence}], left to right.
    pub fn faces_json(&self) -> String {
        let v: Vec<_> = self.finder.as_ref().map(|f| f.faces()).unwrap_or_default().iter().map(|f| serde_json::json!({"x": f.x, "y": f.y, "w": f.w, "h": f.h, "confidence": f.confidence})).collect();
        serde_json::to_string(&v).unwrap()
    }

    /// Choose the face, cut the person out and work out the crop. `face_index` is 1 based, 0 chooses automatically.
    /// `matte` is an optional IS-Net mask at the picture's size (empty uses the desktop's plain backdrop colour cut).
    pub fn prepare(&mut self, rule_id: &str, face_index: u32, fix_tilt: bool, fix_exposure: bool, whiten: bool, color_cutout: bool, background: &str, matte: Vec<u8>) -> Result<String, JsError> {
        let rule = rules::find(rule_id, None).map_err(|e| JsError::new(&e.to_string()))?;
        let faces = self.finder.as_ref().map(|f| f.faces()).unwrap_or_default();
        let base = self.base.clone().ok_or_else(|| JsError::new("no photo"))?;
        self.params = passport::Params { face_index: if face_index > 0 { Some(face_index as usize) } else { None }, background: rules::parse_color(background), whiten, fix_tilt, fix_exposure, color_cutout };
        let alpha = if matte.is_empty() { None } else { Some(matte) };
        let pre = passport::prepare_with(base, faces, rule, &self.params, alpha).map_err(|e| JsError::new(&e.to_string()))?;
        let pr = pre.proportions();
        let out = serde_json::json!({"face_count": pre.face_count, "crown_per_em": pr.crown_per_em, "chin_per_em": pr.chin_per_em, "crown_measured": pr.crown_measured, "matte": pre.matte.as_ref().map(|m| m.engine.label()), "reliable": pre.matte.as_ref().map(|m| m.reliable)});
        self.pre = Some(pre); self.rule = Some(rule);
        Ok(out.to_string())
    }

    /// Render at exact pixels; returns RGBA. `report()` has the checks.
    pub fn render(&mut self, w: u32, h: u32) -> Result<Vec<u8>, JsError> {
        let (pre, rule) = (self.pre.as_ref().ok_or_else(|| JsError::new("call prepare first"))?, self.rule.unwrap());
        let made = passport::render(pre, rule, &self.params, w, h).map_err(|e| JsError::new(&e.to_string()))?;
        self.report = serde_json::to_string(&made.report).unwrap_or_default();
        self.w = made.image.width; self.h = made.image.height;
        let image::Samples::U8(v) = made.image.samples else { return Err(JsError::new("internal")) };
        Ok(match made.image.layout { image::Layout::Rgb => { let mut o = Vec::with_capacity(v.len() / 3 * 4); for p in v.chunks_exact(3) { o.extend_from_slice(&[p[0], p[1], p[2], 255]); } o } _ => v })
    }
    pub fn report(&self) -> String { self.report.clone() }
}
