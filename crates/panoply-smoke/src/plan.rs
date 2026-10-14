//! Which files each module runs on.
//!
//! For every module that takes files, and every file type it accepts (the type the bytes sniff
//! as: `jpg`, `png`, `heic`, `pdf`, `mp4`, `flac`), the plan picks a few real corpus files. It adds
//! one batch job per module (several files of the most common accepted type in one job, as a
//! multi-file drop does). Modules that take no files run once with none.
//!
//! Files come from the corpus manifests (`tests/corpus/*.toml`), in the default tier only, and
//! only when they are on disk. Edge cases that are meant to be refused (encrypted, truncated,
//! broken, not a PDF) are left out: this matrix asks "does a normal file work", and the host's own
//! corpus tests ask "does a bad file fail in plain words". Synthetic files made by `generate.py`
//! are used only for a type that has no real file. Text files for the text modules come from the
//! repository itself, since a text file is the same real file anywhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use panoply_bench::corpus::{Corpus, Slice};
use panoply_desktop::catalog::{Catalog, Source};
use panoply_desktop::profile::FileProfile;

/// Tags that mark a file as deliberately damaged or locked. These are the corpus' own words.
const REFUSED_TAGS: &[&str] = &[
    "encrypted",
    "password",
    "broken",
    "corrupt",
    "corrupted",
    "truncated",
    "damaged",
    "malformed",
    "not-a-pdf",
    "bomb",
    "no-xref",
    "bad-xref",
    "damaged-stream",
    "bitflip",
    "unsupported",
];

/// Repository text files that stand in for "a text file" (relative to the repository root).
const TEXT_FILES: &[&str] = &[
    "README.md",
    "CONTRIBUTING.md",
    "docs/FLOWS.md",
    "tests/corpus/pdf.toml",
];

/// Biggest single input a case may use, bytes. A 400 MP PNG is a scale test, not a smoke test.
pub const MAX_CASE_BYTES: u64 = 40_000_000;

/// One file the plan may pick.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// Corpus id, or the repository path for a text file.
    pub id: String,
    /// Corpus kind (`pdf`, `image`, `video`, `audio`, `office`) or `text`.
    pub kind: String,
    /// Where it is.
    pub path: PathBuf,
    /// Size in bytes.
    pub bytes: u64,
    /// Extension the bytes sniff as.
    pub ext: String,
    /// MIME type the bytes sniff as.
    pub mime: String,
    /// True for files made by `generate.py`.
    pub generated: bool,
}

/// One job to run.
#[derive(Debug, Clone)]
pub struct Case {
    /// Module id.
    pub module: String,
    /// The accepted file type the case covers, or `batch`, or `none`.
    pub group: String,
    /// Corpus ids of the inputs.
    pub ids: Vec<String>,
    /// The inputs.
    pub files: Vec<PathBuf>,
    /// What a person has to type into the options form first, as `(option, text)`.
    pub typed: Vec<(String, String)>,
    /// Input size, bytes.
    pub bytes: u64,
}

/// Options a module cannot default, because the answer is the person's: which country's photo
/// rules, how big to print, what to erase, which part to keep. The manifest gives them no default
/// on purpose ("it will not guess"). The matrix types the smallest valid answer, as the panel's
/// text fields would, and also runs the module once with the defaults alone to check that the
/// refusal is in plain words with a next step.
pub fn needs_input(module: &str) -> &'static [(&'static str, &'static str)] {
    match module {
        "id.passport_photo" => &[("country", "IN")],
        "id.print_sheet" => &[("size", "35x45")],
        "image.erase" => &[("rect", "10%,10%,20%,20%")],
        "image.remove_watermark" => &[("own_content", "true")],
        "media.trim" => &[("keep", "0:00-0:01")],
        "pdf.organize" => &[("rotate", "90")],
        _ => &[],
    }
}

fn typed(module: &str) -> Vec<(String, String)> {
    needs_input(module)
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// The module and what it needs from the plan.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// Module id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// True for the example WebAssembly modules.
    pub community: bool,
    /// Flow pattern letter.
    pub pattern: String,
    /// What it accepts.
    pub accepts: Vec<String>,
}

/// How many files per type.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Files per accepted type, 2 to 5.
    pub per_type: usize,
    /// Files in the batch job.
    pub batch: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            per_type: 2,
            batch: 3,
        }
    }
}

/// The repository root this crate was built from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every module the app would list, built-in first.
pub fn modules(catalog: &Catalog) -> Vec<ModuleInfo> {
    catalog
        .tools()
        .iter()
        .map(|t| ModuleInfo {
            id: t.id.clone(),
            name: t.name.clone(),
            community: matches!(t.source, Source::Community(_)),
            pattern: t.pattern.letter().to_string(),
            accepts: t.manifest.accepts.clone(),
        })
        .collect()
}

/// True when the corpus has files on disk to plan from.
pub fn corpus_present(root: &Path) -> bool {
    root.join("cache").is_dir()
        && Corpus::load(root)
            .map(|c| !c.select(&Slice::default()).0.is_empty())
            .unwrap_or(false)
}

