//! YuNet face detection (2023mar, MIT) through ONNX Runtime: the fallback engine where the OS has
//! none that passes (docs/research/tiny-models.md, job 10).
//!
//! Input is BGR float, 0 to 255, no mean or scale, in an `NCHW` tensor whose sides are
//! multiples of 32. Twelve outputs, three strides (8, 16, 32): `cls_*` and `obj_*` (one score
//! each, the face score is the square root of their product), `bbox_*` (centre offset and log
//! size, four numbers) and `kps_*` (five landmark offsets, ten numbers). Each cell's numbers
//! decode to pixels with the stride. Detections are merged with non-maximum suppression.
//! Landmarks are the right eye, the left eye, the nose tip, the right and the left mouth corner
//! of the subject, so they are sorted by picture side here.

use ort::value::Tensor;

use panoply_image::error::{ImageError, Result};
use panoply_image::ops::kb::resample_image;
use panoply_image::types::{Image, Samples};

use crate::face::{Engine, Face, P};
use crate::runtime;

/// Faces scoring under this are dropped.
const SCORE_MIN: f32 = 0.6;
/// Overlap above which the weaker of two boxes is dropped.
const NMS_IOU: f32 = 0.3;
/// The side of the square input the model takes.
const INPUT: u32 = 640;
const STRIDES: [usize; 3] = [8, 16, 32];

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

/// Find the faces in an 8 bit RGB picture.
///
/// The published model takes a fixed 640 by 640 input, so the picture is brought to fit inside
/// it (aspect ratio kept) and the rest is left black. Coordinates come back divided by the same
/// ratio, so nothing is stretched.
pub fn detect(img: &Image) -> Result<Vec<Face>> {
    let Samples::U8(_) = &img.samples else {
        return Err(ImageError::Failed("internal: YuNet needs 8 bit RGB".into()));
    };
    let long = f64::from(img.width.max(img.height));
    let k = f64::from(INPUT) / long;
    let (nw, nh) = (
        ((f64::from(img.width) * k).round() as u32).clamp(1, INPUT),
        ((f64::from(img.height) * k).round() as u32).clamp(1, INPUT),
    );
    let net = resample_image(img, nw, nh)?;
    let Samples::U8(px) = &net.samples else {
        return Err(ImageError::Failed("internal: YuNet needs 8 bit RGB".into()));
    };
    let side = INPUT as usize;
    let plane = side * side;
    let mut data = vec![0f32; 3 * plane];
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            let p = &px[(y * nw as usize + x) * 3..][..3];
            let i = y * side + x;
            // BGR order, plain 0 to 255.
            data[i] = f32::from(p[2]);
            data[plane + i] = f32::from(p[1]);
            data[2 * plane + i] = f32::from(p[0]);
        }
    }
    let (sx, sy) = (1.0 / k, 1.0 / k);

    let dets = runtime::with_session(|session| {
        let input = Tensor::from_array(([1usize, 3, side, side], data)).map_err(|e| {
            ImageError::Failed(format!("The face model could not take the picture: {e}"))
        })?;
        let name = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .unwrap_or_else(|| "input".into());
        let outputs = session
            .run(ort::inputs![name.as_str() => input])
            .map_err(|e| ImageError::Failed(format!("The face model failed: {e}")))?;
        let mut all = Vec::new();
        for &stride in &STRIDES {
            let get = |prefix: &str| -> Result<Vec<f32>> {
                let key = format!("{prefix}_{stride}");
                let v = outputs.get(key.as_str()).ok_or_else(|| {
                    ImageError::Failed(format!("The face model has no output called {key}."))
                })?;
                let (_, d) = v.try_extract_tensor::<f32>().map_err(|e| {
                    ImageError::Failed(format!("The face model output {key} is not numbers: {e}"))
                })?;
                Ok(d.to_vec())
            };
            let (cls, obj, bbox, kps) = (get("cls")?, get("obj")?, get("bbox")?, get("kps")?);
            let cells = side / stride;
            if cls.len() != cells * cells
                || bbox.len() != cells * cells * 4
                || kps.len() != cells * cells * 10
            {
                return Err(ImageError::Failed(
                    "The face model gave outputs of an unexpected size.".into(),
                ));
            }
            all.extend(decode(
                stride,
                (cells, cells),
                &Maps {
                    cls: &cls,
                    obj: &obj,
                    bbox: &bbox,
                    kps: &kps,
                },
                SCORE_MIN,
            ));
        }
        Ok(all)
    })?;
    let dets = nms(dets, NMS_IOU);
    Ok(dets.into_iter().map(|d| to_face(&d, sx, sy)).collect())
}

fn to_face(d: &Det, sx: f64, sy: f64) -> Face {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_decodes_to_pixels() {
        // One 2 x 2 map at stride 8, a face in cell (row 1, col 0).
        let cls = [0.0, 0.0, 0.9, 0.0];
        let obj = [0.0, 0.0, 0.9, 0.0];
        let mut bbox = vec![0.0; 16];
        bbox[8..12].copy_from_slice(&[0.5, 0.5, (2.0f32).ln(), (3.0f32).ln()]);
        let mut kps = vec![0.0; 40];
        kps[20..30].copy_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 0.0, 1.0, 1.0, 1.0]);
        let d = decode(
            8,
            (2, 2),
            &Maps {
                cls: &cls,
                obj: &obj,
                bbox: &bbox,
                kps: &kps,
            },
            0.6,
        );
        assert_eq!(d.len(), 1);
        let d = &d[0];
        assert!((d.score - 0.9).abs() < 1e-6);
        // centre (0 + 0.5) * 8 = 4, (1 + 0.5) * 8 = 12; size 16 x 24.
        assert!((d.bbox[0] - (4.0 - 8.0)).abs() < 1e-4);
        assert!((d.bbox[1] - (12.0 - 12.0)).abs() < 1e-4);
        assert!((d.bbox[2] - 16.0).abs() < 1e-4 && (d.bbox[3] - 24.0).abs() < 1e-4);
        assert_eq!(d.kps[0], (0.0, 8.0));
        assert_eq!(d.kps[1], (8.0, 8.0));
        assert_eq!(d.kps[4], (8.0, 16.0));
    }

    #[test]
    fn overlapping_boxes_merge_to_the_best() {
        let mk = |score, x| Det {
            score,
            bbox: [x, 0.0, 10.0, 10.0],
            kps: [(0.0, 0.0); 5],
        };
        let kept = nms(vec![mk(0.7, 0.0), mk(0.9, 1.0), mk(0.8, 40.0)], 0.3);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
        assert!((kept[1].score - 0.8).abs() < 1e-6);
    }

    #[test]
    fn landmarks_are_sorted_by_picture_side() {
        let d = Det {
            score: 0.9,
            bbox: [0.0, 0.0, 10.0, 10.0],
            kps: [(8.0, 2.0), (2.0, 2.0), (5.0, 5.0), (7.0, 8.0), (3.0, 8.0)],
        };
        let f = to_face(&d, 1.0, 1.0);
        assert!(f.eye_l.0 < f.eye_r.0 && f.mouth_l.0 < f.mouth_r.0);
    }
}
