//! Panoply OCR in the browser: the desktop PP-OCR pipeline around the model runs (detection strips, DB box finding, crop
//! and recognizer input, CTC decoding, reading order), copied unchanged. ONNX Runtime Web runs the two models in between.
#![allow(dead_code)]
mod ctc;
mod db;
mod error;
mod imgops;
mod lang;
mod layout;
mod types;

use imgops::Oriented;
use types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};
use wasm_bindgen::prelude::*;

const STRIP_H: usize = 384;
const STRIP_MARGIN: usize = 64;
const REC_H: usize = 48;
const REC_MAX_W: usize = 3000;
const MIN_LONG: f32 = 1280.0;
const MAX_LONG: f32 = 1600.0;
const DROP_SCORE: f32 = 0.3;

fn split_long(r: &Oriented) -> Vec<Oriented> {
    let want = (REC_H as f32 * r.w / r.h.max(1.0)).ceil() as usize;
    let n = want.div_ceil(REC_MAX_W).max(1);
    if n == 1 {
        return vec![*r];
    }
    let (s, c) = r.angle.sin_cos();
    let len = r.w / n as f32;
    (0..n)
        .map(|i| {
            let off = (i as f32 + 0.5) * len - r.w / 2.0;
            Oriented { cx: r.cx + off * c, cy: r.cy + off * s, w: len, h: r.h, angle: r.angle }
        })
        .collect()
}

fn merge_pieces(lines: Vec<Line>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for l in lines {
        if let Some(prev) = out.last_mut()
            && prev.bbox.v_overlap(&l.bbox) > 0.8 * prev.bbox.h.min(l.bbox.h)
            && (l.bbox.x - prev.bbox.right()).abs() < 0.5 * prev.bbox.h
            && l.bbox.x >= prev.bbox.x
        {
            prev.words.extend(l.words);
            prev.bbox = prev.bbox.union(&l.bbox);
            continue;
        }
        out.push(l);
    }
    out
}

fn line_from(rect: &Oriented, rows: &[f32], steps: usize, classes: usize, dict: &ctc::Dict) -> Option<Line> {
    let emits = ctc::decode(rows, steps, classes, dict);
    let spans = ctc::words(&emits, steps);
    if spans.is_empty() {
        return None;
    }
    let words: Vec<Word> = spans
        .iter()
        .map(|sp| {
            let u0 = sp.start / steps as f32 * rect.w - rect.w / 2.0;
            let u1 = sp.end / steps as f32 * rect.w - rect.w / 2.0;
            let (x, y, w, h) = imgops::span_bounds(rect, u0, u1);
            Word { text: sp.text.clone(), bbox: Rect { x, y, w, h }, confidence: sp.confidence }
        })
        .collect();
    let line = Line::from_words(words).expect("a line with spans has words");
    let n: usize = spans.iter().map(|w| w.text.chars().count()).sum();
    let score = spans.iter().map(|w| w.confidence * w.text.chars().count() as f32).sum::<f32>() / n.max(1) as f32;
    (score >= DROP_SCORE).then_some(line)
}

#[wasm_bindgen]
pub struct Ocr {
    img: OcrImage,
    dict_text: String,
    dict: Option<ctc::Dict>,
    dw: usize,
    dh: usize,
    px: Vec<u8>,
    prob: Vec<f32>,
    rects: Vec<Oriented>,
    jobs: Vec<(Oriented, usize)>,
    slots: Vec<Option<Line>>,
    order: Vec<usize>,
}

#[wasm_bindgen]
impl Ocr {
    /// `rgba` is flattened on white. `dict` is the recognizer's dictionary text (en, latin or devanagari).
    #[wasm_bindgen(constructor)]
    pub fn new(rgba: &[u8], w: usize, h: usize, dict: String) -> Result<Ocr, JsError> {
        let mut rgb = Vec::with_capacity(w * h * 3);
        for p in rgba.chunks_exact(4) {
            let a = u32::from(p[3]);
            for k in 0..3 {
                rgb.push(((u32::from(p[k]) * a + 255 * (255 - a) + 127) / 255) as u8);
            }
        }
        let img = OcrImage::rgb(w as u32, h as u32, rgb).ok_or_else(|| JsError::new("The picture could not be read."))?;
        let long = w.max(h) as f32;
        let target = long.clamp(MIN_LONG.min(MAX_LONG), MAX_LONG);
        let scale = target / long;
        let round32 = |v: f32| (((v / 32.0).round() as usize).max(1)) * 32;
        let (dw, dh) = (round32(w as f32 * scale), round32(h as f32 * scale));
        let px = imgops::det_resize(&img, dw, dh);
        Ok(Ocr { img, dict_text: dict, dict: None, dw, dh, px, prob: vec![0f32; dw * dh], rects: vec![], jobs: vec![], slots: vec![], order: vec![] })
    }

    pub fn det_width(&self) -> usize { self.dw }
    pub fn det_height(&self) -> usize { self.dh }