/// The files on disk that may be picked, sniffed.
pub fn candidates(root: &Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    if let Ok(corpus) = Corpus::load(root) {
        let (found, _) = corpus.select(&Slice::default());
        for (entry, path, size) in found {
            if size == 0 || size > MAX_CASE_BYTES {
                continue;
            }
            if entry
                .tags
                .iter()
                .any(|t| REFUSED_TAGS.contains(&t.to_ascii_lowercase().as_str()))
                || entry.password.is_some()
            {
                continue;
            }
            let p = FileProfile::read(&path);
            out.push(Candidate {
                id: entry.id.clone(),
                kind: entry.kind().as_str().to_string(),
                path,
                bytes: size,
                ext: p.sniffed.ext,
                mime: p.sniffed.mime,
                generated: entry.is_generated(),
            });
        }
    }
    let repo = root.join("../..");
    for rel in TEXT_FILES {
        let path = repo.join(rel);
        if !path.is_file() {
            continue;
        }
        let p = FileProfile::read(&path);
        out.push(Candidate {
            id: (*rel).to_string(),
            kind: "text".into(),
            path,
            bytes: p.bytes,
            ext: p.sniffed.ext,
            mime: p.sniffed.mime,
            generated: false,
        });
    }
    out
}

/// Pick `n` files from a group, spread across the size range: the smallest, the largest, then the
/// ones between. Real files first, synthetic ones only when there are not enough real ones.
fn spread(group: &[&Candidate], n: usize) -> Vec<Candidate> {
    let mut real: Vec<&Candidate> = group.iter().copied().filter(|c| !c.generated).collect();
    if real.is_empty() {
        real = group.to_vec();
    }
    real.sort_by_key(|c| (c.bytes, c.id.clone()));
    if real.len() <= n {
        return real.into_iter().cloned().collect();
    }
    let mut picks = Vec::new();
    for k in 0..n {
        // Evenly spaced positions from the first to the last.
        let at = if n == 1 {
            0
        } else {
            k * (real.len() - 1) / (n - 1)
        };
        let c = real[at];
        if !picks.iter().any(|p: &&Candidate| p.id == c.id) {
            picks.push(c);
        }
    }
    picks.into_iter().cloned().collect()
}

/// The cases for the given modules over these candidates.
pub fn plan(mods: &[ModuleInfo], catalog: &Catalog, cands: &[Candidate], opts: Options) -> Vec<Case> {
    let mut cases = Vec::new();
    for m in mods {
        let Some(i) = catalog.index_of(&m.id) else {
            continue;
        };
        let tool = catalog.tool(i);
        if !tool.takes_files() {
            cases.push(Case {
                module: m.id.clone(),
                group: "none".into(),
                ids: vec![],
                files: vec![],
                typed: typed(&m.id),
                bytes: 0,
            });
            continue;
        }
        // Group what the module accepts by the type the bytes sniff as.
        let mut groups: BTreeMap<String, Vec<&Candidate>> = BTreeMap::new();
        for c in cands {
            if tool.manifest.accepts_file(&c.mime, &c.ext) {
                groups.entry(c.ext.clone()).or_default().push(c);
            }
        }
        // The batch uses the type with the most files, which is the most common one.
        let mut biggest: Option<(String, usize, Vec<Candidate>)> = None;
        for (ext, group) in &groups {
            let picks = spread(group, opts.per_type);
            for c in &picks {
                cases.push(Case {
                    module: m.id.clone(),
                    group: ext.clone(),
                    ids: vec![c.id.clone()],
                    files: vec![c.path.clone()],
                    typed: typed(&m.id),
                    bytes: c.bytes,
                });
            }
            // The refusal itself: the same first file, nothing typed.
            if !needs_input(&m.id).is_empty()
                && !cases
                    .iter()
                    .any(|c: &Case| c.module == m.id && c.group == "defaults only")
                && let Some(c) = picks.first()
            {
                cases.push(Case {
                    module: m.id.clone(),
                    group: "defaults only".into(),
                    ids: vec![c.id.clone()],
                    files: vec![c.path.clone()],
                    typed: vec![],
                    bytes: c.bytes,
                });
            }
            if biggest.as_ref().is_none_or(|(_, n, _)| group.len() > *n) {
                biggest = Some((ext.clone(), group.len(), spread(group, opts.batch)));
            }
        }
        if let Some((ext, _, picks)) = biggest
            && picks.len() >= 2
        {
            cases.push(Case {
                module: m.id.clone(),
                group: format!("batch of {} {ext}", picks.len()),
                ids: picks.iter().map(|c| c.id.clone()).collect(),
                files: picks.iter().map(|c| c.path.clone()).collect(),
                typed: typed(&m.id),
                bytes: picks.iter().map(|c| c.bytes).sum(),
            });
        }
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, bytes: u64, generated: bool) -> Candidate {
        Candidate {
            id: id.into(),
            kind: "image".into(),
            path: PathBuf::from(id),
            bytes,
            ext: "png".into(),
            mime: "image/png".into(),
            generated,
        }
    }

    #[test]
    fn spread_takes_the_smallest_and_the_largest() {
        let cs: Vec<Candidate> = (1..=7).map(|i| cand(&format!("c{i}"), i * 100, false)).collect();
        let refs: Vec<&Candidate> = cs.iter().collect();
        let got: Vec<String> = spread(&refs, 3).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["c1", "c4", "c7"]);
        let got: Vec<String> = spread(&refs, 2).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["c1", "c7"]);
    }

    #[test]
    fn synthetic_files_fill_in_only_when_nothing_real_exists() {
        let cs = vec![cand("real", 50, false), cand("fake", 10, true)];
        let refs: Vec<&Candidate> = cs.iter().collect();
        let got: Vec<String> = spread(&refs, 2).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["real"]);
        let only_fake = vec![cand("fake", 10, true)];
        let refs: Vec<&Candidate> = only_fake.iter().collect();
        assert_eq!(spread(&refs, 2).len(), 1);
    }

    #[test]
    fn a_small_group_is_taken_whole() {
        let cs = vec![cand("a", 1, false), cand("b", 2, false)];
        let refs: Vec<&Candidate> = cs.iter().collect();
        assert_eq!(spread(&refs, 5).len(), 2);
    }
}
