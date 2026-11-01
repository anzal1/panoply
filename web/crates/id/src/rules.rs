//! Country rule files for the passport and visa photo (docs/FLOWS.md, worked example 2).
//!
//! One TOML file per country and document under `modules/builtin/id-passport-photo/rules/`,
//! embedded in the binary. Every file carries its source link, a quote from the source, the date
//! of the source and the day it was last checked, and a status (`verified`, `conflict`, `stale`),
//! so the UI can show where the numbers came from. Users can add or replace rules with TOML
//! files in the folder named by `PANOPLY_ID_RULES` (a rule with the same id replaces the shipped
//! one).
//!
//! All lengths are millimetres. A head is measured the way most authorities measure it: from the
//! chin to the top of the head (`head_basis = "chin_to_crown"`, hair included unless the file
//! says otherwise in `notes`).

use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{ImageError, Result};

include!(concat!(env!("OUT_DIR"), "/rules_gen.rs"));

/// A source that disagrees with the main numbers.
#[derive(Debug, Clone, Deserialize, serde::Serialize, Default)]
pub struct Conflict {
    /// Where it was read.
    #[serde(default)]
    pub source_url: String,
    /// What it says.
    #[serde(default)]
    pub source_quote: String,
}

fn d300() -> u32 {
    300
}
fn white() -> String {
    "#ffffff".into()
}
fn chin_to_crown() -> String {
    "chin_to_crown".into()
}
fn tilt() -> f64 {
    5.0
}

/// One country and one document.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Rule {
    /// Stable id, for example `us-passport`.
    pub id: String,
    /// Two letter country code (upper case), or `SCHENGEN` for the area wide visa photo.
    pub country: String,
    /// The country's name for people.
    #[serde(default)]
    pub country_name: String,
    /// Other names that select this rule (other country codes, `EU`, the country's English name).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// `passport` or `visa`.
    pub document: String,
    /// Name for the result card, for example "US passport photo".
    pub name: String,
    /// Who sets the rule.
    #[serde(default)]
    pub authority: String,
    /// Printed photo width in millimetres.
    pub width_mm: f64,
    /// Printed photo height in millimetres.
    pub height_mm: f64,
    /// Print resolution used to turn millimetres into pixels. Default 300.
    #[serde(default = "d300")]
    pub dpi: u32,
    /// Exact pixel width when the portal states pixels (digital upload), else derived from mm.
    pub width_px: Option<u32>,
    /// Exact pixel height when the portal states pixels.
    pub height_px: Option<u32>,
    /// Head height, smallest, millimetres.
    pub head_min_mm: Option<f64>,
    /// Head height, largest, millimetres.
    pub head_max_mm: Option<f64>,
    /// Head height, smallest, percent of the photo height (when the source gives percent).
    pub head_min_pct: Option<f64>,
    /// Head height, largest, percent of the photo height.
    pub head_max_pct: Option<f64>,
    /// What the head height measures. `chin_to_crown` (the default) includes hair.
    #[serde(default = "chin_to_crown")]
    pub head_basis: String,
    /// Eye line height above the bottom edge, smallest, millimetres.
    pub eye_min_mm: Option<f64>,
    /// Eye line height above the bottom edge, largest, millimetres.
    pub eye_max_mm: Option<f64>,
    /// Eye line height above the bottom edge, smallest, percent of the photo height.
    pub eye_min_pct: Option<f64>,
    /// Eye line height above the bottom edge, largest, percent of the photo height.
    pub eye_max_pct: Option<f64>,
    /// Required background colour: `#rrggbb`, or `white`.
    #[serde(default = "white")]
    pub background: String,
    /// Largest head tilt in degrees. Default 5.
    #[serde(default = "tilt")]
    pub max_tilt_deg: f64,
    /// Smallest allowed file size, KB as published.
    pub min_kb: Option<f64>,
    /// Largest allowed file size, KB as published.
    pub max_kb: Option<f64>,
    /// "1000", "1024" or "unknown" (the default when a limit is given).
    #[serde(default)]
    pub kb_basis: String,
    /// An `image.resize_to_kb` preset to take the size window (and baseline setting) from.
    pub size_preset: Option<String>,
    /// `allowed`, `not_allowed` or `no_glare`.
    #[serde(default)]
    pub glasses: String,
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
    /// `verified`, `conflict` or `stale`.
    #[serde(default)]
    pub status: String,
    /// Plain notes: children, head coverings, anything the numbers do not say.
    #[serde(default)]
    pub notes: String,
    /// Sources that disagree.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

