//! What a dropped file is, and which tools take it.

use std::path::{Path, PathBuf};

use panoply_core::{FamilyHint, Sniffed, sniff};
use panoply_manifest::{Manifest, Pattern};

use crate::catalog::Catalog;

/// Most files taken from one dropped folder.
pub const MAX_FOLDER_FILES: usize = 500;

/// A dropped or chosen file, sniffed from its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct FileProfile {
    /// Where it is.
    pub path: PathBuf,
    /// File name.
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
    /// What the bytes say it is.
    pub sniffed: Sniffed,
}

impl FileProfile {
    /// Read the size and the first 8 KB of the file. Cheap, but still a file read, so callers on
    /// the UI thread hand it to a worker.
    pub fn read(path: &Path) -> FileProfile {
        FileProfile {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            sniffed: sniff(path),
        }
    }

    /// "HEIC photo", "PDF document", "MP3 audio file".
    pub fn kind_label(&self) -> String {
        // The bytes decide the type, the name only picks between spellings of the same one.
        let named = self
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let same_type = |a: &str, b: &str| {
            a == b
                || [["heic", "heif"], ["jpg", "jpeg"], ["tif", "tiff"]]
                    .iter()
                    .any(|g| g.contains(&a) && g.contains(&b))
        };
        let ext = if same_type(&named, &self.sniffed.ext) {
            named
        } else {
            self.sniffed.ext.clone()
        }
        .to_ascii_uppercase();
        let noun = match self.sniffed.family_hint {
            FamilyHint::Image => match self.sniffed.ext.as_str() {
                "jpg" | "jpeg" | "heic" | "heif" | "dng" | "raw" => "photo",
                _ => "image",
            },
            FamilyHint::Document => "document",
            FamilyHint::Audio => "audio file",
            FamilyHint::Video => "video",
            FamilyHint::Archive => "archive",
            FamilyHint::Font => "font",
            FamilyHint::Text => "text file",
            FamilyHint::Unknown => "file",
        };
        if ext.is_empty() {
            noun.to_string()
        } else {
            format!("{ext} {noun}")
        }
    }
}

/// Folders become the files directly inside them (not recursive), capped at
/// [`MAX_FOLDER_FILES`]. Files stay as they are. Order is kept, names inside a folder are sorted.
pub fn expand_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut inner: Vec<PathBuf> = std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|q| q.is_file())
                .filter(|q| {
                    !q.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                })
                .collect();
            inner.sort();
            out.extend(inner.into_iter().take(MAX_FOLDER_FILES));
        } else {
            out.push(p.clone());
        }
    }
    out.truncate(MAX_FOLDER_FILES);
    out
}

/// A one-line label for a set of files: the name for one, "3 files" for several.
pub fn files_label(files: &[FileProfile]) -> String {
    match files {
        [] => String::new(),
        [one] => one.name.clone(),
        many => format!("{} files", many.len()),
    }
}

/// The noun phrase used in the footer: "HEIC photo", "3 files".
pub fn files_noun(files: &[FileProfile]) -> String {
    match files {
        [] => String::new(),
        [one] => one.kind_label(),
        many => {
            let first = &many[0].sniffed.ext;
            if many.iter().all(|f| f.sniffed.ext == *first) && !first.is_empty() {
                format!("{} files", first.to_ascii_uppercase())
            } else {
                "mixed files".to_string()
            }
        }
    }
}

/// A tool that takes these files.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// Index into the catalog.
    pub tool: usize,
    /// How specifically the manifest names this kind of file: 3 exact, 2 family, 1 anything.
    pub specificity: u8,
    /// Rank score, higher is better.
    pub score: i32,
}

/// How specifically `accepts` names this file, or `None` when it does not take it.
fn specificity(manifest: &Manifest, f: &FileProfile) -> Option<u8> {
    let mime = f.sniffed.mime.as_str();
    let ext = f.sniffed.ext.as_str();
    let mut best = None;
    for pat in &manifest.accepts {
        let one = std::slice::from_ref(pat);
        let m = Manifest {
            accepts: one.to_vec(),
            ..manifest.clone()
        };
        if m.accepts_file(mime, ext) {
            let s = if pat.starts_with("*.") || !pat.ends_with("/*") {
                3
            } else if pat == "*/*" {
                1
            } else {
                2
            };
            best = Some(best.map_or(s, |b: u8| b.max(s)));
        }
    }
    best
}

fn pattern_rank(p: Pattern) -> i32 {
    match p {
        Pattern::InstantTransform | Pattern::TunedTransform => 9,
        Pattern::GuidedCapture => 7,
        Pattern::VisualEditor => 6,
        Pattern::Inspect => 5,
        Pattern::Find => 4,
        Pattern::Generator => 3,
        Pattern::InstantAnswer => 2,
        Pattern::AlwaysOn => 1,
    }
}

