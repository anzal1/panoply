//! Shared helpers for the corpus tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// One entry of tests/corpus/pdf.toml.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub path: PathBuf,
    pub tags: Vec<String>,
    pub password: Option<String>,
    pub tier: String,
}

impl Entry {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

pub fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

/// Every corpus PDF that is present on disk.
pub fn corpus() -> Vec<Entry> {
    let root = corpus_root();
    let text = std::fs::read_to_string(root.join("pdf.toml")).expect("tests/corpus/pdf.toml");
    let v: toml::Value = toml::from_str(&text).expect("pdf.toml parses");
    let mut out = Vec::new();
    for f in v["file"].as_array().unwrap() {
        let path = root.join("cache/pdf").join(f["file"].as_str().unwrap());
        if !path.is_file() {
            continue;
        }
        out.push(Entry {
            id: f["id"].as_str().unwrap().to_string(),
            path,
            tags: f["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            password: f.get("password").and_then(|p| p.as_str()).map(String::from),
            tier: f
                .get("tier")
                .and_then(|t| t.as_str())
                .unwrap_or("default")
                .to_string(),
        });
    }
    out
}

/// A corpus file by id (with or without the `pdf-` prefix). Panics with the fetch command when
/// the cache is empty, so a missing corpus is never a silent pass.
pub fn get(id: &str) -> Entry {
    let id = id.strip_prefix("pdf-").unwrap_or(id);
    corpus()
        .into_iter()
        .find(|e| e.id.strip_prefix("pdf-").unwrap_or(&e.id) == id)
        .unwrap_or_else(|| {
            panic!(
                "corpus file {id} is not on disk: run `python3 -I tests/corpus/fetch.py --kind pdf`"
            )
        })
}

/// A scratch folder that is removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("panoply-pdf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
