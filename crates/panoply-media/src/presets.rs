//! Preset data files: upload limits of the services people send video to, and loudness
//! targets. They are data, not code: each file carries its source link, the quote read there
//! and the date it was checked, and a status (`verified`, `conflict`, `stale`, `unverified`),
//! so the interface can show where a number came from and nothing is hardcoded in Rust.
//!
//! Users can add or replace presets with TOML files in the folder named by `PANOPLY_PRESETS`
//! (subfolders `media-compress` and `media-normalize`).

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

include!(concat!(env!("OUT_DIR"), "/size_presets_gen.rs"));
include!(concat!(env!("OUT_DIR"), "/loudness_presets_gen.rs"));

/// A tier of a service (a paid plan with a higher limit).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Tier {
    /// Plan name.
    #[serde(default)]
    pub tier: String,
    /// The cap in bytes.
    #[serde(default)]
    pub limit_bytes: u64,
    /// Where it was read.
    #[serde(default)]
    pub source_url: String,
    /// Status of this figure.
    #[serde(default)]
    pub status: String,
}

/// One generic or official cap in a list.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SizeEntry {
    /// Label shown to the user.
    #[serde(default)]
    pub label: String,
    /// The cap in bytes.
    #[serde(default)]
    pub limit_bytes: u64,
    /// `generic` or `official`.
    #[serde(default)]
    pub origin: String,
    /// Where an official figure was read.
    #[serde(default)]
    pub source_url: String,
}

/// An upload limit file.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SizePreset {
    /// Stable id, for example `whatsapp-video`.
    pub id: String,
    /// The service.
    #[serde(default)]
    pub service: String,
    /// `video` or `audio`.
    #[serde(default)]
    pub kind: String,
    /// The cap in bytes (0 when unknown).
    #[serde(default)]
    pub limit_bytes: u64,
    /// Container the service takes best.
    #[serde(default)]
    pub container: String,
    /// Video codec the service takes best.
    #[serde(default)]
    pub video_codec: String,
    /// Audio codec the service takes best.
    #[serde(default)]
    pub audio_codec: String,
    /// Tallest picture the service states (0 for none).
    #[serde(default)]
    pub max_height: u32,
    /// Longest clip the service states in seconds (0 for none).
    #[serde(default)]
    pub max_duration_s: u32,
    /// `verified`, `conflict`, `stale` or `unverified`.
    #[serde(default)]
    pub status: String,
    /// Where the figure was read.
    #[serde(default)]
    pub source_url: String,
    /// Date shown on the source page.
    #[serde(default)]
    pub source_date: String,
    /// When Panoply last checked.
    #[serde(default)]
    pub last_verified: String,
    /// Plain notes.
    #[serde(default)]
    pub notes: String,
    /// Paid plans.
    #[serde(default)]
    pub tiers: Vec<Tier>,
    /// Lists of caps (form uploads).
    #[serde(default)]
    pub sizes: Vec<SizeEntry>,
}

/// A loudness target file.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoudnessPreset {
    /// Stable id, for example `ebu-r128`.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Integrated loudness target in LUFS.
    #[serde(default)]
    pub target_lufs: f64,
    /// A separate target for mono files, 0 when the source gives none.
    #[serde(default)]
    pub target_lufs_mono: f64,
    /// Plus or minus tolerance in LU.
    #[serde(default)]
    pub tolerance_lu: f64,
    /// True peak ceiling in dBTP (0 means none was sourced).
    #[serde(default)]
    pub true_peak_dbtp: f64,
    /// `integrated`, `program`, `dialogue-gated` or `short-term`.
    #[serde(default)]
    pub measure: String,
    /// `verified`, `conflict`, `stale` or `unverified`.
    #[serde(default)]
    pub status: String,
    /// Where the figure was read.
    #[serde(default)]
    pub source_url: String,
    /// When Panoply last checked.
    #[serde(default)]
    pub last_verified: String,
    /// Plain notes.
    #[serde(default)]
    pub notes: String,
}

fn user_dir(sub: &str) -> Option<PathBuf> {
    std::env::var_os("PANOPLY_PRESETS").map(|d| PathBuf::from(d).join(sub))
}

fn user_files(sub: &str) -> Vec<String> {
    let Some(dir) = user_dir(sub) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

/// Every upload limit preset (built in, then the user's, which win on the same id).
pub fn size_presets() -> &'static [SizePreset] {
    static ALL: OnceLock<Vec<SizePreset>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<SizePreset> = SIZE_PRESET_FILES
            .iter()
            .map(|(name, text)| {
                toml::from_str(text).unwrap_or_else(|e| panic!("preset {name}: {e}"))
            })
            .collect();
        for text in user_files("media-compress") {
            if let Ok(p) = toml::from_str::<SizePreset>(&text) {
                v.retain(|x| x.id != p.id);
                v.push(p);
            }
        }
        v
    })
}

/// Every loudness preset.
pub fn loudness_presets() -> &'static [LoudnessPreset] {
    static ALL: OnceLock<Vec<LoudnessPreset>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<LoudnessPreset> = LOUDNESS_PRESET_FILES
            .iter()
            .map(|(name, text)| {
                toml::from_str(text).unwrap_or_else(|e| panic!("preset {name}: {e}"))
            })
            .collect();
        for text in user_files("media-normalize") {
            if let Ok(p) = toml::from_str::<LoudnessPreset>(&text) {
                v.retain(|x| x.id != p.id);
                v.push(p);
            }
        }
        v
    })
}

