//! Faces and landmarks, whichever engine found them.
//!
//! The engine ladder (docs/FLOWS.md, ADR 0004): Apple Vision on macOS (box, jaw line, eye and lip
//! outlines), then YuNet through ONNX Runtime everywhere else (box and five points: both eyes,
//! the nose tip, both mouth corners). `Face` is what the rest of the module reads, so a check
//! either has the data it needs or says it was not checked on this engine.
//!
//! Coordinates are picture pixels with the origin at the top left. "Left" and "right" are the
//! sides of the picture, not the subject's.

use panoply_image::error::{ImageError, Result};
use panoply_image::ops::kb::resample_image;
use panoply_image::types::Image;
#[cfg(target_os = "macos")]
use panoply_image::types::Samples;

use crate::yunet;

/// A point in picture pixels.
pub type P = (f64, f64);

/// Which engine found a face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Apple Vision.
    Vision,
    /// YuNet through ONNX Runtime.
    YuNet,
}

impl Engine {
    /// A name for the result card.
    pub fn label(self) -> &'static str {
        match self {
            Engine::Vision => "Apple Vision",
            Engine::YuNet => "YuNet",
        }
    }
}

/// What the caller wants used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePref {
    /// The OS engine when there is one, else YuNet.
    Auto,
    /// Apple Vision only.
    Vision,
    /// YuNet only.
    YuNet,
}

impl EnginePref {
    /// From the `engine` option.
    pub fn from_opt(s: Option<&str>) -> Self {
        match s {
            Some("vision") => Self::Vision,
            Some("yunet") => Self::YuNet,
            _ => Self::Auto,
        }
    }
}

/// One face.
#[derive(Debug, Clone)]
pub struct Face {
    /// Face box left edge.
    pub x: f64,
    /// Face box top edge.
    pub y: f64,
    /// Face box width.
    pub w: f64,
    /// Face box height.
    pub h: f64,
    /// Detector confidence, 0 to 1.
    pub confidence: f32,
    /// Centre of the eye on the left of the picture.
    pub eye_l: P,
    /// Centre of the eye on the right of the picture.
    pub eye_r: P,
    /// Nose tip (or centre of the nose).
    pub nose: P,
    /// Mouth corner on the left of the picture.
    pub mouth_l: P,
    /// Mouth corner on the right of the picture.
    pub mouth_r: P,
    /// The lowest point of the jaw, when the engine measures it.
    pub chin: Option<P>,
    /// Jaw line points, when the engine gives them.
    pub jaw: Vec<P>,
    /// Yaw in degrees as the engine reports it, when it does.
    pub yaw_deg: Option<f64>,
    /// Eye openness (height over width of the eye outline), smaller of the two, when measured.
    pub eye_open: Option<f64>,
    /// Mouth opening (inner lip height over outer lip width), when measured.
    pub mouth_open: Option<f64>,
    /// The engine.
    pub engine: Engine,
}

/// Where the crown sits relative to the eye-to-mouth distance, measured on the corpus portraits
/// with the system's jaw line and subject mask (`tests/faces.rs` prints the calibration and
/// checks these constants against it): eye line to top of the head (hair included) is about this
/// many eye-to-mouth distances. The seven portraits range from 1.6 to 2.4, mean 2.0, so an
/// estimate is good to about 3 mm on a 35 x 45 mm photo and the report says "estimated".
pub const CROWN_PER_EYE_MOUTH: f64 = 2.0;
/// Chin below the mouth line, in eye-to-mouth distances, for engines without a jaw line (the
/// same portraits range from 0.63 to 0.90, mean 0.83).
pub const CHIN_PER_EYE_MOUTH: f64 = 0.85;

impl Face {
    /// Midpoint of the eyes.
    pub fn eye_mid(&self) -> P {
        (
            (self.eye_l.0 + self.eye_r.0) / 2.0,
            (self.eye_l.1 + self.eye_r.1) / 2.0,
        )
    }

    /// Midpoint of the mouth corners.
    pub fn mouth_mid(&self) -> P {
        (
            (self.mouth_l.0 + self.mouth_r.0) / 2.0,
            (self.mouth_l.1 + self.mouth_r.1) / 2.0,
        )
    }

    /// Distance between the eye centres.
    pub fn eye_dist(&self) -> f64 {
        dist(self.eye_l, self.eye_r)
    }

