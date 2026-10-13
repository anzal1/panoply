//! The test corpus manifests (`tests/corpus/*.toml`) and slices of them.
//!
//! The manifests are the source of truth. `fetch.py` downloads the real files and `generate.py`
//! builds the synthetic ones into `tests/corpus/cache/<kind>/`. This module only reads them.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{BenchError, Result};

/// The five media kinds, one manifest each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// `pdf.toml`
    Pdf,
    /// `image.toml`
    Image,
    /// `video.toml`
    Video,
    /// `audio.toml`
    Audio,
    /// `office.toml`
    Office,
}

impl Kind {
    /// Every kind, in manifest order.
    pub const ALL: [Kind; 5] = [
        Kind::Pdf,
        Kind::Image,
        Kind::Video,
        Kind::Audio,
        Kind::Office,
    ];

    /// Lowercase name, also the manifest and cache directory name.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Pdf => "pdf",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Office => "office",
        }
    }

    /// Parse `pdf`, `image` and so on.
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str().eq_ignore_ascii_case(s.trim()))
    }

    /// True for the kinds whose memory must stay flat however long the file is (video, audio).
    pub fn is_streamed(self) -> bool {
        matches!(self, Kind::Video | Kind::Audio)
    }
}

/// Size class of a generated file. Real files are always `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Small, always built.
    Default,
    /// Hundreds of megabytes or an hour of media.
    Large,
    /// The 2 GB file.
    Huge,
}

impl Tier {
    /// Parse `default`, `large`, `huge`.
    pub fn parse(s: &str) -> Option<Tier> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Some(Tier::Default),
            "large" => Some(Tier::Large),
            "huge" => Some(Tier::Huge),
            _ => None,
        }
    }
}

/// One `[[file]]` table.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// Stable id, for example `pdf-qpdf-digitally-signed`.
    pub id: String,
    /// Path under `cache/<kind>/`.
    pub file: String,
    /// Where it came from, or `generated`.
    pub source: String,
    /// Download URL, absent for generated files.
    #[serde(default)]
    pub url: Option<String>,
    /// SPDX id or a `LicenseRef`.
    pub license: String,
    /// Pinned sha256, empty for generated files that are not byte-stable.
    #[serde(default)]
    pub sha256: String,
    /// Size in bytes when pinned, else 0.
    #[serde(default)]
    pub size: u64,
    /// Edge cases covered.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Why the file is in the corpus.
    pub why: String,
    /// Open password for encrypted samples.
    #[serde(default)]
    pub password: Option<String>,
    /// `default`, `large` or `huge`.
    #[serde(default)]
    pub tier: Option<String>,
    /// Generator function name for synthetic files.
    #[serde(default)]
    pub generator: Option<String>,
    /// Which manifest the entry came from. Filled in by [`Corpus::load`].
    #[serde(skip)]
    pub kind: Option<Kind>,
}

impl Entry {
    /// The entry's kind. Always set for entries that came from [`Corpus::load`].
    pub fn kind(&self) -> Kind {
        self.kind.unwrap_or(Kind::Pdf)
    }

    /// Size class, `Default` when the manifest says nothing.
    pub fn tier(&self) -> Tier {
        self.tier
            .as_deref()
            .and_then(Tier::parse)
            .unwrap_or(Tier::Default)
    }

    /// True for files made by `generate.py`.
    pub fn is_generated(&self) -> bool {
        self.source == "generated"
    }
}

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(default)]
    file: Vec<Entry>,
}

/// A loaded corpus: the manifest entries and where the cache lives.
#[derive(Debug, Clone)]
pub struct Corpus {
    root: PathBuf,
    entries: Vec<Entry>,
}

/// Which files to run. Empty lists mean no restriction on that axis.
#[derive(Debug, Clone, Default)]
pub struct Slice {
    /// Only these kinds.
    pub kinds: Vec<Kind>,
    /// Entries carrying any of these tags.
    pub tags: Vec<String>,
    /// Only these ids.
    pub ids: Vec<String>,
    /// Largest tier to include. `None` means `Default` only.
    pub max_tier: Option<Tier>,
    /// Skip files smaller than this many bytes (on disk).
    pub min_bytes: u64,
    /// Skip files larger than this many bytes (on disk).
    pub max_bytes: Option<u64>,
}

