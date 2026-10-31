#![allow(non_snake_case)]
//! Panoply image AI, browser side: the desktop pre and post processing of the IS-Net cutout (resize, guided filter edge
//! refinement, foreground colour estimation, composing) as wasm. The model itself runs in ONNX Runtime Web.
pub mod compose;
pub mod inpaint;
pub mod error;
pub mod matte;
pub mod px;
pub mod tile;
pub mod watermark;
pub mod yunet;

use panoply_core::CancelToken;
use px::Kernel;
use wasm_bindgen::prelude::*;


fn rgb_of(rgba: &[u8]) -> Vec<u8> {
    // Flatten on white when the picture has its own transparency, like flatten_for_model.
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for p in rgba.chunks_exact(4) {
        let a = u32::from(p[3]);
        for k in 0..3 {
            out.push(((u32::from(p[k]) * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    out
}

/// The IS-Net input: the picture at side x side (desktop: 1024) (bilinear), planar, (v - 128) / 256.
#[wasm_bindgen]
pub fn isnet_prep(rgba: &[u8], w: usize, h: usize, side: usize) -> Result<Vec<f32>, JsError> {
    let SIDE = side;
    let rgb = rgb_of(rgba);
    let small = px::resize_u8(&rgb, w, h, 3, SIDE, SIDE, Kernel::Bilinear).map_err(|e| JsError::new(&e.to_string()))?;
    let mut t = vec![0f32; 3 * SIDE * SIDE];
    for (i, p) in small.chunks_exact(3).enumerate() {
        for k in 0..3 {
            t[k * SIDE * SIDE + i] = (f32::from(p[k]) - 128.0) / 256.0;
        }
    }
    Ok(t)
}

/// Turn the model's map into the finished picture. look: 0 transparent (RGBA out), 1 solid colour, 2 blurred copy (RGBA out, alpha 255).
#[wasm_bindgen]
pub fn isnet_finish(map: &[f32], side: usize, rgba: &[u8], w: usize, h: usize, look: u8, bg_r: u8, bg_g: u8, bg_b: u8, blur: f64, refine: bool, decontaminate: bool) -> Result<Vec<u8>, JsError> {
    let SIDE = side;
    let fail = |e: error::ImageError| JsError::new(&e.to_string());
    if map.len() != SIDE * SIDE {
        return Err(JsError::new("The background model returned an unexpected answer. Try again."));
    }
    let (lo, hi) = map.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    let small_mask: Vec<u8> = map.iter().map(|&v| (((v - lo) / span) * 255.0 + 0.5) as u8).collect();
    let mut alpha = px::resize_u8(&small_mask, SIDE, SIDE, 1, w, h, Kernel::CatmullRom).map_err(fail)?;
    let rgb = rgb_of(rgba);
    let cancel = CancelToken::new();
    if refine {
        matte::refine_alpha(&rgb, w, h, &mut alpha, matte::Refine::for_size(w, h), &cancel).map_err(fail)?;
    }
    // The picture's own transparency stays.
    for (a, p) in alpha.iter_mut().zip(rgba.chunks_exact(4)) {
        *a = ((u32::from(*a) * u32::from(p[3]) + 127) / 255) as u8;
    }
    let fg = if decontaminate { matte::foreground_colors(&rgb, &alpha, w, h, &cancel).map_err(fail)? } else { rgb.clone() };
    let bg = [bg_r, bg_g, bg_b];
    let mut out = Vec::with_capacity(w * h * 4);
    match look {
        0 => {
            for (i, p) in fg.chunks_exact(3).enumerate() {
                let a = alpha[i];
                if !decontaminate && a == 0 {
                    out.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    out.extend_from_slice(&[p[0], p[1], p[2], a]);
                }
            }
        }
        1 => {
            let c = compose::compose_color(&fg, &alpha, w, h, bg);
            for p in c.chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        _ => {
            let sigma = (blur.clamp(1.0, 100.0) / 100.0 * w.max(h) as f64 * 0.02) as f32;
            let c = compose::compose_blur(&rgb, &fg, &alpha, w, h, sigma, &cancel).map_err(fail)?;
            for p in c.chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }
    Ok(out)
}

/// Erase: plan the fills for a mask, hand each crop to the model, blend the answers back. Pixels outside the grown mask are never touched.
#[wasm_bindgen]
pub struct Eraser {
    rgb: Vec<u8>,
    alpha: Vec<u8>,
    w: usize,
    h: usize,
    plans: Vec<inpaint::Plan>,
    prepared: Vec<Option<inpaint::Prepared>>,
    changed: Vec<u8>,
    shrunk: bool,
    before: Vec<u8>,
}

#[wasm_bindgen]
impl Eraser {
    /// `mask`: one byte a pixel, non-zero is erased. `expand` grows it (desktop default 8 for erase, 4 for watermarks).
    #[wasm_bindgen(constructor)]
    pub fn new(rgba: &[u8], w: usize, h: usize, mask: &[u8], expand: usize) -> Result<Eraser, JsError> {
        if mask.len() != w * h {
            return Err(JsError::new("The mask does not match the picture."));
        }
        let m: Vec<u8> = mask.iter().map(|&v| u8::from(v != 0)).collect();
        if m.iter().all(|&v| v == 0) {
            return Err(JsError::new("Nothing is marked, so there is nothing to erase. Paint over what you want gone."));
        }
        let rgb = rgb_of(rgba);
        let alpha: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
        let (plans, changed) = inpaint::plan(&m, w, h, expand);
        let n = plans.len();
        Ok(Eraser { before: rgb.clone(), rgb, alpha, w, h, plans, prepared: (0..n).map(|_| None).collect(), changed, shrunk: false })
    }
    pub fn count(&self) -> usize { self.plans.len() }
    fn prep(&mut self, i: usize) -> Result<(), JsError> {
        if self.prepared[i].is_none() {
            self.prepared[i] = Some(inpaint::prepare(&self.rgb, self.w, &self.plans[i]).map_err(|e| JsError::new(&e.to_string()))?);
        }
        Ok(())
    }
    /// [model width, model height] of fill `i`.
    pub fn dims(&mut self, i: usize) -> Result<Vec<u32>, JsError> { self.prep(i)?; let p = self.prepared[i].as_ref().unwrap(); Ok(vec![p.mw as u32, p.mh as u32]) }
    pub fn image(&mut self, i: usize) -> Result<Vec<u8>, JsError> { self.prep(i)?; Ok(self.prepared[i].as_ref().unwrap().planar.clone()) }
    pub fn keep(&mut self, i: usize) -> Result<Vec<u8>, JsError> { self.prep(i)?; Ok(self.prepared[i].as_ref().unwrap().keep.clone()) }
    pub fn put(&mut self, i: usize, result: &[u8]) -> Result<(), JsError> {
        self.prep(i)?;
        let pr = self.prepared[i].take().unwrap();
        self.shrunk |= pr.f > 1.0;
        inpaint::blend(&mut self.rgb, self.w, &self.plans[i], &pr, result).map_err(|e| JsError::new(&e.to_string()))
    }
    pub fn shrunk(&self) -> bool { self.shrunk }
    pub fn changed_percent(&self) -> f64 { 100.0 * self.changed.iter().filter(|&&v| v != 0).count() as f64 / (self.w * self.h) as f64 }
    /// RGBA of the finished picture. Verifies that nothing outside the changed area differs from the input.
    pub fn finish(&self) -> Result<Vec<u8>, JsError> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for i in 0..self.w * self.h {
            let (a, b) = (&self.before[i * 3..i * 3 + 3], &self.rgb[i * 3..i * 3 + 3]);
            if self.changed[i] == 0 && a != b {
                return Err(JsError::new("internal: the fill changed pixels outside the mask, so the result was discarded."));
            }
            out.extend_from_slice(&[b[0], b[1], b[2], if self.changed[i] != 0 { 255 } else { self.alpha[i] }]);
        }
        Ok(out)
    }
}

/// Watermark, logo and date-stamp finding (the desktop's classical detector) with the YuNet face guard: faces are never filled in.
#[wasm_bindgen]
pub struct Marks {
    rgb: Vec<u8>,
    w: usize,
    h: usize,
    reduce: usize,
    k: f64,
    dets: Vec<yunet::Det>,
    faces: Vec<px::Rect>,
    mask: Vec<u8>,
}

#[wasm_bindgen]
impl Marks {
    #[wasm_bindgen(constructor)]
    pub fn new(rgba: &[u8], w: usize, h: usize) -> Marks {
        Marks { rgb: rgb_of(rgba), w, h, reduce: 1, k: 1.0, dets: vec![], faces: vec![], mask: vec![] }
    }

    /// The YuNet input (640 x 640, BGR 0 to 255, planar): the picture reduced to at most 1600 px by box averaging, then fitted into
    /// the square at the top left, as the desktop does.
    pub fn face_input(&mut self) -> Result<Vec<f32>, JsError> {
        let (w, h) = (self.w, self.h);
        let f = w.max(h).div_ceil(1600).max(1);
        self.reduce = f;
        let (sw, sh) = (w.div_ceil(f), h.div_ceil(f));
        let small: Vec<u8> = if f == 1 {
            self.rgb.clone()
        } else {
            let mut s = vec![0u8; sw * sh * 3];
            for dy in 0..sh {
                for dx in 0..sw {
                    let (x1, y1) = (((dx + 1) * f).min(w), ((dy + 1) * f).min(h));
                    let mut sum = [0u32; 3];
                    for y in dy * f..y1 {
                        for x in dx * f..x1 {
                            for (c, v) in sum.iter_mut().enumerate() {
                                *v += u32::from(self.rgb[(y * w + x) * 3 + c]);
                            }
                        }
                    }
                    let n = ((x1 - dx * f) * (y1 - dy * f)) as u32;
                    for c in 0..3 {
                        s[(dy * sw + dx) * 3 + c] = (sum[c] / n) as u8;
                    }
                }
            }
            s
        };
        let side = 640usize;
        let long = sw.max(sh) as f64;
        self.k = side as f64 / long;
        let (nw, nh) = (((sw as f64 * self.k).round() as usize).clamp(1, side), ((sh as f64 * self.k).round() as usize).clamp(1, side));
        let net = px::resize_u8(&small, sw, sh, 3, nw, nh, Kernel::Lanczos3).map_err(|e| JsError::new(&e.to_string()))?;
        let plane = side * side;
        let mut data = vec![0f32; 3 * plane];
        for y in 0..nh {
            for x in 0..nw {
                let p = &net[(y * nw + x) * 3..][..3];
                let i = y * side + x;
                data[i] = f32::from(p[2]);
                data[plane + i] = f32::from(p[1]);
                data[2 * plane + i] = f32::from(p[0]);
            }
        }
        Ok(data)
    }

    /// Feed the four maps of one stride (8, 16 or 32).
    pub fn face_put(&mut self, stride: usize, cls: &[f32], obj: &[f32], bbox: &[f32], kps: &[f32]) -> Result<(), JsError> {
        let cells = 640 / stride;
        if cls.len() != cells * cells || bbox.len() != cells * cells * 4 || kps.len() != cells * cells * 10 {
            return Err(JsError::new("The face model gave outputs of an unexpected size."));
        }
        let d = yunet::decode(stride, (cells, cells), &yunet::Maps { cls, obj, bbox, kps }, 0.6);
        self.dets.extend(d);
        Ok(())
    }

    /// Merge detections into face boxes in picture pixels; returns how many.
    pub fn face_done(&mut self) -> usize {
        let dets = yunet::nms(std::mem::take(&mut self.dets), 0.3);
        let inv = (1.0 / self.k) * self.reduce as f64;
        self.faces = dets
            .iter()
            .filter_map(|d| {
                let (x, y, bw, bh) = (f64::from(d.bbox[0]) * inv, f64::from(d.bbox[1]) * inv, f64::from(d.bbox[2]) * inv, f64::from(d.bbox[3]) * inv);
                let (x0, y0) = (x.max(0.0).min(self.w as f64), y.max(0.0).min(self.h as f64));
                let (x1, y1) = ((x + bw).max(0.0).min(self.w as f64), (y + bh).max(0.0).min(self.h as f64));
                (x1 - x0 >= 2.0 && y1 - y0 >= 2.0).then(|| px::Rect { x: x0 as usize, y: y0 as usize, w: (x1 - x0) as usize, h: (y1 - y0) as usize })
            })
            .collect();
        self.faces.len()
    }

    /// Find the marks. search: 0 anywhere, 1 edges only. Returns JSON {regions: [{x, y, w, h, kind, score, where}], spared, covered}.
    pub fn detect(&mut self, edges: bool) -> String {
        let faces = self.faces.clone();
        let d = watermark::detect_with(&self.rgb, self.w, self.h, if edges { watermark::Search::Edges } else { watermark::Search::Anywhere }, || faces);
        self.mask = d.mask;
        let covered = self.mask.iter().filter(|&&v| v != 0).count() as f64 / (self.w * self.h) as f64;
        let regions: Vec<_> = d.regions.iter().map(|r| serde_json::json!({"x": r.rect.x, "y": r.rect.y, "w": r.rect.w, "h": r.rect.h, "kind": format!("{:?}", r.kind).to_lowercase(), "score": r.score, "where": watermark::place_words(&r.rect, self.w, self.h)})).collect();
        serde_json::json!({"regions": regions, "spared": d.spared, "covered": covered}).to_string()
    }

    pub fn mask(&self) -> Vec<u8> { self.mask.clone() }

    /// Blur fill for flat areas (desktop fill=blur): returns RGBA with the changed pixels opaque.
    pub fn blur_fill(&self, rgba: &[u8], expand: usize) -> Vec<u8> {
        let mut rgb = rgb_of(rgba);
        let changed = watermark::blur_fill(&mut rgb, self.w, self.h, &self.mask, expand);
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for i in 0..self.w * self.h {
            out.extend_from_slice(&[rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2], if changed[i] != 0 { 255 } else { rgba[i * 4 + 3] }]);
        }
        out
    }
}
