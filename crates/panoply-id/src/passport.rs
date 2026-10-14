//! `id.passport_photo`: a photo that passes the passport or visa rules of a country
//! (docs/FLOWS.md, worked example 2).
//!
//! The job, in order:
//!
//! 1. Open the photo upright (orientation applied, sRGB), brought to at most 4096 px on the long side.
//! 2. Find the face and its landmarks (Apple Vision on macOS, YuNet elsewhere). Two faces: ask which.
//! 3. Cut the person out (system subject mask, else a plain-backdrop colour cut) to measure the top of
//!    the head, hair included, and to whiten the background.
//! 4. Work out the crop from the rule: head height in the middle of the allowed range, the eye line
//!    inside its band, the head centred, the head level (tilt corrected by turning the picture).
//! 5. Render it in one pass to exact pixels, set the background, pull the exposure toward the middle
//!    if the face is too dark or bright.
//! 6. Check the result against the rule and say pass, warn or fail for each thing, with the number.
//! 7. Encode as JPEG, searching the quality until it lands inside the portal's size window.
//!
//! Everything that can be fixed is fixed when its option is on (tilt, background, exposure) and the
//! check then says what was done. What cannot be fixed from pixels (a turned head, closed eyes, a
//! busy backdrop) is reported as it is.

use std::path::{Path, PathBuf};

use serde::Serialize;

use panoply_image::error::{ImageError, Result};
use panoply_image::format::Format;
use panoply_image::ops::common::{check_cancel, display_name, write_output};
use panoply_image::ops::fit::Fit;
use panoply_image::ops::kb::{self, FitChoice, Spec};
use panoply_image::ops::options::Opts;
use panoply_image::ops::{Io, NameSpec, human_bytes, with_commas};
use panoply_image::types::{Image, Layout, Samples};

use crate::face::{self, CHIN_PER_EYE_MOUTH, CROWN_PER_EYE_MOUTH, Engine, EnginePref, Face, P};
use crate::imageio::{jpeg_bytes, load_upright, set_jfif_density};
use crate::rules::{self, Rule};
use crate::segment::{self, Matte};
use crate::sheet;
use crate::warp::{self, Affine};

/// The longest side the source is brought to. A passport photo needs about 1200 px tall at the
/// most, so a 4096 px source leaves room for a head that fills a quarter of the frame.
const SOURCE_CAP: u32 = 4096;

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Meets the rule.
    Pass,
    /// Close to the limit, or not fully certain.
    Warn,
    /// Does not meet the rule.
    Fail,
}

impl Status {
    /// The word on the result card.
    pub fn word(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// One line of the compliance report.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// Stable id (`head_size`, `eye_line`, `centered`, `tilt`, `pose`, `eyes_open`, `expression`,
    /// `resolution`, `framing`, `background`, `lighting`, `sharpness`, `color`, `file`).
    pub id: &'static str,
    /// What it is, for people.
    pub label: String,
    /// The result.
    pub status: Status,
    /// The number and the rule, in a sentence.
    pub detail: String,
    /// A fix exists for this (tilt, background, exposure).
    pub fixable: bool,
    /// The fix was applied, and `status` is for the picture after it.
    pub fixed: bool,
}

/// What was measured and decided.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The rule used.
    pub rule: String,
    /// Output pixels.
    pub width_px: u32,
    /// Output pixels.
    pub height_px: u32,
    /// Face engine.
    pub engine: &'static str,
    /// Cut-out engine, when one was used.
    pub cutout: Option<&'static str>,
    /// Head height in the photo, millimetres.
    pub head_mm: f64,
    /// Eye line above the bottom edge, millimetres.
    pub eye_line_mm: f64,
    /// Whether the top of the head was measured (true) or estimated from the face (false).
    pub crown_measured: bool,
    /// The checks, in order.
    pub checks: Vec<Check>,
    /// Things nothing here looks at, so a person should.
    pub unchecked: Vec<String>,
}

impl Report {
    /// The worst status of any check.
    pub fn overall(&self) -> Status {
        self.checks
            .iter()
            .map(|c| c.status)
            .fold(Status::Pass, |a, b| match (a, b) {
                (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
                (Status::Warn, _) | (_, Status::Warn) => Status::Warn,
                _ => Status::Pass,
            })
    }

    /// Lines for the result card, one per check.
    pub fn lines(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .checks
            .iter()
            .map(|c| {
                let tag = if c.fixed {
                    " (fixed)"
                } else if c.fixable && c.status != Status::Pass {
                    " (can be fixed: turn the fix on)"
                } else {
                    ""
                };
                format!("{}: {}. {}{}", c.status.word(), c.label, c.detail, tag)
            })
            .collect();
        if !self.unchecked.is_empty() {
            v.push(format!(
                "not checked, please look yourself: {}",
                self.unchecked.join(", ")
            ));
        }
        v
    }
}

/// What a run is asked to do.
#[derive(Debug, Clone)]
pub struct Params {
    /// Face engine choice.
    pub engine: EnginePref,
    /// Which face when there are several (1 is the leftmost).
    pub face_index: Option<usize>,
    /// Background override.
    pub background: Option<[u8; 3]>,
    /// Make the background the rule's colour.
    pub whiten: bool,
    /// Turn the head level.
    pub fix_tilt: bool,
    /// Pull a dark or bright face toward the middle.
    pub fix_exposure: bool,
    /// Use the plain-backdrop colour cut even where the OS has a subject mask.
    pub color_cutout: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            engine: EnginePref::Auto,
            face_index: None,
            background: None,
            whiten: true,
            fix_tilt: true,
            fix_exposure: true,
            color_cutout: false,
        }
    }
}

/// Everything found about the source that rendering needs.
pub struct Prepared {
    /// The source picture.
    pub base: Image,
    /// The chosen face.
    pub face: Face,
    /// Engine that found it.
    pub engine: Engine,
    /// The cut-out, when whitening or head measuring uses one.
    pub matte: Option<Matte>,
    geo: Geometry,
    /// How many faces were in the picture.
    pub face_count: usize,
}

/// The crop, in source pixels, in the frame where the head is level.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// Pivot of the turn (the point between the eyes).
    pivot: P,
    /// Turn applied, radians (0 when not levelling).
    turn: f64,
    /// Roll found, degrees.
    roll_deg: f64,
    /// Crop rectangle in the level frame.
    y0: f64,
    h: f64,
    /// Chin and crown rows in the level frame.
    chin_y: f64,
    crown_y: f64,
    crown_measured: bool,
    /// Horizontal centre of the face in the level frame.
    face_cx: f64,
}

