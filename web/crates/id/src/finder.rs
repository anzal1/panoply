//! Finding faces, the desktop way (face::detect over yunet::detect: one pass, then up to three refine rounds on the three biggest faces),
//! turned into a step machine so the model runs can happen in the browser between steps.
use crate::error::Result;
use crate::face::{Face, rotate};
use crate::image::{Image, Layout, Samples, resample_image};
use crate::warp::{self, Affine};
use crate::yunet::{self, Det, Maps, to_face};

const INPUT: usize = 640;
const DETECT_SIDE: u32 = 1280;
const REFINE_ROUNDS: usize = 3;
const REFINE_DONE_DEG: f64 = 0.5;
const REFINE_FACE_PX: f64 = 480.0;
const REFINE_FACES: usize = 3;
const SCORE_MIN: f32 = 0.6;
const NMS_IOU: f32 = 0.3;

struct Refine {
    face: usize,
    ctr: (f64, f64),
    k: f64,
    phi: f64,
    rounds: Vec<(f64, Affine, Face)>,
}

pub struct Finder {
    work: Image,
    inv: f64,
    k_once: f64,
    dets: Vec<Det>,
    faces: Vec<Face>,
    queue: Vec<usize>,
    cur: Option<Refine>,
    once_done: bool,
    pending: Option<Vec<f32>>,
}

fn rgb_px(img: &Image) -> &[u8] {
    match &img.samples { Samples::U8(v) => v, _ => &[] }
}

fn tensor_bgr(px: &[u8], nw: usize, nh: usize) -> Vec<f32> {
    let plane = INPUT * INPUT;
    let mut data = vec![0f32; 3 * plane];
    for y in 0..nh {
        for x in 0..nw {
            let p = &px[(y * nw + x) * 3..][..3];
            let i = y * INPUT + x;
            data[i] = f32::from(p[2]);
            data[plane + i] = f32::from(p[1]);
            data[2 * plane + i] = f32::from(p[0]);
        }
    }
    data
}

impl Finder {
    pub fn new(rgb: Image) -> Result<Finder> {
        let long = rgb.width.max(rgb.height);
        let scale = if long > DETECT_SIDE { f64::from(DETECT_SIDE) / f64::from(long) } else { 1.0 };
        let work = if scale < 1.0 { resample_image(&rgb, ((f64::from(rgb.width) * scale).round() as u32).max(1), ((f64::from(rgb.height) * scale).round() as u32).max(1))? } else { rgb };
        Ok(Finder { work, inv: 1.0 / scale, k_once: 1.0, dets: vec![], faces: vec![], queue: vec![], cur: None, once_done: false, pending: None })
    }

    /// The next model input (640 x 640 BGR planar), or None when the faces are final.
    pub fn next_input(&mut self) -> Option<Vec<f32>> {
        if !self.once_done {
            let long = f64::from(self.work.width.max(self.work.height));
            let k = f64::from(INPUT as u32) / long;
            self.k_once = k;
            let (nw, nh) = (((f64::from(self.work.width) * k).round() as u32).clamp(1, INPUT as u32), ((f64::from(self.work.height) * k).round() as u32).clamp(1, INPUT as u32));
            let net = resample_image(&self.work, nw, nh).ok()?;
            return Some(tensor_bgr(rgb_px(&net), nw as usize, nh as usize));
        }
        loop {
            if self.cur.is_none() {
                let i = self.queue.pop()?;
                let face = &self.faces[i];
                let size = face.w.max(face.h).max(8.0);
                let pts = [face.eye_l, face.eye_r, face.nose, face.mouth_l, face.mouth_r];
                let ctr = (pts.iter().map(|p| p.0).sum::<f64>() / 5.0, pts.iter().map(|p| p.1).sum::<f64>() / 5.0);
                self.cur = Some(Refine { face: i, ctr, k: size / REFINE_FACE_PX, phi: face.roll_deg(), rounds: vec![] });
            }
            let r = self.cur.as_ref().unwrap();
            let half = INPUT as f64 / 2.0;
            let (s, co) = r.phi.to_radians().sin_cos();
            let m = Affine { a: co * r.k, b: -s * r.k, c: r.ctr.0 - (co - s) * half * r.k, d: s * r.k, e: co * r.k, f: r.ctr.1 - (s + co) * half * r.k };
            match warp::render(&self.work, &m, INPUT as u32, INPUT as u32, &[0u8; 3]) {
                Ok((crop, _)) => { self.pending = Some(vec![]); self.cur.as_mut().unwrap().rounds.push((f64::NAN, m, Face { x: 0.0, y: 0.0, w: 0.0, h: 0.0, confidence: 0.0, eye_l: (0.0, 0.0), eye_r: (0.0, 0.0), nose: (0.0, 0.0), mouth_l: (0.0, 0.0), mouth_r: (0.0, 0.0), chin: None, jaw: vec![], yaw_deg: None, eye_open: None, mouth_open: None, engine: crate::face::Engine::YuNet })); return Some(tensor_bgr(rgb_px(&crop), INPUT, INPUT)); }
                Err(_) => { self.finish_refine(); }
            }
        }
    }