impl Rule {
    /// Output pixel size: the portal's pixels, else millimetres at the rule's dpi (or `dpi`).
    pub fn pixels(&self, dpi: Option<u32>) -> (u32, u32) {
        if dpi.is_none()
            && let (Some(w), Some(h)) = (self.width_px, self.height_px)
        {
            return (w, h);
        }
        let d = f64::from(dpi.unwrap_or(self.dpi));
        (
            (self.width_mm / 25.4 * d).round().max(1.0) as u32,
            (self.height_mm / 25.4 * d).round().max(1.0) as u32,
        )
    }

    /// Head height range in millimetres for this photo size.
    pub fn head_range_mm(&self) -> Option<(f64, f64)> {
        let h = self.height_mm;
        let lo = self
            .head_min_mm
            .or(self.head_min_pct.map(|p| p / 100.0 * h));
        let hi = self
            .head_max_mm
            .or(self.head_max_pct.map(|p| p / 100.0 * h));
        match (lo, hi) {
            (Some(a), Some(b)) => Some((a, b)),
            (Some(a), None) => Some((a, h)),
            (None, Some(b)) => Some((0.0, b)),
            _ => None,
        }
    }

    /// Eye line height above the bottom edge, range in millimetres.
    pub fn eye_range_mm(&self) -> Option<(f64, f64)> {
        let h = self.height_mm;
        let lo = self.eye_min_mm.or(self.eye_min_pct.map(|p| p / 100.0 * h));
        let hi = self.eye_max_mm.or(self.eye_max_pct.map(|p| p / 100.0 * h));
        match (lo, hi) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }

    /// The background as RGB.
    pub fn background_rgb(&self) -> [u8; 3] {
        parse_color(&self.background).unwrap_or([255, 255, 255])
    }


}

/// `#rrggbb`, `rrggbb`, or `white`.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim().to_ascii_lowercase();
    match t.as_str() {
        "white" => return Some([255, 255, 255]),
        "off-white" | "offwhite" => return Some([245, 245, 245]),
        "light grey" | "light gray" | "grey" | "gray" => return Some([220, 220, 220]),
        "light blue" => return Some([200, 220, 240]),
        "blue" => return Some([70, 130, 220]),
        _ => {}
    }
    let h = t.trim_start_matches('#');
    if h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        let v = u32::from_str_radix(h, 16).ok()?;
        return Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    }
    None
}

fn parse(text: &str) -> Option<Rule> {
    toml::from_str(text).ok()
}

/// Every rule: shipped ones, then any in `PANOPLY_ID_RULES` (which replace shipped ones with the
/// same id).
pub fn all() -> &'static [Rule] {
    static ALL: OnceLock<Vec<Rule>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<Rule> = RULE_FILES.iter().filter_map(|(_, t)| parse(t)).collect();
        if let Some(dir) = std::env::var_os("PANOPLY_ID_RULES")
            && let Ok(rd) = std::fs::read_dir(dir)
        {
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "toml")
                    && let Some(r) = std::fs::read_to_string(e.path())
                        .ok()
                        .and_then(|t| parse(&t))
                {
                    v.retain(|q| q.id != r.id);
                    v.push(r);
                }
            }
        }
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    })
}

fn matches_country(r: &Rule, key: &str) -> bool {
    let k = key.trim().to_ascii_lowercase();
    r.country.to_ascii_lowercase() == k
        || r.country_name.to_ascii_lowercase() == k
        || r.aliases.iter().any(|a| a.to_ascii_lowercase() == k)
}