fn level_pt(p: P, g: &Geometry) -> P {
    face::rotate(p, g.pivot, -g.turn)
}

/// Mid-point of the allowed head height as a fraction of the photo height.
fn head_target_frac(rule: &Rule) -> f64 {
    let (lo, hi) = rule
        .head_range_mm()
        .unwrap_or((0.7 * rule.height_mm, 0.8 * rule.height_mm));
    (lo + hi) / 2.0 / rule.height_mm
}

/// Pick the face to use. One face is the normal case; several need a choice.
fn choose_face(mut faces: Vec<Face>, index: Option<usize>) -> Result<(Face, usize)> {
    let n = faces.len();
    match (n, index) {
        (0, _) => Err(ImageError::Failed(
            "No face was found in this photo. Use a photo where one person faces the camera, with the whole head in the frame.".into(),
        )),
        (1, Some(i)) if i != 1 => Err(ImageError::BadOption(
            "Only one face was found, so face must be 1.".into(),
        )),
        (_, Some(i)) => {
            if i == 0 || i > n {
                Err(ImageError::BadOption(format!(
                    "This photo has {n} faces. Choose face 1 to {n}, counting from the left."
                )))
            } else {
                Ok((faces.swap_remove(i - 1), n))
            }
        }
        (1, None) => Ok((faces.remove(0), 1)),
        (_, None) => {
            // Two faces: if one is much bigger it is the subject, as in a photo with someone
            // behind. Otherwise ask which.
            let mut by_size: Vec<usize> = (0..n).collect();
            by_size.sort_by(|&a, &b| (faces[b].w * faces[b].h).total_cmp(&(faces[a].w * faces[a].h)));
            let (big, next) = (&faces[by_size[0]], &faces[by_size[1]]);
            if big.w * big.h >= 2.5 * next.w * next.h {
                Ok((faces.swap_remove(by_size[0]), n))
            } else {
                Err(ImageError::BadOption(format!(
                    "This photo has {n} faces. Say which one with face=1 to {n}, counting from the left. A passport photo has one person in it."
                )))
            }
        }
    }
}

/// Find the face, cut the person out and work out the crop for `rule`.
pub fn prepare(base: Image, rule: &Rule, p: &Params) -> Result<Prepared> {
    let (faces, engine) = face::detect(&base, p.engine)?;
    let (face, face_count) = choose_face(faces, p.face_index)?;
    let matte_pref = if p.color_cutout {
        EnginePref::YuNet
    } else {
        p.engine
    };
    let matte = segment::compute(&base, &face, matte_pref).ok();
    let geo = geometry(&base, &face, matte.as_ref(), rule, p);
    Ok(Prepared {
        base,
        face,
        engine,
        matte,
        geo,
        face_count,
    })
}

/// Where the top of the head and the chin sit, in eye-to-mouth distances (for calibration).
#[derive(Debug, Clone, Copy)]
pub struct Proportions {
    /// Eye line to top of head.
    pub crown_per_em: f64,
    /// Mouth line to chin.
    pub chin_per_em: f64,
    /// The top of the head came from the cut-out (true) or from the estimate (false).
    pub crown_measured: bool,
}

impl Prepared {
    /// What was measured about the head, relative to the eye-to-mouth distance.
    pub fn proportions(&self) -> Proportions {
        let g = &self.geo;
        let em = self.face.eye_mouth();
        let mouth_y = level_pt(self.face.mouth_mid(), g).1;
        Proportions {
            crown_per_em: (g.pivot.1 - g.crown_y) / em,
            chin_per_em: (g.chin_y - mouth_y) / em,
            crown_measured: g.crown_measured,
        }
    }
}

fn geometry(base: &Image, face: &Face, matte: Option<&Matte>, rule: &Rule, p: &Params) -> Geometry {
    let pivot = face.eye_mid();
    let roll = face.roll_deg();
    let turn = if p.fix_tilt { roll.to_radians() } else { 0.0 };
    let mut g = Geometry {
        pivot,
        turn,
        roll_deg: roll,
        y0: 0.0,
        h: 0.0,
        chin_y: 0.0,
        crown_y: 0.0,
        crown_measured: false,
        face_cx: pivot.0,
    };
    let em = face.eye_mouth();
    // Chin: measured by the engine when it has a jaw line, else from the face's proportions.
    // On a tilted head the lowest jaw point in the picture is not the chin: look in the level frame.
    let chin_y = if face.jaw.len() >= 5 {
        face.jaw
            .iter()
            .map(|&q| level_pt(q, &g).1)
            .fold(f64::MIN, f64::max)
    } else if let Some(c) = face.chin {
        level_pt(c, &g).1
    } else {
        level_pt(face.mouth_mid(), &g).1 + CHIN_PER_EYE_MOUTH * em
    };
    // Crown: the top of the cut-out above the face (hair included), else proportions.
    let est = pivot.1 - CROWN_PER_EYE_MOUTH * em;
    let mut crown_y = est;
    let mut measured = false;
    if let Some(m) = matte.filter(|m| m.reliable) {
        // Walk up the level frame's vertical lines through the face width.
        let half = face.w * 0.45;
        let cx = pivot.0;
        let steps = 9;
        let max_up = 4.5 * em;
        let mut y = pivot.1 - 0.5 * em;
        let top_limit = pivot.1 - max_up;
        let mut found = None;
        // Go from the eyes upward: the head is solid until its top, so the first row with too
        // little person is the crown.
        while y > top_limit {
            let covered = (0..steps)
                .filter(|&i| {
                    let x = cx - half * 0.6 + (half * 1.2) * f64::from(i) / f64::from(steps - 1);
                    let s = face::rotate((x, y), pivot, g.turn);
                    m.at(s.0, s.1) >= 128
                })
                .count();
            if covered < 2 {
                found = Some(y + 1.0);
                break;
            }
            y -= 1.0;
        }
        if let Some(top) = found
            && top < pivot.1 - 1.1 * em
            && top > pivot.1 - 4.2 * em
        {
            crown_y = top;
            measured = true;
        }
    }
    g.chin_y = chin_y;
    g.crown_y = crown_y;
    g.crown_measured = measured;
    // Face centre: the eyes and the jaw agree about it on a straight head.
    g.face_cx = if face.jaw.len() >= 5 {
        let xs: Vec<f64> = face.jaw.iter().map(|&q| level_pt(q, &g).0).collect();
        let (a, b) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        (pivot.0 + (a + b) / 2.0) / 2.0
    } else {
        let mm = level_pt(face.mouth_mid(), &g).0;
        (pivot.0 + mm) / 2.0
    };
    // The crop.
    let head_h = (chin_y - crown_y).max(1.0);
    let frac = head_target_frac(rule);
    let crop_h = head_h / frac;
    // Where the eye line sits in the photo, as a fraction of the height from the bottom.
    let eye_from_chin = chin_y - pivot.1;
    let eye_to_crown = pivot.1 - crown_y;
    let margin_min = 0.03;
    let e_pos = match rule.eye_range_mm() {
        Some((lo, hi)) => {
            let (lo, hi) = (lo / rule.height_mm, hi / rule.height_mm);
            let cap = 1.0 - margin_min - eye_to_crown / crop_h;
            ((lo + hi) / 2.0).min(cap).max(lo)
        }
        None => {
            // 40 percent of what is left over goes above the head, 60 below the chin.
            let spare = crop_h - head_h;
            (eye_from_chin + 0.6 * spare) / crop_h
        }
    };
    let bottom = pivot.1 + e_pos * crop_h;
    g.h = crop_h;
    g.y0 = bottom - crop_h;
    let _ = base;
    g
}

