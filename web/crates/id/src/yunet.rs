//! YuNet output decoding and non-maximum suppression, copied unchanged from panoply-id yunet.rs.
#![allow(dead_code)]
use crate::face::{Engine, Face, P, rotate};

/// One decoded detection before it becomes a [`Face`].
#[derive(Debug, Clone, PartialEq)]
pub struct Det {
    /// Score, 0 to 1.
    pub score: f32,
    /// Box left, top, width, height in network pixels.
    pub bbox: [f32; 4],
    /// Five landmarks in network pixels: right eye, left eye, nose, right mouth, left mouth.
    pub kps: [(f32, f32); 5],
}

/// The four maps one stride produces, row major over the grid.
pub struct Maps<'a> {
    /// One score per cell.
    pub cls: &'a [f32],
    /// One score per cell.
    pub obj: &'a [f32],
    /// Four numbers per cell.
    pub bbox: &'a [f32],
    /// Ten numbers per cell.
    pub kps: &'a [f32],
}

/// Decode one stride's maps over a `rows` by `cols` grid of cells `stride` pixels wide.
pub fn decode(
    stride: usize,
    (rows, cols): (usize, usize),
    maps: &Maps<'_>,
    score_min: f32,
) -> Vec<Det> {
    let Maps {
        cls,
        obj,
        bbox,
        kps,
    } = *maps;
    let mut out = Vec::new();
    let s = stride as f32;
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
            if score < score_min {
                continue;
            }
            let b = &bbox[i * 4..i * 4 + 4];
            let (cx, cy) = ((c as f32 + b[0]) * s, (r as f32 + b[1]) * s);
            let (w, h) = (b[2].exp() * s, b[3].exp() * s);
            let k = &kps[i * 10..i * 10 + 10];
            let mut pts = [(0.0f32, 0.0f32); 5];
            for (n, p) in pts.iter_mut().enumerate() {
                *p = ((k[2 * n] + c as f32) * s, (k[2 * n + 1] + r as f32) * s);
            }
            out.push(Det {
                score,
                bbox: [cx - w / 2.0, cy - h / 2.0, w, h],
                kps: pts,
            });
        }
    }
    out
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (ax1, ay1, ax2, ay2) = (a[0], a[1], a[0] + a[2], a[1] + a[3]);
    let (bx1, by1, bx2, by2) = (b[0], b[1], b[0] + b[2], b[1] + b[3]);
    let iw = (ax2.min(bx2) - ax1.max(bx1)).max(0.0);
    let ih = (ay2.min(by2) - ay1.max(by1)).max(0.0);
    let inter = iw * ih;
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Non-maximum suppression, best score first.
pub fn nms(mut dets: Vec<Det>, thr: f32) -> Vec<Det> {
    dets.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<Det> = Vec::new();
    for d in dets {
        if keep.iter().all(|k| iou(&k.bbox, &d.bbox) <= thr) {
            keep.push(d);
        }
    }
    keep
}


pub fn to_face(d: &Det, sx: f64, sy: f64) -> Face {
    let pt = |p: (f32, f32)| -> P { (f64::from(p.0) * sx, f64::from(p.1) * sy) };
    let (mut e0, mut e1) = (pt(d.kps[0]), pt(d.kps[1]));
    if e0.0 > e1.0 {
        std::mem::swap(&mut e0, &mut e1);
    }
    let (mut m0, mut m1) = (pt(d.kps[3]), pt(d.kps[4]));
    if m0.0 > m1.0 {
        std::mem::swap(&mut m0, &mut m1);
    }
    Face {
        x: f64::from(d.bbox[0]) * sx,
        y: f64::from(d.bbox[1]) * sy,
        w: f64::from(d.bbox[2]) * sx,
        h: f64::from(d.bbox[3]) * sy,
        confidence: d.score,
        eye_l: e0,
        eye_r: e1,
        nose: pt(d.kps[2]),
        mouth_l: m0,
        mouth_r: m1,
        chin: None,
        jaw: Vec::new(),
        yaw_deg: None,
        eye_open: None,
        mouth_open: None,
        engine: Engine::YuNet,
    }
}
