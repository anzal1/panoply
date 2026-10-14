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
    /// What is known about the file: the corpus' own tags, plus `no-audio` for a video or sound
    /// file that has no audio stream. The expected refusals are judged on these.
    pub traits: Vec<String>,
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
    /// The traits of each input, in the order of `ids`.
    pub traits: Vec<Vec<String>>,
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
            let mut traits: Vec<String> =
                entry.tags.iter().map(|t| t.to_ascii_lowercase()).collect();
            if matches!(entry.kind().as_str(), "video" | "audio") && has_no_audio(&path) {
                traits.push("no-audio".into());
            }
            out.push(Candidate {
                id: entry.id.clone(),
                kind: entry.kind().as_str().to_string(),
                path,
                bytes: size,
                ext: p.sniffed.ext,
                mime: p.sniffed.mime,
                generated: entry.is_generated(),
                traits,
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
            traits: Vec::new(),
        });
    }
    out
}

/// True when `ffprobe` says the file has no audio stream. A machine without `ffprobe` answers
/// false, so the file is not marked and a refusal for missing sound is then not expected.
fn has_no_audio(path: &Path) -> bool {
    std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim().is_empty())
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

/// Files that exercise what a module does and the spread by size would miss: a tag that makes a
/// file the right input for the module. The face modules need a face, the HDR module needs an HDR
/// picture, OCR needs a page with no text layer. Each module gets up to two of them as an extra
/// group, so the matrix shows the module doing its job and not only declining a landscape.
pub fn wanted_tags(module: &str) -> &'static [&'static str] {
    match module {
        "id.passport_photo" | "id.print_sheet" => &["face"],
        "media.hdr_to_sdr" => &["hdr"],
        "media.streams" => &["multi-audio"],
        "pdf.ocr" => &["no-text-layer"],
        "image.to_text" => &["ocr-text"],
        _ => &[],
    }
}

fn case(module: &str, group: String, picks: &[Candidate], typed: Vec<(String, String)>) -> Case {
    Case {
        module: module.to_string(),
        group,
        ids: picks.iter().map(|c| c.id.clone()).collect(),
        files: picks.iter().map(|c| c.path.clone()).collect(),
        typed,
        bytes: picks.iter().map(|c| c.bytes).sum(),
        traits: picks.iter().map(|c| c.traits.clone()).collect(),
    }
}

fn tagged(c: &Candidate, tag: &str) -> bool {
    c.traits.iter().any(|t| t == tag)
}

/// The cases for the given modules over these candidates.
pub fn plan(
    mods: &[ModuleInfo],
    catalog: &Catalog,
    cands: &[Candidate],
    opts: Options,
) -> Vec<Case> {
    let mut cases = Vec::new();
    for m in mods {
        let Some(i) = catalog.index_of(&m.id) else {
            continue;
        };
        let tool = catalog.tool(i);
        if !tool.takes_files() {
            cases.push(case(&m.id, "none".into(), &[], typed(&m.id)));
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
                cases.push(case(
                    &m.id,
                    ext.clone(),
                    std::slice::from_ref(c),
                    typed(&m.id),
                ));
            }
            // The refusal itself: the same first file, nothing typed.
            if !needs_input(&m.id).is_empty()
                && !cases
                    .iter()
                    .any(|c: &Case| c.module == m.id && c.group == "defaults only")
                && let Some(c) = picks.first()
            {
                cases.push(case(
                    &m.id,
                    "defaults only".into(),
                    std::slice::from_ref(c),
                    vec![],
                ));
            }
            if biggest.as_ref().is_none_or(|(_, n, _)| group.len() > *n) {
                biggest = Some((ext.clone(), group.len(), spread(group, opts.batch)));
            }
        }
        // Files that are the right input for this module, whatever their size.
        for tag in wanted_tags(&m.id) {
            let group: Vec<&Candidate> = cands
                .iter()
                .filter(|c| tagged(c, tag) && tool.manifest.accepts_file(&c.mime, &c.ext))
                .collect();
            for c in spread(&group, 2) {
                if !cases
                    .iter()
                    .any(|k| k.module == m.id && k.ids == [c.id.clone()])
                {
                    cases.push(case(
                        &m.id,
                        format!("{} ({tag})", c.ext),
                        std::slice::from_ref(&c),
                        typed(&m.id),
                    ));
                }
            }
        }
        // Animated pictures: the form asks what to do by default, so the plan also answers it
        // the way a person would ("first frame") to check the answer works.
        let takes_first_frame = tool
            .manifest
            .options
            .get("animation")
            .is_some_and(|o| o.values.iter().any(|v| v == "first"));
        if takes_first_frame {
            let group: Vec<&Candidate> = cands
                .iter()
                .filter(|c| tagged(c, "animated") && tool.manifest.accepts_file(&c.mime, &c.ext))
                .collect();
            for c in spread(&group, 2) {
                let mut t = typed(&m.id);
                t.push(("animation".into(), "first".into()));
                cases.push(case(
                    &m.id,
                    format!("{} (first frame)", c.ext),
                    std::slice::from_ref(&c),
                    t,
                ));
            }
        }
        if let Some((ext, _, picks)) = biggest
            && picks.len() >= 2
        {
            cases.push(case(
                &m.id,
                format!("batch of {} {ext}", picks.len()),
                &picks,
                typed(&m.id),
            ));
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
            traits: Vec::new(),
        }
    }

    #[test]
    fn spread_takes_the_smallest_and_the_largest() {
        let cs: Vec<Candidate> = (1..=7)
            .map(|i| cand(&format!("c{i}"), i * 100, false))
            .collect();
        let refs: Vec<&Candidate> = cs.iter().collect();
        let got: Vec<String> = spread(&refs, 3).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["c1", "c4", "c7"]);
        let got: Vec<String> = spread(&refs, 2).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["c1", "c7"]);
    }

    #[test]
    fn synthetic_files_fill_in_only_when_nothing_real_exists() {
        let cs = [cand("real", 50, false), cand("fake", 10, true)];
        let refs: Vec<&Candidate> = cs.iter().collect();
        let got: Vec<String> = spread(&refs, 2).into_iter().map(|c| c.id).collect();
        assert_eq!(got, ["real"]);
        let only_fake = [cand("fake", 10, true)];
        let refs: Vec<&Candidate> = only_fake.iter().collect();
        assert_eq!(spread(&refs, 2).len(), 1);
    }

    #[test]
    fn a_small_group_is_taken_whole() {
        let cs = [cand("a", 1, false), cand("b", 2, false)];
        let refs: Vec<&Candidate> = cs.iter().collect();
        assert_eq!(spread(&refs, 5).len(), 2);
    }
}