/// Source position of the output pixel grid for an output of `w` by `h` pixels.
fn affine(g: &Geometry, w: u32, h: u32) -> Affine {
    // Square pixels: one scale both ways, and the crop is as wide as the output's shape asks (a
    // portal's 600 x 750 px is not the 35 x 45 mm of the print).
    let k = g.h / f64::from(h);
    let (sx, sy) = (k, k);
    let x0 = g.face_cx - f64::from(w) * k / 2.0;
    let (c, s) = (g.turn.cos(), g.turn.sin());
    let (dx0, dy0) = (x0 - g.pivot.0, g.y0 - g.pivot.1);
    Affine {
        a: c * sx,
        b: -s * sy,
        c: g.pivot.0 + c * dx0 - s * dy0,
        d: s * sx,
        e: c * sy,
        f: g.pivot.1 + s * dx0 + c * dy0,
    }
}

/// Output pixel position of a source point.
fn to_out(g: &Geometry, p: P, w: u32, h: u32) -> P {
    let l = level_pt(p, g);
    let k = g.h / f64::from(h);
    let x0 = g.face_cx - f64::from(w) * k / 2.0;
    ((l.0 - x0) / k, (l.1 - g.y0) / k)
}

fn luma(c: &[u8]) -> f64 {
    0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])
}

