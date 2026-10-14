//! Exam and portal presets (docs/specs/id-and-image.md, "Exam preset data format").
//!
//! One TOML file per portal and field under `modules/builtin/image-resize-to-kb/presets/`,
//! embedded in the binary. Each carries its source link, quote and date, and a status
//! (`verified`, `conflict`, `stale`) so the UI can show where the numbers came from. Users can
//! add or replace presets with TOML files in the folder named by `PANOPLY_PRESETS`.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{ImageError, Result};

include!(concat!(env!("OUT_DIR"), "/presets_gen.rs"));

/// A source that disagrees with the main numbers.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Conflict {
    /// Published minimum.
    #[serde(default)]
    pub min_bytes_claimed: f64,
    /// Published maximum.
    #[serde(default)]
    pub max_bytes_claimed: f64,
    /// Unit of the two numbers.
    #[serde(default)]
    pub unit_claimed: String,
    /// Published pixel width.
    pub width_px: Option<u32>,
    /// Published pixel height.
    pub height_px: Option<u32>,
    /// Where it was read.
    #[serde(default)]
    pub source_url: String,
    /// What it says.
    #[serde(default)]
    pub source_quote: String,
}

fn yes() -> bool {
    true
}

/// One portal and field.
#[derive(Debug, Clone, Deserialize)]
pub struct Preset {
    /// Stable id, for example `ssc-photo`.
    pub id: String,
    /// Who sets the rule.
    #[serde(default)]
    pub authority: String,
    /// The form or exam.
    #[serde(default)]
    pub form: String,
    /// photo, signature, thumb, declaration or document.
    #[serde(default)]
    pub field: String,
    /// Exact pixel width, when the source gives pixels.
    pub width_px: Option<u32>,
    /// Exact pixel height.
    pub height_px: Option<u32>,
    /// How far from the exact size the portal accepts.
    #[serde(default)]
    pub tolerance_px: u32,
    /// Physical size in millimetres, width then height.
    pub physical_mm: Option<[f64; 2]>,
    /// Resolution for turning millimetres into pixels.
    pub dpi: Option<u32>,
    /// Published minimum, in `unit_claimed`.
    #[serde(default)]
    pub min_bytes_claimed: f64,
    /// Published maximum, in `unit_claimed`. 0 means no limit was found.
    #[serde(default)]
    pub max_bytes_claimed: f64,
    /// KB, MB or bytes.
    #[serde(default)]
    pub unit_claimed: String,
    /// "1000", "1024" or "unknown".
    #[serde(default)]
    pub kb_basis: String,
    /// jpg, png or pdf.
    #[serde(default)]
    pub format: String,
    /// Required background color.
    pub background: Option<String>,
    /// rgb, gray or bw.
    #[serde(default)]
    pub color_mode: String,
    /// The portal rejects progressive JPEG.
    #[serde(default = "yes")]
    pub baseline_only: bool,
    /// Strip EXIF and the color profile.
    #[serde(default = "yes")]
    pub strip_metadata: bool,
    /// verified, conflict or stale.
    #[serde(default)]
    pub status: String,
    /// official, third-party or none.
    #[serde(default)]
    pub verified_against: String,
    /// Where the rule was read.
    #[serde(default)]
    pub source_url: String,
    /// Words from the source.
    #[serde(default)]
    pub source_quote: String,
    /// Date printed on the source.
    #[serde(default)]
    pub source_date: String,
    /// Day it was last checked.
    #[serde(default)]
    pub last_verified: String,
    /// Plain notes.
    #[serde(default)]
    pub notes: String,
    /// Sources that disagree.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

/// The byte window a result must land in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Smallest allowed size in bytes.
    pub lo: u64,
    /// Largest allowed size in bytes, `None` when there is no upper limit.
    pub hi: Option<u64>,
}

impl Window {
    /// True when `bytes` is inside the window.
    pub fn contains(&self, bytes: u64) -> bool {
        bytes >= self.lo && self.hi.is_none_or(|h| bytes <= h)
    }
}

fn unit_factor(unit: &str, base: u64) -> u64 {
    match unit.to_ascii_uppercase().as_str() {
        "MB" => base * base,
        "BYTES" | "B" => 1,
        _ => base,
    }
}