    pub fn put(&mut self, stride: usize, cls: &[f32], obj: &[f32], bbox: &[f32], kps: &[f32]) {
        let cells = INPUT / stride;
        self.dets.extend(yunet::decode(stride, (cells, cells), &Maps { cls, obj, bbox, kps }, SCORE_MIN));
    }

    /// Call after the three strides of one run were put.
    pub fn pass_done(&mut self) {
        let dets = yunet::nms(std::mem::take(&mut self.dets), NMS_IOU);
        if !self.once_done {
            self.once_done = true;
            let inv = 1.0 / self.k_once;
            self.faces = dets.iter().map(|d| to_face(d, inv, inv)).collect();
            let mut by_size: Vec<usize> = (0..self.faces.len()).collect();
            by_size.sort_by(|&a, &b| (self.faces[b].w * self.faces[b].h).total_cmp(&(self.faces[a].w * self.faces[a].h)));
            by_size.truncate(REFINE_FACES);
            by_size.reverse(); // popped from the end, biggest first
            self.queue = by_size;
            return;
        }
        let half = INPUT as f64 / 2.0;
        let cur = self.cur.as_mut().unwrap();
        let (_, m, _) = cur.rounds.pop().unwrap();
        let off = |d: &Det| (f64::from(d.bbox[0] + d.bbox[2] / 2.0) - half).hypot(f64::from(d.bbox[1] + d.bbox[3] / 2.0) - half);
        let best = dets.into_iter().min_by(|a, b| off(a).total_cmp(&off(b)));
        let Some(d) = best else { self.finish_refine(); return };
        if off(&d) > 0.35 * REFINE_FACE_PX { self.finish_refine(); return; }
        let read = to_face(&d, 1.0, 1.0);
        let r = read.roll_deg();
        cur.rounds.push((cur.phi + r, m, read));
        if r.abs() < REFINE_DONE_DEG || cur.rounds.len() >= REFINE_ROUNDS { self.finish_refine(); return; }
        cur.phi += r;
    }

    fn finish_refine(&mut self) {
        let Some(cur) = self.cur.take() else { return };
        // Drop a placeholder pushed for a round whose detection did not complete.
        let rounds: Vec<_> = cur.rounds.into_iter().filter(|r| !r.0.is_nan()).collect();
        let Some((last_roll, m, read)) = rounds.last() else { return };
        let roll = rounds.iter().map(|r| r.0).sum::<f64>() / rounds.len() as f64;
        let mut pts = [read.eye_l, read.eye_r, read.nose, read.mouth_l, read.mouth_r].map(|p| m.at(p.0, p.1));
        let c = (pts.iter().map(|p| p.0).sum::<f64>() / 5.0, pts.iter().map(|p| p.1).sum::<f64>() / 5.0);
        let turn = (roll - last_roll).to_radians();
        for p in &mut pts { *p = rotate(*p, c, turn); }
        let f = &mut self.faces[cur.face];
        [f.eye_l, f.eye_r, f.nose, f.mouth_l, f.mouth_r] = pts;
    }

    /// The faces in picture pixels, left to right.
    pub fn faces(&self) -> Vec<Face> {
        let mut v = self.faces.clone();
        if self.inv != 1.0 { for f in &mut v { f.scale_by(self.inv); } }
        v.sort_by(|a, b| a.x.total_cmp(&b.x));
        v
    }
}

#[allow(dead_code)]
fn _unused(_: Layout) {}