/// Find the rule for `key`: a rule id (`us-visa`), or a country (`IN`, `India`, `Schengen`, `UK`)
/// together with `document` (`passport` by default). When the country has only one rule that
/// one is used whatever the document.
pub fn find(key: &str, document: Option<&str>) -> Result<&'static Rule> {
    let list = all();
    if let Some(r) = list.iter().find(|r| r.id.eq_ignore_ascii_case(key.trim())) {
        return Ok(r);
    }
    let cands: Vec<&Rule> = list.iter().filter(|r| matches_country(r, key)).collect();
    let want = document.unwrap_or("passport");
    if let Some(r) = cands.iter().find(|r| r.document.eq_ignore_ascii_case(want)) {
        return Ok(r);
    }
    if cands.len() == 1 && document.is_none() {
        return Ok(cands[0]);
    }
    if !cands.is_empty() {
        let ds: Vec<&str> = cands.iter().map(|r| r.document.as_str()).collect();
        return Err(ImageError::BadOption(format!(
            "There is no {want} photo rule for \"{key}\". It has rules for: {}.",
            ds.join(", ")
        )));
    }
    let ids: Vec<&str> = list.iter().map(|r| r.id.as_str()).collect();
    Err(ImageError::BadOption(format!(
        "There is no photo rule for \"{key}\". The rules are: {}.",
        ids.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#ffffff"), Some([255, 255, 255]));
        assert_eq!(parse_color("White"), Some([255, 255, 255]));
        assert_eq!(parse_color("1a2b3c"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn every_shipped_rule_parses_and_cites_its_source() {
        assert!(!RULE_FILES.is_empty(), "no rule files were embedded");
        assert_eq!(
            all().len(),
            RULE_FILES.len(),
            "a rule file failed to parse (or two files share an id)"
        );
        for (stem, _) in RULE_FILES {
            let r = all()
                .iter()
                .find(|r| r.id == *stem)
                .unwrap_or_else(|| panic!("file {stem} has a different id inside"));
            assert!(
                r.source_url.starts_with("http"),
                "{} has no source link",
                r.id
            );
            assert!(!r.source_quote.is_empty(), "{} has no quote", r.id);
            assert!(!r.last_verified.is_empty(), "{} has no check date", r.id);
            assert!(
                ["verified", "conflict", "stale"].contains(&r.status.as_str()),
                "{}: status {}",
                r.id,
                r.status
            );
            assert!(r.width_mm > 10.0 && r.height_mm > 10.0, "{}", r.id);
            let (lo, hi) = r
                .head_range_mm()
                .unwrap_or_else(|| panic!("{} has no head size rule", r.id));
            assert!(lo > 0.0 && lo < hi && hi <= r.height_mm, "{}", r.id);
            if let Some((a, b)) = r.eye_range_mm() {
                assert!(a < b && b <= r.height_mm, "{}", r.id);
            }
            assert!(parse_color(&r.background).is_some(), "{}", r.id);
            // Any size window must be a real one (no conflict, no unknown preset).
            r.size_window(None)
                .unwrap_or_else(|e| panic!("{}: {e}", r.id));
        }
    }

    #[test]
    fn the_nine_countries_are_there() {
        for c in ["IN", "US", "GB", "SCHENGEN", "CA", "BD", "AE", "CN", "AU"] {
            find(c, None).unwrap_or_else(|e| panic!("{c}: {e}"));
        }
        assert_eq!(find("uk", None).unwrap().country, "GB");
        assert_eq!(find("India", None).unwrap().country, "IN");
    }

    #[test]
    fn a_missing_document_says_what_exists() {
        let e = find("US", Some("residence")).unwrap_err().to_string();
        assert!(e.contains("passport"), "{e}");
    }

    #[test]
    fn a_made_up_country_lists_the_rules() {
        let e = find("Atlantis", None).unwrap_err().to_string();
        assert!(e.contains("us-passport"), "{e}");
    }
}