    /// Head roll in degrees from the eye line: positive when the eye on the right of the
    /// picture is lower than the other (the head leans clockwise on screen).
    pub fn roll_deg(&self) -> f64 {
        (self.eye_r.1 - self.eye_l.1)
            .atan2(self.eye_r.0 - self.eye_l.0)
            .to_degrees()
    }

    /// Sideways turn of the head estimated from the nose position between the eyes, as a
    /// fraction of the eye distance (0 is facing the camera). Works for every engine.
    pub fn turn(&self) -> f64 {
        let a = self.roll_deg().to_radians();
        let (c, s) = (a.cos(), a.sin());
        let em = self.eye_mid();
        let d = (self.nose.0 - em.0, self.nose.1 - em.1);
        // Component along the eye line.
        (d.0 * c + d.1 * s) / self.eye_dist().max(1.0)
    }

    /// Eye line to mouth line distance.
    pub fn eye_mouth(&self) -> f64 {
        dist(self.eye_mid(), self.mouth_mid())
    }

    /// Points rotated into the frame where the eye line is level, about `pivot`.
    pub fn level(&self, p: P, pivot: P) -> P {
        let a = self.roll_deg().to_radians();
        rotate(p, pivot, -a)
    }
}

/// Distance between two points.
pub fn dist(a: P, b: P) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Rotate `p` about `c` by `a` radians (y points down, so positive turns clockwise on screen).
pub fn rotate(p: P, c: P, a: f64) -> P {
    let (s, co) = a.sin_cos();
    let d = (p.0 - c.0, p.1 - c.1);
    (c.0 + d.0 * co - d.1 * s, c.1 + d.0 * s + d.1 * co)
}

/// The longest side a picture is brought to before a face is looked for.
const DETECT_SIDE: u32 = 1280;

/// Find the faces in an 8 bit RGB picture. Faces come back sorted left to right, in the
/// picture's own pixels, with the engine that found them.
pub fn detect(img: &Image, pref: EnginePref) -> Result<(Vec<Face>, Engine)> {
    let rgb = rgb8(img)?;
    let long = rgb.width.max(rgb.height);
    let scale = if long > DETECT_SIDE {
        f64::from(DETECT_SIDE) / f64::from(long)
    } else {
        1.0
    };
    let small;
    let work = if scale < 1.0 {
        small = resample_image(
            &rgb,
            ((f64::from(rgb.width) * scale).round() as u32).max(1),
            ((f64::from(rgb.height) * scale).round() as u32).max(1),
        )?;
        &small
    } else {
        &rgb
    };
    let (mut faces, engine) = run(work, pref)?;
    let inv = 1.0 / scale;
    if scale < 1.0 {
        for f in &mut faces {
            f.scale_by(inv);
        }
    }
    faces.sort_by(|a, b| a.x.total_cmp(&b.x));
    Ok((faces, engine))
}

impl Face {
    fn scale_by(&mut self, k: f64) {
        let sc = |p: &mut P| *p = (p.0 * k, p.1 * k);
        self.x *= k;
        self.y *= k;
        self.w *= k;
        self.h *= k;
        sc(&mut self.eye_l);
        sc(&mut self.eye_r);
        sc(&mut self.nose);
        sc(&mut self.mouth_l);
        sc(&mut self.mouth_r);
        if let Some(c) = &mut self.chin {
            sc(c);
        }
        for p in &mut self.jaw {
            sc(p);
        }
    }
}

fn rgb8(img: &Image) -> Result<Image> {
    let i = img.as_u8_image().into_owned();
    let i = if i.layout.has_alpha() {
        i.without_alpha()
    } else {
        i
    };
    Ok(i.as_rgb_image().into_owned())
}

fn run(img: &Image, pref: EnginePref) -> Result<(Vec<Face>, Engine)> {
    #[cfg(target_os = "macos")]
    {
        if pref != EnginePref::YuNet {
            use panoply_platform_macos::faces;
            if faces::available() {
                let Samples::U8(px) = &img.samples else {
                    return Err(ImageError::Failed(
                        "internal: face detection needs 8 bit RGB".into(),
                    ));
                };
                return faces::detect_faces(px, img.width, img.height)
                    .map(|v| {
                        (
                            v.into_iter().filter_map(from_vision).collect(),
                            Engine::Vision,
                        )
                    })
                    .map_err(|e| ImageError::Failed(e.message().to_string()));
            } else if pref == EnginePref::Vision {
                return Err(ImageError::Unsupported(
                    "Apple Vision is not available on this Mac.".into(),
                ));
            }
        }
    }
    if pref == EnginePref::Vision {
        return Err(ImageError::Unsupported(
            "Apple Vision only exists on macOS. Use the YuNet engine here.".into(),
        ));
    }
    Ok((yunet::detect(img)?, Engine::YuNet))
}