    /// Strips as a flat list [y0, y1, keep0, keep1, ...].
    pub fn det_strips(&self) -> Vec<u32> {
        let mut v = Vec::new();
        let (dh, mut y0) = (self.dh, 0usize);
        while y0 < dh {
            let y1 = (y0 + STRIP_H).min(dh);
            let keep0 = if y0 == 0 { y0 } else { y0 + STRIP_MARGIN };
            let keep1 = if y1 == dh { y1 } else { y1 - STRIP_MARGIN };
            v.extend_from_slice(&[y0 as u32, y1 as u32, keep0 as u32, keep1 as u32]);
            if y1 == dh {
                break;
            }
            y0 += STRIP_H - 2 * STRIP_MARGIN;
        }
        v
    }

    pub fn det_input(&self, y0: usize, y1: usize) -> Vec<f32> {
        imgops::det_tensor(&self.px, usize::from(self.img.channels), self.dw, y0, y1)
    }

    pub fn det_put(&mut self, y0: usize, keep0: usize, keep1: usize, data: &[f32]) {
        let dw = self.dw;
        self.prob[keep0 * dw..keep1 * dw].copy_from_slice(&data[(keep0 - y0) * dw..(keep1 - y0) * dw]);
    }

    /// Find the lines; returns how many pieces will be recognized, sorted narrow to wide.
    pub fn det_finish(&mut self) -> usize {
        let found = db::boxes(&self.prob, self.dw, self.dh, &db::DbParams::default());
        self.prob = Vec::new();
        self.px = Vec::new();
        let (iw, ih) = (self.img.width as f32, self.img.height as f32);
        let (sx, sy) = (iw / self.dw as f32, ih / self.dh as f32);
        let sm = (sx + sy) / 2.0;
        self.rects = found.iter().map(|b| Oriented { cx: b.rect.cx * sx, cy: b.rect.cy * sy, w: b.rect.w * sm, h: b.rect.h * sm, angle: b.rect.angle }).collect();
        let mut jobs: Vec<(usize, Oriented, usize)> = Vec::new();
        for rect in &self.rects {
            for piece in split_long(rect) {
                let ratio = piece.w / piece.h.max(1.0);
                let out_w = ((REC_H as f32 * ratio).ceil() as usize).clamp(16, REC_MAX_W);
                jobs.push((jobs.len(), piece, out_w));
            }
        }
        jobs.sort_by_key(|j| j.2);
        self.jobs = jobs.iter().map(|j| (j.1, j.2)).collect();
        // Output order is the order of the unsorted jobs, so remember it.
        self.order = jobs.iter().map(|j| j.0).collect();
        self.slots = vec![None; self.jobs.len()];
        self.jobs.len()
    }

    pub fn rec_width(&self, i: usize) -> usize { self.jobs[i].1 }

    /// Planar 3 x 48 x width input for piece `i`.
    pub fn rec_input(&self, i: usize) -> Vec<f32> {
        let (rect, w) = &self.jobs[i];
        imgops::rec_tensor(&self.img, rect, REC_H, *w)
    }

    /// Logits of one piece: [1, steps, classes].
    pub fn rec_put(&mut self, i: usize, logits: &[f32], steps: usize, classes: usize) -> Result<(), JsError> {
        if self.dict.is_none() {
            self.dict = Some(ctc::Dict::new(&self.dict_text, classes).map_err(|e| JsError::new(&e.to_string()))?);
        }
        let dict = self.dict.as_ref().unwrap();
        if classes != dict.len() {
            return Err(JsError::new("The text recognition model does not match its dictionary."));
        }
        let (rect, _) = &self.jobs[i];
        self.slots[self.order[i]] = line_from(rect, logits, steps, classes, dict);
        Ok(())
    }

    fn page(&self) -> OcrPage {
        let lines: Vec<Line> = self.slots.iter().cloned().flatten().collect();
        OcrPage { width: self.img.width, height: self.img.height, lines: layout::reading_order(merge_pieces(lines)), engine: EngineKind::PpOcr, languages: vec![] }
    }

    pub fn text(&self, keep_breaks: bool) -> String { layout::plain_text(&self.page(), keep_breaks) }
    pub fn markdown(&self) -> String { layout::markdown(&self.page()) }

    /// JSON: {width, height, lines: [{text, x, y, w, h, words: [{text, x, y, w, h, c}]}]}
    pub fn json(&self) -> String {
        let p = self.page();
        let lines: Vec<_> = p.lines.iter().map(|l| serde_json::json!({
            "text": l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            "x": l.bbox.x, "y": l.bbox.y, "w": l.bbox.w, "h": l.bbox.h,
            "words": l.words.iter().map(|w| serde_json::json!({"text": w.text, "x": w.bbox.x, "y": w.bbox.y, "w": w.bbox.w, "h": w.bbox.h, "c": w.confidence})).collect::<Vec<_>>(),
        })).collect();
        serde_json::json!({"width": p.width, "height": p.height, "lines": lines}).to_string()
    }
}