/// The window for published limits. A known `basis` ("1000" or "1024") is used as is. When
/// the basis is unknown ("both"), the window is the part that holds under both readings:
/// at least `min * 1024` and at most `max * 1000` (docs/specs/id-and-image.md, KB handling).
/// An empty window is a conflict the caller must show.
pub fn window(min: f64, max: f64, unit: &str, basis: &str) -> Result<Window> {
    let scale = |v: f64, base: u64| (v * unit_factor(unit, base) as f64).round() as u64;
    let (lo, hi) = match basis {
        "1000" => (scale(min, 1000), (max > 0.0).then(|| scale(max, 1000))),
        "1024" => (scale(min, 1024), (max > 0.0).then(|| scale(max, 1024))),
        _ => (scale(min, 1024), (max > 0.0).then(|| scale(max, 1000))),
    };
    if let Some(h) = hi
        && lo > h
    {
        return Err(ImageError::Failed(format!(
            "The published limits only fit under one reading of KB: {min} to {max} {unit} is {} to {} bytes counting 1000 per KB, and {} to {} bytes counting 1024 per KB, and no size is inside both. Choose which reading the portal uses (kb_basis = 1000 or 1024).",
            scale(min, 1000),
            scale(max, 1000),
            scale(min, 1024),
            scale(max, 1024)
        )));
    }
    Ok(Window { lo, hi })
}

impl Preset {
    /// Pixel size: the published pixels, else millimetres at the preset's dpi.
    pub fn pixels(&self) -> Option<(u32, u32)> {
        match (self.width_px, self.height_px) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => {
                let (mm, dpi) = (self.physical_mm?, self.dpi? as f64);
                Some((
                    (mm[0] / 25.4 * dpi).round().max(1.0) as u32,
                    (mm[1] / 25.4 * dpi).round().max(1.0) as u32,
                ))
            }
        }
    }

    /// The byte window, honouring `basis_override` ("1000", "1024" or "both").
    pub fn window(&self, basis_override: Option<&str>) -> Result<Window> {
        let basis = basis_override.unwrap_or(&self.kb_basis);
        window(
            self.min_bytes_claimed,
            self.max_bytes_claimed,
            &self.unit_claimed,
            basis,
        )
    }

    /// True when there is a limit to meet.
    pub fn has_size_rule(&self) -> bool {
        self.max_bytes_claimed > 0.0
    }
}

fn parse(text: &str) -> Option<Preset> {
    toml::from_str(text).ok()
}

/// Every preset: shipped ones, then any in `PANOPLY_PRESETS` (which replace shipped ones with
/// the same id).
pub fn all() -> &'static [Preset] {
    static ALL: OnceLock<Vec<Preset>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<Preset> = PRESET_FILES.iter().filter_map(|(_, t)| parse(t)).collect();
        if let Some(dir) = std::env::var_os("PANOPLY_PRESETS")
            && let Ok(rd) = std::fs::read_dir(dir)
        {
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "toml")
                    && let Some(p) = std::fs::read_to_string(e.path())
                        .ok()
                        .and_then(|t| parse(&t))
                {
                    v.retain(|q| q.id != p.id);
                    v.push(p);
                }
            }
        }
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    })
}

/// Find a preset by id.
pub fn find(id: &str) -> Result<&'static Preset> {
    all().iter().find(|p| p.id == id).ok_or_else(|| {
        let ids: Vec<&str> = all().iter().map(|p| p.id.as_str()).collect();
        ImageError::BadOption(format!(
            "There is no preset called \"{id}\". The presets are: {}.",
            ids.join(", ")
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_file_parses_and_has_a_source() {
        assert!(PRESET_FILES.len() >= 14, "{} presets", PRESET_FILES.len());
        assert_eq!(
            all().len(),
            PRESET_FILES.len(),
            "a preset file failed to parse"
        );
        for (stem, _) in PRESET_FILES {
            let p = find(stem).unwrap();
            assert!(
                p.source_url.starts_with("http"),
                "{} has no source link",
                p.id
            );
            assert!(!p.source_quote.is_empty(), "{} has no quote", p.id);
            assert!(
                ["verified", "conflict", "stale"].contains(&p.status.as_str()),
                "{}: status {}",
                p.id,
                p.status
            );
        }
    }

    #[test]
    fn unknown_basis_uses_the_intersection() {
        let w = window(20.0, 50.0, "KB", "unknown").unwrap();
        assert_eq!((w.lo, w.hi), (20_480, Some(50_000)));
        let w = window(20.0, 50.0, "KB", "1024").unwrap();
        assert_eq!((w.lo, w.hi), (20_480, Some(51_200)));
        let w = window(0.0, 30.0, "KB", "1000").unwrap();
        assert_eq!((w.lo, w.hi), (0, Some(30_000)));
    }

    #[test]
    fn an_empty_intersection_is_a_conflict() {
        let e = window(48.0, 49.0, "KB", "unknown").unwrap_err();
        assert!(e.to_string().contains("no size is inside both"));
    }

    #[test]
    fn millimetres_become_pixels() {
        let p = find("ssc-photo").unwrap();
        assert_eq!(p.pixels(), Some((276, 354)));
    }
}