#[cfg(target_os = "macos")]
fn mean(p: &[P]) -> Option<P> {
    if p.is_empty() {
        return None;
    }
    let n = p.len() as f64;
    Some((
        p.iter().map(|q| q.0).sum::<f64>() / n,
        p.iter().map(|q| q.1).sum::<f64>() / n,
    ))
}

#[cfg(target_os = "macos")]
fn extent(p: &[P]) -> Option<(f64, f64, f64, f64)> {
    let first = p.first()?;
    let mut e = (first.0, first.1, first.0, first.1);
    for q in p {
        e = (e.0.min(q.0), e.1.min(q.1), e.2.max(q.0), e.3.max(q.1));
    }
    Some(e)
}

#[cfg(target_os = "macos")]
fn openness(eye: &[P]) -> Option<f64> {
    let (x0, y0, x1, y1) = extent(eye)?;
    let w = x1 - x0;
    (w > 1.0).then(|| (y1 - y0) / w)
}

#[cfg(target_os = "macos")]
fn from_vision(v: panoply_platform_macos::faces::Face) -> Option<Face> {
    let eye_l = mean(&v.pupil_left).or_else(|| mean(&v.eye_left))?;
    let eye_r = mean(&v.pupil_right).or_else(|| mean(&v.eye_right))?;
    let nose = mean(&v.nose)?;
    let (lx0, _, lx1, _) = extent(&v.lips_outer)?;
    let at = |x: f64| {
        v.lips_outer
            .iter()
            .min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs()))
            .copied()
    };
    let (mouth_l, mouth_r) = (at(lx0)?, at(lx1)?);
    let chin = v.contour.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1));
    let eo = match (openness(&v.eye_left), openness(&v.eye_right)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        _ => None,
    };
    let mo = extent(&v.lips_inner).and_then(|(x0, y0, x1, y1)| {
        let w = lx1 - lx0;
        let _ = (x0, x1);
        (w > 1.0).then(|| (y1 - y0) / w)
    });
    Some(Face {
        x: v.x,
        y: v.y,
        w: v.width,
        h: v.height,
        confidence: v.confidence,
        eye_l,
        eye_r,
        nose,
        mouth_l,
        mouth_r,
        chin,
        jaw: v.contour,
        yaw_deg: v.yaw_deg.map(f64::from),
        eye_open: eo,
        mouth_open: mo,
        engine: Engine::Vision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face() -> Face {
        Face {
            x: 80.0,
            y: 60.0,
            w: 100.0,
            h: 120.0,
            confidence: 1.0,
            eye_l: (100.0, 100.0),
            eye_r: (160.0, 100.0),
            nose: (130.0, 130.0),
            mouth_l: (108.0, 150.0),
            mouth_r: (152.0, 150.0),
            chin: None,
            jaw: vec![],
            yaw_deg: None,
            eye_open: None,
            mouth_open: None,
            engine: Engine::YuNet,
        }
    }

    #[test]
    fn a_level_face_has_no_roll_and_no_turn() {
        let f = face();
        assert!(f.roll_deg().abs() < 1e-9);
        assert!(f.turn().abs() < 1e-9);
        assert!((f.eye_mouth() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn roll_is_positive_when_the_right_eye_is_lower() {
        let mut f = face();
        f.eye_r.1 = 110.0;
        assert!(f.roll_deg() > 9.0 && f.roll_deg() < 10.0);
    }

    #[test]
    fn a_turned_nose_shows_in_turn() {
        let mut f = face();
        f.nose.0 = 145.0;
        assert!((f.turn() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn leveling_a_tilted_face_puts_the_eyes_on_one_row() {
        let mut f = face();
        f.eye_l = (100.0, 95.0);
        f.eye_r = (160.0, 105.0);
        let pivot = f.eye_mid();
        let (a, b) = (f.level(f.eye_l, pivot), f.level(f.eye_r, pivot));
        assert!((a.1 - b.1).abs() < 1e-9, "{a:?} {b:?}");
    }

    #[test]
    fn rotation_by_zero_is_identity() {
        let p = (3.0, 4.0);
        let q = rotate(p, (1.0, 1.0), 0.0);
        assert!((p.0 - q.0).abs() < 1e-12 && (p.1 - q.1).abs() < 1e-12);
    }
}