impl Corpus {
    /// `tests/corpus` of the source tree this binary was built from, overridable with
    /// `PANOPLY_CORPUS`.
    pub fn default_root() -> PathBuf {
        if let Some(p) = std::env::var_os("PANOPLY_CORPUS") {
            return PathBuf::from(p);
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
    }

    /// Read every manifest under `root`. A missing manifest is an error, so a typo in `--corpus`
    /// is not mistaken for an empty corpus.
    pub fn load(root: &Path) -> Result<Corpus> {
        let mut entries = Vec::new();
        for kind in Kind::ALL {
            let path = root.join(format!("{}.toml", kind.as_str()));
            let text = std::fs::read_to_string(&path)
                .map_err(|e| BenchError::io(format!("reading {}", path.display()), e))?;
            let parsed: ManifestFile = toml::from_str(&text).map_err(|e| BenchError::Manifest {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
            for mut e in parsed.file {
                e.kind = Some(kind);
                entries.push(e);
            }
        }
        Ok(Corpus {
            root: root.to_path_buf(),
            entries,
        })
    }

    /// Every entry.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Where an entry's file lives (it may not exist yet).
    pub fn path_of(&self, e: &Entry) -> PathBuf {
        self.root
            .join("cache")
            .join(e.kind().as_str())
            .join(&e.file)
    }

    /// The entries that match the slice and exist on disk, with their on-disk sizes.
    /// The second value is how many matching entries were not on disk.
    pub fn select(&self, slice: &Slice) -> (Vec<(&Entry, PathBuf, u64)>, usize) {
        let max_tier = slice.max_tier.unwrap_or(Tier::Default);
        let mut found = Vec::new();
        let mut missing = 0;
        for e in &self.entries {
            if !slice.kinds.is_empty() && !slice.kinds.contains(&e.kind()) {
                continue;
            }
            if !slice.ids.is_empty() && !slice.ids.iter().any(|i| i == &e.id) {
                continue;
            }
            if !slice.tags.is_empty() && !e.tags.iter().any(|t| slice.tags.contains(t)) {
                continue;
            }
            if e.tier() > max_tier {
                continue;
            }
            let path = self.path_of(e);
            match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => {
                    let size = m.len();
                    if size < slice.min_bytes || slice.max_bytes.is_some_and(|mx| size > mx) {
                        continue;
                    }
                    found.push((e, path, size));
                }
                _ => missing += 1,
            }
        }
        (found, missing)
    }

    /// Entry counts per kind: (all entries, generated entries).
    pub fn counts(&self) -> Vec<(Kind, usize, usize)> {
        Kind::ALL
            .into_iter()
            .map(|k| {
                let of_kind = self.entries.iter().filter(|e| e.kind() == k);
                let total = of_kind.clone().count();
                let generated = of_kind.filter(|e| e.is_generated()).count();
                (k, total, generated)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_corpus(dir: &Path) {
        for kind in Kind::ALL {
            let body = if kind == Kind::Pdf {
                r#"
[[file]]
id = "pdf-a"
file = "a.pdf"
source = "somewhere"
license = "MIT"
tags = ["encrypted"]
why = "w"

[[file]]
id = "pdf-big"
file = "generated/big.pdf"
source = "generated"
license = "MIT"
tags = ["huge"]
why = "w"
tier = "huge"
generator = "g"

[[file]]
id = "pdf-missing"
file = "missing.pdf"
source = "somewhere"
license = "MIT"
why = "w"
"#
            } else {
                ""
            };
            std::fs::write(dir.join(format!("{}.toml", kind.as_str())), body).unwrap();
        }
        let cache = dir.join("cache/pdf/generated");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(dir.join("cache/pdf/a.pdf"), vec![0u8; 100]).unwrap();
        std::fs::write(cache.join("big.pdf"), vec![0u8; 5000]).unwrap();
    }

    #[test]
    fn loads_and_selects() {
        let dir = std::env::temp_dir().join(format!("pb-corpus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_corpus(&dir);
        let c = Corpus::load(&dir).unwrap();
        assert_eq!(c.entries().len(), 3);
        assert_eq!(c.counts()[0], (Kind::Pdf, 3, 1));

        // Default tier skips the huge entry, and counts the file that is not on disk.
        let (found, missing) = c.select(&Slice::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.id, "pdf-a");
        assert_eq!(found[0].2, 100);
        assert_eq!(missing, 1);

        let all = Slice {
            max_tier: Some(Tier::Huge),
            ..Slice::default()
        };
        assert_eq!(c.select(&all).0.len(), 2);

        let tagged = Slice {
            max_tier: Some(Tier::Huge),
            tags: vec!["huge".into()],
            ..Slice::default()
        };
        let (found, _) = c.select(&tagged);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.id, "pdf-big");

        let big_only = Slice {
            max_tier: Some(Tier::Huge),
            min_bytes: 1000,
            ..Slice::default()
        };
        assert_eq!(c.select(&big_only).0.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_manifest_is_an_error() {
        let dir = std::env::temp_dir().join(format!("pb-nocorpus-{}", std::process::id()));
        assert!(Corpus::load(&dir).is_err());
    }

    #[test]
    fn the_real_manifests_parse() {
        // The manifests in the source tree must always load, whether or not files are cached.
        let c = Corpus::load(&Corpus::default_root()).unwrap();
        for (kind, total, _) in c.counts() {
            assert!(total >= 10, "{} has only {total} entries", kind.as_str());
        }
        let mut ids: Vec<_> = c.entries().iter().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate ids");
    }

    #[test]
    fn kind_and_tier_parse() {
        assert_eq!(Kind::parse(" PDF "), Some(Kind::Pdf));
        assert_eq!(Kind::parse("midi"), None);
        assert!(Kind::Video.is_streamed() && !Kind::Pdf.is_streamed());
        assert_eq!(Tier::parse("Large"), Some(Tier::Large));
        assert!(Tier::Huge > Tier::Large && Tier::Large > Tier::Default);
    }
}