/// A cap resolved from the option the user chose.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// What to call it in the result card.
    pub label: String,
    /// The cap in bytes.
    pub bytes: u64,
    /// The preset file it came from.
    pub preset: SizePreset,
}

/// The upload limit for a `preset` option value such as `whatsapp` or `discord_nitro_basic`.
pub fn resolve_size(name: &str) -> Option<Resolved> {
    let (base, tier) = match name {
        "discord_nitro_basic" => ("discord", Some("Nitro Basic")),
        "discord_nitro" => ("discord", Some("Nitro")),
        other => (other, None),
    };
    let id = format!("{base}-video");
    let p = size_presets().iter().find(|p| p.id == id)?.clone();
    let (bytes, label) = match tier {
        Some(t) => {
            let tr = p.tiers.iter().find(|x| x.tier == t)?;
            (tr.limit_bytes, format!("{} {t}", p.service))
        }
        None => (p.limit_bytes, p.service.clone()),
    };
    if bytes == 0 {
        return None;
    }
    Some(Resolved {
        label,
        bytes,
        preset: p,
    })
}

const FLOORS: &str = include_str!("../../../modules/builtin/media-compress/floors.toml");

/// One row of `floors.toml`.
#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct FloorRow {
    /// Bits per pixel per frame below which the picture falls apart.
    #[serde(default)]
    pub floor_bpp: f64,
    /// Bits per pixel per frame where more bits stop buying visible quality.
    #[serde(default)]
    pub knee_bpp: f64,
}

/// The floors for a codec and encoder kind (`h264`, `hevc`, `av1`, `vp9`). A row that was not
/// measured borrows another row times a factor (`estimated_from`, `factor`).
pub fn floors(codec: &str, hardware: bool) -> Option<FloorRow> {
    static ALL: OnceLock<toml::Table> = OnceLock::new();
    let t = ALL.get_or_init(|| toml::from_str(FLOORS).expect("floors.toml parses"));
    let key = format!("{codec}_{}", if hardware { "hardware" } else { "software" });
    resolve_floor(t, &key, 0)
}

fn resolve_floor(t: &toml::Table, key: &str, depth: u8) -> Option<FloorRow> {
    let row = t.get(key)?.as_table()?;
    if let Some(from) = row.get("estimated_from").and_then(|v| v.as_str()) {
        if depth > 2 {
            return None;
        }
        let factor = row.get("factor").and_then(|v| v.as_float()).unwrap_or(1.0);
        let base = resolve_floor(t, from, depth + 1)?;
        return Some(FloorRow {
            floor_bpp: base.floor_bpp * factor,
            knee_bpp: base.knee_bpp * factor,
        });
    }
    Some(FloorRow {
        floor_bpp: row.get("floor_bpp")?.as_float()?,
        knee_bpp: row.get("knee_bpp")?.as_float()?,
    })
}

/// The loudness preset with this id.
pub fn loudness(id: &str) -> Option<&'static LoudnessPreset> {
    loudness_presets().iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_preset_parses_and_has_a_source() {
        assert!(size_presets().len() >= 6);
        for p in size_presets() {
            assert!(!p.id.is_empty() && !p.service.is_empty(), "{}", p.id);
            assert!(!p.last_verified.is_empty(), "{}: no check date", p.id);
            // Every figure points at where it was read, except the generic form caps.
            let generic_only = !p.sizes.is_empty() && p.sizes.iter().all(|s| s.origin == "generic");
            assert!(
                !p.source_url.is_empty() || !p.sizes.is_empty() || generic_only,
                "{}: no source",
                p.id
            );
        }
        for p in loudness_presets() {
            assert!(p.target_lufs < 0.0, "{}", p.id);
            assert!(
                !p.source_url.is_empty() && !p.last_verified.is_empty(),
                "{}",
                p.id
            );
        }
    }

    #[test]
    fn the_named_services_resolve_to_a_byte_cap() {
        for name in [
            "whatsapp",
            "discord",
            "discord_nitro_basic",
            "discord_nitro",
            "gmail",
            "slack",
            "telegram",
            "x",
        ] {
            let r = resolve_size(name).unwrap_or_else(|| panic!("{name} does not resolve"));
            assert!(r.bytes >= 1_000_000, "{name}: {}", r.bytes);
        }
        assert!(resolve_size("nope").is_none());
        assert_eq!(resolve_size("gmail").unwrap().bytes, 25_000_000);
    }

    #[test]
    fn floors_exist_for_every_codec_the_ladder_can_pick() {
        for (c, hw) in [
            ("h264", true),
            ("hevc", true),
            ("av1", false),
            ("vp9", false),
        ] {
            let f = floors(c, hw).unwrap_or_else(|| panic!("{c} {hw}"));
            assert!(f.floor_bpp > 0.0 && f.knee_bpp > f.floor_bpp, "{c}: {f:?}");
        }
    }

    #[test]
    fn loudness_presets_include_the_broadcast_standards() {
        assert_eq!(loudness("ebu-r128").unwrap().target_lufs, -23.0);
        assert_eq!(loudness("ebu-r128").unwrap().true_peak_dbtp, -1.0);
        assert!(
            loudness("podcast-general").unwrap().target_lufs_mono
                < loudness("podcast-general").unwrap().target_lufs
        );
    }
}
