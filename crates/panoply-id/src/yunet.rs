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

use crate::face::{Engine, Face, P, rotate};
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

/// Run the network on an 8 bit RGB picture no larger than the input square (placed at the top
/// left, the rest black) and return the merged detections in that picture's own pixels.
fn infer(px: &[u8], nw: usize, nh: usize) -> Result<Vec<Det>> {
    let side = INPUT as usize;
    let plane = side * side;
    let mut data = vec![0f32; 3 * plane];
    for y in 0..nh {
        for x in 0..nw {
            let p = &px[(y * nw + x) * 3..][..3];
            let i = y * side + x;
            // BGR order, plain 0 to 255.
            data[i] = f32::from(p[2]);
            data[plane + i] = f32::from(p[1]);
            data[2 * plane + i] = f32::from(p[0]);
        }
    }
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
    Ok(nms(dets, NMS_IOU))
}

fn rgb_px(img: &Image) -> Result<&[u8]> {
    match &img.samples {
        Samples::U8(px) if img.layout == panoply_image::types::Layout::Rgb => Ok(px),
        _ => Err(ImageError::Failed("internal: YuNet needs 8 bit RGB".into())),
    }
}

/// Find the faces in an 8 bit RGB picture, one network pass, no refinement.
///
/// The published model takes a fixed 640 by 640 input, so the picture is brought to fit inside
/// it (aspect ratio kept) and the rest is left black. Coordinates come back divided by the same
/// ratio, so nothing is stretched.
pub fn detect_once(img: &Image) -> Result<Vec<Face>> {
    rgb_px(img)?;
    let long = f64::from(img.width.max(img.height));
    let k = f64::from(INPUT) / long;
    let (nw, nh) = (
        ((f64::from(img.width) * k).round() as u32).clamp(1, INPUT),
        ((f64::from(img.height) * k).round() as u32).clamp(1, INPUT),
    );
    let net = resample_image(img, nw, nh)?;
    let dets = infer(rgb_px(&net)?, nw as usize, nh as usize)?;
    let inv = 1.0 / k;
    Ok(dets.into_iter().map(|d| to_face(&d, inv, inv)).collect())
}

/// Rounds of re-reading a face on a level crop, at most.
const REFINE_ROUNDS: usize = 3;
/// A residual roll under this many degrees ends the refinement.
const REFINE_DONE_DEG: f64 = 0.5;
/// The face box is brought to this many pixels on the model's 640 pixel input. YuNet reads
/// landmarks best on a face that is large and upright: measured on the corpus portraits turned
/// by -15 to 15 degrees, 480 gives the smallest error (0.9 degrees RMS, against 5.7 for the
/// single pass at the picture's own scale, where a face is small and tilted).
const REFINE_FACE_PX: f64 = 480.0;
/// Only this many of the largest faces are refined; a photo with more is a group shot, which
/// the passport tool refuses anyway.
const REFINE_FACES: usize = 3;

/// Find the faces in an 8 bit RGB picture.
///
/// The first pass reads the eye line from the picture as it is, and YuNet pulls a turned head
/// toward upright: a face rotated 11 degrees reads as 4 to 6. So each face is then read again on
/// a crop turned by the roll found so far (see [`refine`]).
pub fn detect(img: &Image) -> Result<Vec<Face>> {
    let mut faces = detect_once(img)?;
    let mut by_size: Vec<usize> = (0..faces.len()).collect();
    by_size.sort_by(|&a, &b| (faces[b].w * faces[b].h).total_cmp(&(faces[a].w * faces[a].h)));
    for &i in by_size.iter().take(REFINE_FACES) {
        refine(img, &mut faces[i]);
    }
    Ok(faces)
}

/// Re-read `face` on a crop turned level by the roll found so far, add the residual, and repeat
/// until it is under [`REFINE_DONE_DEG`] or after [`REFINE_ROUNDS`] rounds. The eyes, nose and
/// mouth corners are replaced by the refined ones in picture pixels; the box stays as the first
/// pass found it. If no round finds the face near the crop's centre, the first pass is kept.
fn refine(img: &Image, face: &mut Face) {
    use crate::warp::{self, Affine};
    let size = face.w.max(face.h).max(8.0);
    let pts = [
        face.eye_l,
        face.eye_r,
        face.nose,
        face.mouth_l,
        face.mouth_r,
    ];
    let ctr = (
        pts.iter().map(|p| p.0).sum::<f64>() / 5.0,
        pts.iter().map(|p| p.1).sum::<f64>() / 5.0,
    );
    // Picture pixels per crop pixel.
    let k = size / REFINE_FACE_PX;
    let half = f64::from(INPUT) / 2.0;
    let mut phi = face.roll_deg();
    // Each round: the roll it ended on (turn applied plus residual), the crop's map to picture
    // pixels and the landmarks it read.
    let mut rounds: Vec<(f64, Affine, Face)> = Vec::new();
    for _ in 0..REFINE_ROUNDS {
        // The crop's x axis points along the face's eye line, its centre is the face's centre.
        let (s, co) = phi.to_radians().sin_cos();
        let m = Affine {
            a: co * k,
            b: -s * k,
            c: ctr.0 - (co - s) * half * k,
            d: s * k,
            e: co * k,
            f: ctr.1 - (s + co) * half * k,
        };
        let Ok((crop, _)) = warp::render(img, &m, INPUT, INPUT, &[0u8; 3]) else {
            break;
        };
        let Ok(px) = rgb_px(&crop) else { break };
        let Ok(dets) = infer(px, INPUT as usize, INPUT as usize) else {
            break;
        };
        let off = |d: &Det| {
            (f64::from(d.bbox[0] + d.bbox[2] / 2.0) - half)
                .hypot(f64::from(d.bbox[1] + d.bbox[3] / 2.0) - half)
        };
        let Some(d) = dets.into_iter().min_by(|a, b| off(a).total_cmp(&off(b))) else {
            break;
        };
        // A face that is not where the crop was centred is another face, not this one.
        if off(&d) > 0.35 * REFINE_FACE_PX {
            break;
        }
        let read = to_face(&d, 1.0, 1.0);
        let r = read.roll_deg();
        rounds.push((phi + r, m, read));
        if r.abs() < REFINE_DONE_DEG {
            break;
        }
        phi += r;
    }
    let Some((last_roll, m, read)) = rounds.last() else {
        return;
    };
    // The landmarks of the last round, in picture pixels, turned about their centre to the mean
    // of the rolls the rounds ended on. YuNet's reading jumps by a few degrees between crops
    // that differ by a few degrees (it can settle on 17 and then on 20 for a head turned 17),
    // and the mean of the rounds is steadier than any one of them.
    let roll = rounds.iter().map(|r| r.0).sum::<f64>() / rounds.len() as f64;
    let mut pts = [
        read.eye_l,
        read.eye_r,
        read.nose,
        read.mouth_l,
        read.mouth_r,
    ]
    .map(|p| m.at(p.0, p.1));
    let c = (
        pts.iter().map(|p| p.0).sum::<f64>() / 5.0,
        pts.iter().map(|p| p.1).sum::<f64>() / 5.0,
    );
    let turn = (roll - last_roll).to_radians();
    for p in &mut pts {
        *p = rotate(*p, c, turn);
    }
    [
        face.eye_l,
        face.eye_r,
        face.nose,
        face.mouth_l,
        face.mouth_r,
    ] = pts;
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