/// Tools that take every one of these files, best first. `recent` is module ids from the
/// history, newest first, which lifts what the person uses.
pub fn tools_for_files(catalog: &Catalog, files: &[FileProfile], recent: &[String]) -> Vec<Match> {
    if files.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, t) in catalog.tools().iter().enumerate() {
        if !t.on_bar() || !t.takes_files() {
            continue;
        }
        let spec = files
            .iter()
            .map(|f| specificity(&t.manifest, f))
            .collect::<Option<Vec<u8>>>()
            .and_then(|v| v.into_iter().min());
        let Some(specificity) = spec else { continue };
        let recency = recent
            .iter()
            .position(|id| *id == t.id)
            .map_or(0, |p| 20 - p as i32 * 3)
            .max(0);
        out.push(Match {
            tool: i,
            specificity,
            score: i32::from(specificity) * 100 + pattern_rank(t.pattern) * 4 + recency,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| catalog.tool(a.tool).name.cmp(&catalog.tool(b.tool).name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(name: &str, head: &[u8]) -> FileProfile {
        let ext = Path::new(name)
            .extension()
            .map(|e| e.to_string_lossy().into_owned());
        FileProfile {
            path: PathBuf::from(name),
            name: name.to_string(),
            bytes: 3_100_000,
            sniffed: panoply_core::sniff_bytes(head, ext.as_deref()),
        }
    }

    const PNG_HEAD: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn an_image_ranks_image_tools_and_never_pdf_tools() {
        let c = Catalog::with_dirs(&[]);
        let f = fake("shot.png", PNG_HEAD);
        let m = tools_for_files(&c, &[f], &[]);
        let ids: Vec<&str> = m.iter().map(|m| c.tool(m.tool).id.as_str()).collect();
        assert!(ids.contains(&"image.compress"), "{ids:?}");
        assert!(ids.contains(&"pdf.images_to_pdf"), "{ids:?}");
        assert!(!ids.contains(&"pdf.compress"), "{ids:?}");
        // The catch-all hash tool takes everything but ranks below the specific tools.
        let hash = ids.iter().position(|i| *i == "files.hash").unwrap();
        let compress = ids.iter().position(|i| *i == "image.compress").unwrap();
        assert!(compress < hash);
    }

    #[test]
    fn every_file_must_be_taken() {
        let c = Catalog::with_dirs(&[]);
        let png = fake("a.png", PNG_HEAD);
        let pdf = fake("b.pdf", b"%PDF-1.7\n");
        let text = fake("c.txt", b"plain words\n");

        // A picture and a text file: only the tools that take any file take both (hash, and pack
        // into an archive).
        let m = tools_for_files(&c, &[png.clone(), text], &[]);
        let mut ids: Vec<&str> = m.iter().map(|m| c.tool(m.tool).id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["archive.create", "files.hash"]);

        // A picture and a PDF: the any-file tools, and OCR, which reads both kinds. No tool that
        // takes only pictures or only PDFs may show up for the pair.
        let m = tools_for_files(&c, &[png, pdf], &[]);
        let mut ids: Vec<&str> = m.iter().map(|m| c.tool(m.tool).id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["archive.create", "files.hash", "pdf.ocr"]);
    }

    #[test]
    fn recent_use_lifts_a_tool_within_its_tier() {
        let c = Catalog::with_dirs(&[]);
        let f = fake("shot.png", PNG_HEAD);
        let plain = tools_for_files(&c, std::slice::from_ref(&f), &[]);
        let lifted = tools_for_files(&c, &[f], &["image.strip_metadata".to_string()]);
        let pos = |m: &[Match], id: &str| m.iter().position(|x| c.tool(x.tool).id == id).unwrap();
        assert!(pos(&lifted, "image.strip_metadata") <= pos(&plain, "image.strip_metadata"));
    }

    #[test]
    fn kind_labels_read_like_the_wireframe() {
        let heic_head = [
            0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c', 0, 0, 0, 0, b'm', b'i',
            b'f', b'1', b'h', b'e', b'i', b'c',
        ];
        assert_eq!(fake("IMG_2041.heic", &heic_head).kind_label(), "HEIC photo");
        assert_eq!(fake("r.pdf", b"%PDF-1.7\n").kind_label(), "PDF document");
        assert_eq!(files_label(&[]), "");
    }

    #[test]
    fn folders_expand_to_their_files() {
        let d = std::env::temp_dir().join(format!("panoply-prof-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("b.txt"), "x").unwrap();
        std::fs::write(d.join("a.txt"), "x").unwrap();
        std::fs::write(d.join(".hidden"), "x").unwrap();
        let out = expand_paths(std::slice::from_ref(&d));
        assert_eq!(out, vec![d.join("a.txt"), d.join("b.txt")]);
        let _ = std::fs::remove_dir_all(d);
    }
}