/// Mean luma of the rectangle `(x0, y0, x1, y1)` of an RGB picture, only where `keep(x, y)`.
fn mean_luma(img: &Image, r: (f64, f64, f64, f64), keep: impl Fn(u32, u32) -> bool) -> Option<f64> {
    let Samples::U8(v) = &img.samples else {
        return None;
    };
    let (w, h) = (img.width, img.height);
    let (x0, y0) = (r.0.max(0.0) as u32, r.1.max(0.0) as u32);
    let (x1, y1) = (
        (r.2.min(f64::from(w))) as u32,
        (r.3.min(f64::from(h))) as u32,
    );
    let (mut sum, mut n) = (0.0, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            if keep(x, y) {
                let o = (y as usize * w as usize + x as usize) * 3;
                sum += luma(&v[o..o + 3]);
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum / n as f64)
}

/// Sharpness of the face: the Laplacian variance of the face box brought to a fixed 160 px width,
/// so the number does not depend on how big the face is in the frame (a tight crop of a soft photo
/// must not score higher than a wide crop of a sharp one). A face with fewer than 160 px is kept
/// at its own size: an enlarged face reads as soft, which it is.
fn face_sharpness(img: &Image, r: (f64, f64, f64, f64)) -> f64 {
    let (x0, y0) = (r.0.max(0.0) as u32, r.1.max(0.0) as u32);
    let (x1, y1) = (
        (r.2.min(f64::from(img.width))) as u32,
        (r.3.min(f64::from(img.height))) as u32,
    );
    if x1 <= x0 + 8 || y1 <= y0 + 8 {
        return 0.0;
    }
    let crop = img.crop(x0, y0, x1 - x0, y1 - y0);
    let target = 160u32;
    let crop = if crop.width > target {
        let h = ((f64::from(crop.height) * f64::from(target) / f64::from(crop.width)).round()
            as u32)
            .max(8);
        panoply_image::ops::kb::resample_image(&crop, target, h).unwrap_or(crop)
    } else {
        crop
    };
    laplacian_variance(
        &crop,
        (0.0, 0.0, f64::from(crop.width), f64::from(crop.height)),
    )
}

/// Variance of the Laplacian over a rectangle (a standard sharpness number: sharp portraits
/// score in the hundreds at this size, a blurred one in the low tens).
fn laplacian_variance(img: &Image, r: (f64, f64, f64, f64)) -> f64 {
    let Samples::U8(v) = &img.samples else {
        return 0.0;
    };
    let (w, h) = (img.width as usize, img.height as usize);
    let (x0, y0) = ((r.0.max(1.0)) as usize, (r.1.max(1.0)) as usize);
    let (x1, y1) = (
        (r.2.min(w as f64 - 1.0)) as usize,
        (r.3.min(h as f64 - 1.0)) as usize,
    );
    let l = |x: usize, y: usize| luma(&v[(y * w + x) * 3..(y * w + x) * 3 + 3]);
    let (mut s, mut s2, mut n) = (0.0, 0.0, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let lap = 4.0 * l(x, y) - l(x - 1, y) - l(x + 1, y) - l(x, y - 1) - l(x, y + 1);
            s += lap;
            s2 += lap * lap;
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    let m = s / n as f64;
    s2 / n as f64 - m * m
}

fn range_status(v: f64, lo: f64, hi: f64, tol: f64) -> Status {
    if v >= lo && v <= hi {
        Status::Pass
    } else if v >= lo - tol && v <= hi + tol {
        Status::Warn
    } else {
        Status::Fail
    }
}

fn check(id: &'static str, label: &str, status: Status, detail: String) -> Check {
    Check {
        id,
        label: label.to_string(),
        status,
        detail,
        fixable: false,
        fixed: false,
    }
}

/// The rendered photo and what was found.
pub struct Made {
    /// The photo at the output size, RGB.
    pub image: Image,
    /// The report.
    pub report: Report,
}

/// Render the photo at `w` by `h` pixels and check it.
pub fn render(pre: &Prepared, rule: &Rule, p: &Params, w: u32, h: u32) -> Result<Made> {
    let g = &pre.geo;
    let bg = p.background.unwrap_or_else(|| rule.background_rgb());
    let m = affine(g, w, h);
    // Enough pixels? Source pixels per output pixel below 0.5 means a 2x enlargement or more.
    let k = m.scale();
    if k < 0.5 {
        let have = (g.h).round() as u32;
        return Err(ImageError::Failed(format!(
            "This photo is too small to make a {} photo from: the area that would fill the frame is only {have} pixels tall and the photo needs {h}. Use the original from the camera, or a photo where your face fills more of the picture.",
            rule.name
        )));
    }
    // The picture, RGB, through the crop.
    let (mut fg, cov) = warp::render(&pre.base, &m, w, h, &bg)?;
    // The matte through the same map.
    let matte_img = pre.matte.as_ref().map(|mt| -> Result<Image> {
        let s = mt.scale;
        let mm = Affine {
            a: m.a * s,
            b: m.b * s,
            c: m.c * s,
            d: m.d * s,
            e: m.e * s,
            f: m.f * s,
        };
        Ok(warp::render(&mt.alpha, &mm, w, h, &[0])?.0)
    });
    let matte_out = match matte_img {
        Some(r) => Some(r?),
        None => None,
    };

    let mut checks: Vec<Check> = Vec::new();
    let mut unchecked: Vec<String> = vec![
        "glasses and glare".into(),
        "hair across the eyes".into(),
        "head coverings".into(),
    ];
    let height_mm = rule.height_mm;
    let mm_per_src = height_mm / g.h;
    let head_mm = (g.chin_y - g.crown_y) * mm_per_src;
    let eye_line_mm = (g.y0 + g.h - g.pivot.1) * mm_per_src;

    // Face and engine.
    checks.push(check(
        "face",
        "Face",
        if pre.face.confidence >= 0.7 {
            Status::Pass
        } else {
            Status::Warn
        },
        format!(
            "one face found with {} (confidence {:.0} percent)",
            pre.engine.label(),
            pre.face.confidence * 100.0
        ),
    ));

    // Head size.
    if let Some((lo, hi)) = rule.head_range_mm() {
        let mut st = range_status(head_mm, lo, hi, 1.0);
        let how = if g.crown_measured {
            "measured chin to top of hair"
        } else {
            // The top of the head came from the face's proportions, good to about 3 mm on a
            // 35 x 45 mm photo: only a clear margin inside the range counts as a pass.
            let u = 0.065 * rule.height_mm;
            if st == Status::Pass
                && (hi - lo <= u || head_mm < lo + u * 0.5 || head_mm > hi - u * 0.5)
            {
                st = Status::Warn;
            }
            "top of the head estimated from the face (no subject mask), good to about 3 mm"
        };
        checks.push(check(
            "head_size",
            "Head size",
            st,
            format!("{head_mm:.1} mm, {how}; the rule asks {lo:.1} to {hi:.1} mm"),
        ));
    }
    // Eye line.
    if let Some((lo, hi)) = rule.eye_range_mm() {
        let st = range_status(eye_line_mm, lo, hi, 1.0);
        checks.push(check(
            "eye_line",
            "Eye line",
            st,
            format!(
                "{eye_line_mm:.1} mm above the bottom edge; the rule asks {lo:.1} to {hi:.1} mm"
            ),
        ));
    }
    // Centred.
    let face_out = to_out(g, (pre.face.x + pre.face.w / 2.0, pre.face.y), w, h);
    let jaw_out = if pre.face.jaw.len() >= 5 {
        let xs: Vec<f64> = pre.face.jaw.iter().map(|&q| to_out(g, q, w, h).0).collect();
        let (a, b) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        (a + b) / 2.0
    } else {
        face_out.0
    };
    let off = (jaw_out - f64::from(w) / 2.0) / f64::from(w);
    let st = if off.abs() <= 0.02 {
        Status::Pass
    } else if off.abs() <= 0.05 {
        Status::Warn
    } else {
        Status::Fail
    };
    checks.push(check(
        "centered",
        "Centred",
        st,
        format!(
            "the face is {:.1} percent of the width off centre",
            off.abs() * 100.0
        ),
    ));
    // Tilt (before the fix).
    {
        let r = g.roll_deg.abs();
        let tol = rule.max_tilt_deg;
        let raw = if r <= tol / 2.0 {
            Status::Pass
        } else if r <= tol {
            Status::Warn
        } else {
            Status::Fail
        };
        let mut c = check(
            "tilt",
            "Head tilt",
            raw,
            format!("the head was tilted {r:.1} degrees (the rule allows about {tol:.0})"),
        );
        c.fixable = r > 0.4;
        if p.fix_tilt && c.fixable {
            c.fixed = true;
            c.status = Status::Pass;
            c.detail = format!("the head was tilted {r:.1} degrees and was turned level");
        }
        checks.push(c);
    }
    // Pose.
    {
        let t = pre.face.turn().abs();
        let st = if t < 0.10 {
            Status::Pass
        } else if t < 0.18 {
            Status::Warn
        } else {
            Status::Fail
        };
        checks.push(check(
            "pose",
            "Facing the camera",
            st,
            format!("the nose is {:.0} percent of the eye distance off centre; a turned head cannot be fixed, retake it", t * 100.0),
        ));
    }
    // Eyes and mouth, where the engine measures them.
    match pre.face.eye_open {
        Some(e) => {
            let st = if e >= 0.17 {
                Status::Pass
            } else if e >= 0.11 {
                Status::Warn
            } else {
                Status::Fail
            };
            checks.push(check(
                "eyes_open",
                "Eyes open",
                st,
                format!("eye opening is {e:.2} of the eye width (open eyes read 0.2 or more)"),
            ));
        }
        None => unchecked.push("eyes open".into()),
    }
    match pre.face.mouth_open {
        Some(mo) => {
            let st = if mo <= 0.07 {
                Status::Pass
            } else if mo <= 0.25 {
                Status::Warn
            } else {
                Status::Fail
            };
            checks.push(check(
                "expression",
                "Neutral expression",
                st,
                format!("mouth opening is {mo:.2} of the mouth width (closed reads 0.07 or less)"),
            ));
        }
        None => unchecked.push("neutral expression".into()),
    }
    // Resolution.
    {
        let st = if k >= 1.0 {
            Status::Pass
        } else if k >= 0.7 {
            Status::Warn
        } else {
            Status::Fail
        };
        let detail = if k >= 1.0 {
            format!(
                "the photo has {k:.1} source pixels for each output pixel, so nothing was enlarged"
            )
        } else {
            format!(
                "the photo was enlarged {:.1} times to reach {w} x {h} px, so it may look soft; a larger original is better",
                1.0 / k
            )
        };
        checks.push(check("resolution", "Resolution", st, detail));
    }
    // Framing: did the crop reach past the picture, and is the head all there?
    {
        // The top of the head (measured, or estimated) as a point in the picture.
        let crown = face::rotate((g.face_cx, g.crown_y), g.pivot, g.turn);
        let head_cut = crown.1 < -0.5;
        let st = if cov.outside <= 0.002 && !head_cut {
            Status::Pass
        } else if head_cut || cov.outside > 0.12 {
            Status::Fail
        } else {
            Status::Warn
        };
        let detail = if head_cut {
            "the top of the head is outside the photo; retake it with some space above the head"
                .to_string()
        } else if cov.outside <= 0.002 {
            "the whole frame comes from the photo".to_string()
        } else {
            format!(
                "{:.1} percent of the frame was beyond the photo (shoulders or sides cut by the picture's edge) and was filled with the background colour",
                cov.outside * 100.0
            )
        };
        checks.push(check("framing", "Framing", st, detail));
    }

    // Exposure on the person, before the background changes.
    let face_box = {
        let tl = to_out(g, (pre.face.x, pre.face.y), w, h);
        let br = to_out(g, (pre.face.x + pre.face.w, pre.face.y + pre.face.h), w, h);
        (
            tl.0.min(br.0),
            tl.1.min(br.1),
            tl.0.max(br.0),
            tl.1.max(br.1),
        )
    };
    let eyes_out = (
        to_out(g, pre.face.eye_l, w, h),
        to_out(g, pre.face.eye_r, w, h),
        to_out(g, pre.face.mouth_mid(), w, h),
    );
    let cheek_band = (
        face_box.0 + (face_box.2 - face_box.0) * 0.12,
        eyes_out.0.1.min(eyes_out.1.1),
        face_box.2 - (face_box.2 - face_box.0) * 0.12,
        eyes_out.2.1,
    );
    let mid_x = (cheek_band.0 + cheek_band.2) / 2.0;
    let face_luma = mean_luma(&fg, cheek_band, |_, _| true).unwrap_or(128.0);
    let left = mean_luma(
        &fg,
        (
            cheek_band.0,
            cheek_band.1,
            mid_x - (cheek_band.2 - cheek_band.0) * 0.08,
            cheek_band.3,
        ),
        |_, _| true,
    )
    .unwrap_or(face_luma);
    let right = mean_luma(
        &fg,
        (
            mid_x + (cheek_band.2 - cheek_band.0) * 0.08,
            cheek_band.1,
            cheek_band.2,
            cheek_band.3,
        ),
        |_, _| true,
    )
    .unwrap_or(face_luma);
    let uneven = (left - right).abs() / ((left + right) / 2.0).max(1.0);
    {
        let bad_exposure = !(55.0..=215.0).contains(&face_luma);
        let mut st = if uneven <= 0.18 {
            Status::Pass
        } else if uneven <= 0.32 {
            Status::Warn
        } else {
            Status::Fail
        };
        let mut detail = format!(
            "the face averages {face_luma:.0} of 255 and the two sides differ by {:.0} percent",
            uneven * 100.0
        );
        let mut c_fixed = false;
        let mut fixable = false;
        if bad_exposure {
            fixable = true;
            st = st.max_status(Status::Fail);
            if p.fix_exposure {
                // Pull the face's brightness toward 135 with a gamma curve on the whole picture.
                let gamma = (135.0f64 / 255.0).ln() / (face_luma.clamp(8.0, 247.0) / 255.0).ln();
                apply_gamma(&mut fg, gamma.clamp(0.4, 2.5));
                c_fixed = true;
                detail = format!(
                    "the face averaged {face_luma:.0} of 255, too {}; brightness was corrected. The two sides differ by {:.0} percent",
                    if face_luma < 55.0 { "dark" } else { "bright" },
                    uneven * 100.0
                );
                st = if uneven <= 0.18 {
                    Status::Pass
                } else if uneven <= 0.32 {
                    Status::Warn
                } else {
                    Status::Fail
                };
            } else {
                detail.push_str(&format!(
                    "; that is too {}",
                    if face_luma < 55.0 { "dark" } else { "bright" }
                ));
            }
        }
        let mut c = check("lighting", "Lighting", st, detail);
        c.fixable = fixable;
        c.fixed = c_fixed;
        checks.push(c);
    }

    // Background.
    let mut cutout = pre.matte.as_ref().map(|m| m.engine.label());
    let mut bg_check;
    let whiten_ok =
        p.whiten && pre.matte.as_ref().is_some_and(|m| m.reliable) && matte_out.is_some();
    if whiten_ok {
        let alpha = matte_out.as_ref().expect("checked above");
        composite(&mut fg, alpha, bg);
        let orig = pre.matte.as_ref().map(|m| m.backdrop).unwrap_or(bg);
        let delta = colour_delta(orig, bg);
        bg_check = check(
            "background",
            "Background",
            Status::Pass,
            format!(
                "the background is now {} (it was about {}), cut out with {}",
                colour_name(bg),
                colour_name(orig),
                pre.matte.as_ref().map(|m| m.engine.label()).unwrap_or("")
            ),
        );
        bg_check.fixable = delta > 18.0;
        bg_check.fixed = delta > 18.0;
        if delta <= 18.0 {
            bg_check.detail = format!(
                "the background was already close to {}; it was evened out",
                colour_name(bg)
            );
        }
    } else if p.whiten {
        cutout = pre.matte.as_ref().map(|m| m.engine.label());
        bg_check = check(
            "background",
            "Background",
            Status::Fail,
            "the backdrop is too busy to cut the person out cleanly without a subject mask, so it was left as it is. Retake the photo against a plain wall".into(),
        );
    } else {
        let orig = pre.matte.as_ref().map(|m| m.backdrop).unwrap_or([128; 3]);
        let delta = colour_delta(orig, bg);
        let uniform = pre.matte.as_ref().map(|m| m.uniform).unwrap_or(0.0);
        let st = if delta <= 22.0 && uniform > 0.85 {
            Status::Pass
        } else if delta <= 40.0 {
            Status::Warn
        } else {
            Status::Fail
        };
        bg_check = check(
            "background",
            "Background",
            st,
            format!(
                "the background is about {}; the rule asks for {}",
                colour_name(orig),
                colour_name(bg)
            ),
        );
        bg_check.fixable = true;
    }
    checks.push(bg_check);

    // Sharpness on the face of the final picture.
    {
        let lv = face_sharpness(&fg, face_box);
        let st = if lv >= SHARP_PASS {
            Status::Pass
        } else if lv >= SHARP_WARN {
            Status::Warn
        } else {
            Status::Fail
        };
        checks.push(check(
            "sharpness",
            "Sharpness",
            st,
            format!("the face scores {lv:.0} for fine detail ({SHARP_PASS:.0} or more is sharp); a blurred photo cannot be fixed, retake it"),
        ));
    }
    // Colour.
    {
        let chroma = face_chroma(&fg, cheek_band);
        let st = if chroma >= 8.0 {
            Status::Pass
        } else {
            Status::Warn
        };
        checks.push(check(
            "color",
            "Colour photo",
            st,
            if chroma >= 8.0 {
                "the photo is in colour".into()
            } else {
                "the photo looks black and white; most authorities ask for colour".to_string()
            },
        ));
    }
    let report = Report {
        rule: rule.id.clone(),
        width_px: w,
        height_px: h,
        engine: pre.engine.label(),
        cutout,
        head_mm,
        eye_line_mm,
        crown_measured: g.crown_measured,
        checks,
        unchecked,
    };
    Ok(Made { image: fg, report })
}

/// Sharpness limits for the Laplacian variance of the face (see `tests/passport.rs`).
pub const SHARP_PASS: f64 = 100.0;
/// Below this a photo is called blurry.
pub const SHARP_WARN: f64 = 40.0;

impl Status {
    fn max_status(self, other: Status) -> Status {
        match (self, other) {
            (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
            (Status::Warn, _) | (_, Status::Warn) => Status::Warn,
            _ => Status::Pass,
        }
    }
}

fn colour_delta(a: [u8; 3], b: [u8; 3]) -> f64 {
    let d: f64 = a
        .iter()
        .zip(&b)
        .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
        .sum();
    d.sqrt()
}

fn colour_name(c: [u8; 3]) -> String {
    let (r, g, b) = (i32::from(c[0]), i32::from(c[1]), i32::from(c[2]));
    let hex = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let l = (r + g + b) / 3;
    let spread = r.max(g).max(b) - r.min(g).min(b);
    let word = if spread < 18 {
        if l > 235 {
            "white"
        } else if l > 190 {
            "light grey"
        } else if l > 90 {
            "grey"
        } else {
            "dark"
        }
    } else if b > r && b >= g {
        "blue"
    } else if r > g && r > b {
        "red or brown"
    } else {
        "green"
    };
    format!("{word} ({hex})")
}

fn face_chroma(img: &Image, r: (f64, f64, f64, f64)) -> f64 {
    let Samples::U8(v) = &img.samples else {
        return 0.0;
    };
    let (w, h) = (img.width, img.height);
    let (x0, y0) = (r.0.max(0.0) as u32, r.1.max(0.0) as u32);
    let (x1, y1) = (r.2.min(f64::from(w)) as u32, r.3.min(f64::from(h)) as u32);
    let (mut s, mut n) = (0.0, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let o = (y as usize * w as usize + x as usize) * 3;
            let mx = v[o].max(v[o + 1]).max(v[o + 2]);
            let mn = v[o].min(v[o + 1]).min(v[o + 2]);
            s += f64::from(mx - mn);
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { s / n as f64 }
}

fn apply_gamma(img: &mut Image, gamma: f64) {
    let Samples::U8(v) = &mut img.samples else {
        return;
    };
    let mut lut = [0u8; 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = ((i as f64 / 255.0).powf(gamma) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    }
    for p in v.iter_mut() {
        *p = lut[*p as usize];
    }
}

/// Put the person on `bg`. The matte is tightened a little so no old backdrop shows at the
/// edge, and edge pixels take their colour from the person next to them, not from what was
/// behind them.
fn composite(fg: &mut Image, alpha: &Image, bg: [u8; 3]) {
    let (Samples::U8(px), Samples::U8(a)) = (&mut fg.samples, &alpha.samples) else {
        return;
    };
    let (w, h) = (fg.width as usize, fg.height as usize);
    // Weighted colour of the solid person, spread outward a few pixels.
    let weight: Vec<f32> = a
        .iter()
        .map(|&p| if p >= 242 { 1.0 } else { 0.0 })
        .collect();
    let mut chans: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            (0..w * h)
                .map(|i| f32::from(px[i * 3 + c]) * weight[i])
                .collect()
        })
        .collect();
    let spread = |data: &[f32], r: usize| -> Vec<f32> { box_blur(data, w, h, r) };
    let wb = spread(&weight, 3);
    for ch in &mut chans {
        *ch = spread(ch, 3);
    }
    for i in 0..w * h {
        let t = f32::from(a[i]) / 255.0;
        // Tighten: values under 0.12 are backdrop, over 0.92 are person.
        let alpha = ((t - 0.12) / 0.8).clamp(0.0, 1.0);
        let mut c = [
            f32::from(px[i * 3]),
            f32::from(px[i * 3 + 1]),
            f32::from(px[i * 3 + 2]),
        ];
        if t < 0.95 && wb[i] > 0.02 {
            let est = [
                chans[0][i] / wb[i],
                chans[1][i] / wb[i],
                chans[2][i] / wb[i],
            ];
            let keep = ((t - 0.5) / 0.45).clamp(0.0, 1.0);
            for k in 0..3 {
                c[k] = est[k] * (1.0 - keep) + c[k] * keep;
            }
        }
        for k in 0..3 {
            let v = c[k] * alpha + f32::from(bg[k]) * (1.0 - alpha);
            px[i * 3 + k] = (v + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    // Running sums on a padded border: out of range counts as zero (the weights do the same).
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut acc: f32 = row[..=r.min(w - 1)].iter().sum();
        for x in 0..w {
            tmp[y * w + x] = acc;
            if x + r + 1 < w {
                acc += row[x + r + 1];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
    let mut out = vec![0f32; w * h];
    for x in 0..w {
        let mut acc: f32 = (0..=r.min(h - 1)).map(|y| tmp[y * w + x]).sum();
        for y in 0..h {
            out[y * w + x] = acc;
            if y + r + 1 < h {
                acc += tmp[(y + r + 1) * w + x];
            }
            if y >= r {
                acc -= tmp[(y - r) * w + x];
            }
        }
    }
    out
}

/// What the module was asked for, from the options.
#[derive(Debug, Clone)]
pub struct Request {
    /// The rule.
    pub rule: &'static Rule,
    /// How to process.
    pub params: Params,
    /// `single`, `sheet` or `both`.
    pub output: String,
    /// Paper for the sheet.
    pub paper: String,
    /// Copies on the sheet (`None` fills the sheet).
    pub copies: Option<u32>,
    /// Print dpi override for the pixel size and the sheet.
    pub dpi: Option<u32>,
    /// Treat a failed check as an error and write nothing.
    pub strict: bool,
    /// Write the report as a JSON file next to the photo.
    pub report: bool,
    /// The KB reading to use ("both", "1000", "1024").
    pub kb_basis: Option<String>,
}

/// The country of this computer's region (`en_IN` is `IN`), when there is a rule for it.
fn region_country() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .and_then(|l| {
            l.split(['.', '@'])
                .next()
                .and_then(|x| x.split('_').nth(1))
                .map(str::to_string)
        })
        .filter(|c| rules::find(c, None).is_ok())
}

/// Read the options.
pub fn request_from(opts: &Opts) -> Result<Request> {
    let region = region_country();
    let key = match opts.str("country") {
        Some(k) => k.to_string(),
        None => region.ok_or_else(|| {
            ImageError::BadOption(
                "Say which country's rule to use, for example country=IN or country=US.".into(),
            )
        })?,
    };
    let rule = rules::find(&key, opts.str("document"))?;
    let params = Params {
        engine: EnginePref::from_opt(opts.str("engine")),
        face_index: opts.u32("face").map(|v| v as usize),
        background: match opts.str("background") {
            Some(s) => Some(rules::parse_color(s).ok_or_else(|| {
                ImageError::BadOption(
                    "The background color is not a color. Use something like #ffffff.".into(),
                )
            })?),
            None => None,
        },
        whiten: opts.flag("whiten", true),
        fix_tilt: opts.flag("fix_tilt", true),
        fix_exposure: opts.flag("fix_exposure", true),
        color_cutout: opts.str("cutout") == Some("color"),
    };
    Ok(Request {
        rule,
        params,
        output: opts.str("output").unwrap_or("single").to_string(),
        paper: opts
            .str("paper")
            .unwrap_or_else(|| sheet::default_paper())
            .to_string(),
        copies: opts.u32("copies"),
        dpi: opts.u32("dpi"),
        strict: opts.flag("strict", false),
        report: opts.flag("report", false),
        kb_basis: opts.str("kb_basis").map(str::to_string),
    })
}

/// Run the module on each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let req = request_from(opts)?;
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Making the photo from {}", display_name(path)),
        );
        match one(io, path, &req) {
            Ok(mut v) => outputs.append(&mut v),
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) if inputs.len() > 1 => {
                io.note(format!("{}: skipped. {e}", display_name(path)));
                first_err.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    if outputs.is_empty()
        && let Some(e) = first_err
    {
        return Err(e);
    }
    Ok(outputs)
}

fn one(io: &dyn Io, path: &Path, req: &Request) -> Result<Vec<PathBuf>> {
    let name = display_name(path);
    let rule = req.rule;
    let up = load_upright(io, path, SOURCE_CAP, Some([255, 255, 255]))?;
    for n in &up.notes {
        io.note(format!("{name}: {n}"));
    }
    io.progress(0.25, "Finding the face");
    let pre = prepare(up.image, rule, &req.params)?;
    check_cancel(io)?;
    let want_single = req.output != "sheet";
    let want_sheet = req.output != "single";
    let mut written = Vec::new();

    // The single file: portal pixels (or millimetres at the dpi), inside the size window.
    let (w, h) = rule.pixels(req.dpi);
    // The resolution the file's pixels have at the photo's real height (a portal's 630 x 810 px
    // for a 45 mm photo is 457 DPI), so a print of the file is the right size.
    let eff_dpi = ((f64::from(h) / rule.height_mm * 25.4).round() as u32).clamp(72, 1200);
    let made = render(&pre, rule, &req.params, w, h)?;
    let mut report = made.report.clone();
    if want_single {
        io.progress(0.6, "Fitting the file size");
        let window = rule.size_window(req.kb_basis.as_deref())?;
        let preset = match &rule.size_preset {
            Some(id) => Some(panoply_image::presets::find(id)?),
            None => None,
        };
        let spec = Spec {
            pixels: Some((w, h)),
            window,
            format: Format::Jpeg,
            background: req
                .params
                .background
                .unwrap_or_else(|| rule.background_rgb()),
            baseline: true,
            fit: FitChoice::Stretch,
            anchor_y: 0.5,
            pad_to_min: true,
            preset,
            published: rule.published_kb(),
        };
        let _ = Fit::Stretch;
        let (bytes, size_notes, size_status) = if window.is_some() {
            let sol = kb::solve(&made.image, &spec, io)?;
            (sol.bytes, sol.notes, Status::Pass)
        } else {
            let b = jpeg_bytes(&made.image, 92.0, false, eff_dpi)?;
            let line = format!("{w} x {h} px, {} bytes", with_commas(b.len() as u64));
            (b, vec![line], Status::Pass)
        };
        let mut bytes = bytes;
        set_jfif_density(&mut bytes, eff_dpi);
        let mut c = check(
            "file",
            "File",
            size_status,
            size_notes.first().cloned().unwrap_or_default(),
        );
        c.detail = c.detail.trim_end_matches('.').to_string();
        report.checks.push(c);
        if req.strict && report.overall() == Status::Fail {
            return Err(ImageError::Failed(format!(
                "The photo does not meet the {} rule: {}. Nothing was written.",
                rule.name,
                report
                    .checks
                    .iter()
                    .filter(|c| c.status == Status::Fail)
                    .map(|c| c.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let label = format!("{} {w}x{h}", rule.id);
        let out = write_output(io, path, &NameSpec::Label(label), "jpg", &bytes)?;
        io.note(summary_line(&name, rule, w, h, bytes.len() as u64, &report));
        for l in report.lines() {
            io.note(format!("{name}: {l}"));
        }
        for n in size_notes.iter().skip(1) {
            io.note(format!("{name}: {n}"));
        }
        io.note(format!(
            "{name}: rule {}, {} ({}), source {}. This meets the published rule; it does not promise the authority accepts the photo",
            rule.id,
            rule.authority,
            match rule.status.as_str() {
                "verified" => "verified",
                "conflict" => "sources disagree, the stricter reading was used",
                _ => "not recently verified",
            },
            rule.source_url
        ));
        if req.report {
            let json = serde_json::to_vec_pretty(&report)
                .map_err(|e| ImageError::Failed(format!("Could not write the report: {e}")))?;
            let rp = write_output(
                io,
                path,
                &NameSpec::Label(format!("{} check", rule.id)),
                "json",
                &json,
            )?;
            written.push(rp);
        }
        written.insert(0, out);
    }
    if want_sheet {
        io.progress(0.85, "Laying out the sheet");
        let dpi = req.dpi.unwrap_or(rule.dpi);
        let (pw, ph) = (
            (rule.width_mm / 25.4 * f64::from(dpi)).round() as u32,
            (rule.height_mm / 25.4 * f64::from(dpi)).round() as u32,
        );
        let print = if (pw, ph) == (w, h) {
            made.image.clone()
        } else {
            render(&pre, rule, &req.params, pw, ph)?.image
        };
        let spec = sheet::SheetSpec {
            paper: sheet::paper(&req.paper)?,
            cell_mm: (rule.width_mm, rule.height_mm),
            margin_mm: 3.0,
            gutter_mm: 2.0,
            copies: req.copies,
            cut_marks: true,
            rotate: true,
            dpi,
        };
        let made_sheet = sheet::make(&[print], &spec)?;
        let bytes = sheet::encode(&made_sheet, "jpg", dpi)?;
        let out = write_output(
            io,
            path,
            &NameSpec::Label(format!("{} sheet", rule.id)),
            "jpg",
            &bytes,
        )?;
        io.note(format!("{name}: {}", made_sheet.card_line(dpi)));
        written.push(out);
    }
    let _ = human_bytes;
    Ok(written)
}

fn summary_line(name: &str, rule: &Rule, w: u32, h: u32, bytes: u64, report: &Report) -> String {
    let kb = bytes as f64 / 1000.0;
    let verdict = match report.overall() {
        Status::Pass => "all checks passed".to_string(),
        Status::Warn => format!(
            "{} warning{}, no failures",
            report
                .checks
                .iter()
                .filter(|c| c.status == Status::Warn)
                .count(),
            if report
                .checks
                .iter()
                .filter(|c| c.status == Status::Warn)
                .count()
                == 1
            {
                ""
            } else {
                "s"
            }
        ),
        Status::Fail => format!(
            "{} check{} failed",
            report
                .checks
                .iter()
                .filter(|c| c.status == Status::Fail)
                .count(),
            if report
                .checks
                .iter()
                .filter(|c| c.status == Status::Fail)
                .count()
                == 1
            {
                ""
            } else {
                "s"
            }
        ),
    };
    format!(
        "{name}: {}, {} x {} mm, {w} x {h} px, {kb:.0} KB, {verdict}.",
        rule.name,
        trim(rule.width_mm),
        trim(rule.height_mm)
    )
}

fn trim(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// An RGB picture from a layout check, used by tests.
#[doc(hidden)]
pub fn is_rgb(img: &Image) -> bool {
    img.layout == Layout::Rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_status_has_a_warn_band() {
        assert_eq!(range_status(33.0, 31.0, 36.0, 1.0), Status::Pass);
        assert_eq!(range_status(36.8, 31.0, 36.0, 1.0), Status::Warn);
        assert_eq!(range_status(38.0, 31.0, 36.0, 1.0), Status::Fail);
        assert_eq!(range_status(29.5, 31.0, 36.0, 1.0), Status::Fail);
    }

    #[test]
    fn gamma_brightens_a_dark_picture() {
        let mut img = Image {
            width: 2,
            height: 1,
            layout: Layout::Rgb,
            samples: Samples::U8(vec![40, 40, 40, 100, 100, 100]),
            icc: None,
        };
        apply_gamma(&mut img, 0.6);
        let Samples::U8(v) = &img.samples else {
            panic!()
        };
        assert!(v[0] > 40 && v[3] > 100);
    }

    #[test]
    fn a_composite_puts_the_person_on_the_new_backdrop() {
        let (w, h) = (20usize, 20usize);
        let mut px = vec![0u8; w * h * 3];
        let mut a = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if (5..15).contains(&x) && (5..15).contains(&y) {
                    px[i * 3..i * 3 + 3].copy_from_slice(&[200, 100, 80]);
                    a[i] = 255;
                } else {
                    px[i * 3..i * 3 + 3].copy_from_slice(&[40, 80, 200]);
                }
            }
        }
        let mut fg = Image {
            width: 20,
            height: 20,
            layout: Layout::Rgb,
            samples: Samples::U8(px),
            icc: None,
        };
        let alpha = Image {
            width: 20,
            height: 20,
            layout: Layout::Gray,
            samples: Samples::U8(a),
            icc: None,
        };
        composite(&mut fg, &alpha, [255, 255, 255]);
        let Samples::U8(v) = &fg.samples else {
            panic!()
        };
        assert_eq!(&v[0..3], &[255, 255, 255]);
        let c = (10 * w + 10) * 3;
        assert_eq!(&v[c..c + 3], &[200, 100, 80]);
    }

    #[test]
    fn a_blur_of_zero_stays_zero_and_conserves_a_point() {
        let mut v = vec![0f32; 7 * 7];
        v[3 * 7 + 3] = 1.0;
        let b = box_blur(&v, 7, 7, 1);
        let total: f32 = b.iter().sum();
        assert!((total - 9.0).abs() < 1e-4, "{total}");
    }

    #[test]
    fn colour_names_are_plain() {
        assert!(colour_name([255, 255, 255]).starts_with("white"));
        assert!(colour_name([70, 110, 190]).starts_with("blue"));
        assert!(colour_name([120, 120, 120]).starts_with("grey"));
    }

    #[test]
    fn one_face_is_chosen_and_two_equal_faces_ask() {
        let mk = |x: f64, s: f64| Face {
            x,
            y: 0.0,
            w: s,
            h: s,
            confidence: 1.0,
            eye_l: (0.0, 0.0),
            eye_r: (1.0, 0.0),
            nose: (0.0, 0.0),
            mouth_l: (0.0, 0.0),
            mouth_r: (0.0, 0.0),
            chin: None,
            jaw: vec![],
            yaw_deg: None,
            eye_open: None,
            mouth_open: None,
            engine: Engine::YuNet,
        };
        assert!(choose_face(vec![mk(0.0, 100.0)], None).is_ok());
        let e = choose_face(vec![mk(0.0, 100.0), mk(300.0, 100.0)], None)
            .err()
            .unwrap();
        assert!(e.to_string().contains("which one"), "{e}");
        // A much bigger face is the subject.
        let (f, n) = choose_face(vec![mk(0.0, 300.0), mk(500.0, 100.0)], None).unwrap();
        assert_eq!((f.x, n), (0.0, 2));
        // An explicit choice counts from the left.
        let (f, _) = choose_face(vec![mk(0.0, 100.0), mk(300.0, 100.0)], Some(2)).unwrap();
        assert_eq!(f.x, 300.0);
        assert!(
            choose_face(vec![], None)
                .err()
                .unwrap()
                .to_string()
                .contains("No face")
        );
    }
}
