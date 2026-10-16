//! The built-in PDF modules, on the `panoply-pdf` engine: merge, split, organize, images to PDF,
//! PDF to images and compress.
//!
//! Each module reads only the files the user chose (through the capability guard), writes its
//! results through [`crate::Outputs`] (temporary name first, never overwriting, nothing left
//! behind on cancel or failure), and reports what it did as plain lines in the job notes.

mod compress;
mod images;
mod merge;
mod organize;
mod split;
mod to_images;
mod zip;

use std::path::{Path, PathBuf};

use panoply_manifest::Manifest;
use panoply_pdf::{Opened, PdfError};
use serde_json::Value;

use crate::{Module, ModuleError, RunContext};

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `pdf.merge`
    Merge,
    /// `pdf.split`
    Split,
    /// `pdf.organize`
    Organize,
    /// `pdf.images_to_pdf`
    ImagesToPdf,
    /// `pdf.to_images`
    ToImages,
    /// `pdf.compress`
    Compress,
}

const MANIFESTS: [(Op, &str); 6] = [
    (
        Op::Merge,
        include_str!("../../../../../modules/builtin/pdf-merge/module.toml"),
    ),
    (
        Op::Split,
        include_str!("../../../../../modules/builtin/pdf-split/module.toml"),
    ),
    (
        Op::Organize,
        include_str!("../../../../../modules/builtin/pdf-organize/module.toml"),
    ),
    (
        Op::ImagesToPdf,
        include_str!("../../../../../modules/builtin/pdf-images-to-pdf/module.toml"),
    ),
    (
        Op::ToImages,
        include_str!("../../../../../modules/builtin/pdf-to-images/module.toml"),
    ),
    (
        Op::Compress,
        include_str!("../../../../../modules/builtin/pdf-compress/module.toml"),
    ),
];

/// One PDF module: its manifest and the operation it runs.
pub struct PdfModule {
    op: Op,
    manifest: Manifest,
}

/// The six PDF modules.
pub fn modules() -> Vec<PdfModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| PdfModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in pdf manifest parses"),
        })
        .collect()
}

impl Module for PdfModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        if inputs.is_empty() {
            return Err(ModuleError::Failed(
                "Choose at least one file first.".into(),
            ));
        }
        let opts = Opts(crate::options::resolve(&self.manifest, options)?);
        match self.op {
            Op::Merge => merge::run(ctx, inputs, &opts),
            Op::Split => split::run(ctx, inputs, &opts),
            Op::Organize => organize::run(ctx, inputs, &opts),
            Op::ImagesToPdf => images::run(ctx, inputs, &opts),
            Op::ToImages => to_images::run(ctx, inputs, &opts),
            Op::Compress => compress::run(ctx, inputs, &opts),
        }
    }
}

/// The resolved options of a job (defaults filled in, types checked by the host).
pub(crate) struct Opts(Value);

impl Opts {
    pub fn str(&self, key: &str) -> &str {
        self.0.get(key).and_then(Value::as_str).unwrap_or("")
    }
    pub fn bool(&self, key: &str) -> bool {
        self.0.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
    pub fn num(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(Value::as_f64)
    }
}

/// A PDF error as the host's error, keeping the engine's plain sentence.
pub(crate) fn map_err(e: PdfError) -> ModuleError {
    match e {
        PdfError::Cancelled => ModuleError::Cancelled,
        PdfError::Io { path, source } if !path.as_os_str().is_empty() => {
            ModuleError::Io { path, source }
        }
        PdfError::Io { source, .. } => ModuleError::Failed(format!(
            "The result could not be written: {}. Free some space or choose another folder, then try again.",
            panoply_core::naming::plain_io(&source)
        )),
        other => ModuleError::Failed(other.to_string()),
    }
}

/// Like [`map_err`], putting the file name first when a job has several files.
pub(crate) fn map_err_for(path: &Path, many: bool, e: PdfError) -> ModuleError {
    match map_err(e) {
        ModuleError::Failed(m) if many => ModuleError::Failed(format!("{}: {m}", file_name(path))),
        other => other,
    }
}

pub(crate) fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(crate) fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A PDF opened through the guard. Encrypted files are refused with the engine's sentence.
pub(crate) struct Input {
    pub path: PathBuf,
    pub opened: Opened,
}

pub(crate) fn open_pdf(ctx: &RunContext, path: &Path, many: bool) -> Result<Input, ModuleError> {
    let file = ctx.guard.open_read(path)?;
    let opened = panoply_pdf::open(&file).map_err(|e| map_err_for(path, many, e))?;
    opened
        .doc
        .require_unencrypted()
        .map_err(|e| map_err_for(path, many, e))?;
    Ok(Input {
        path: path.to_path_buf(),
        opened,
    })
}

/// Notes every output carries: a repaired input, and signatures that do not survive.
pub(crate) fn note_input(ctx: &RunContext, i: &Input, many: bool) {
    let name = if many {
        format!("{}: ", file_name(&i.path))
    } else {
        String::new()
    };
    if i.opened.repaired {
        ctx.note(format!(
            "{name}This PDF was damaged and was repaired in memory. The original is untouched."
        ));
    }
    let sig = i.opened.doc.signatures();
    if sig.count > 0 {
        ctx.note(format!(
            "{name}This PDF is digitally signed. The new file is not signed, because changing a signed file breaks its signature. The original is untouched."
        ));
    }
}

/// "12.3 MB", "880 KB", "412 bytes". Decimal units, as file managers show them.
pub(crate) fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", b / 1e9)
    } else if bytes >= 10_000_000 {
        format!("{:.0} MB", b / 1e6)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else if bytes >= 1000 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} bytes")
    }
}

/// One-based page list for file names: `1-2, 5, 7-9`.
pub(crate) fn compact_pages(pages: &[usize]) -> String {
    let mut v = pages.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
            j += 1;
        }
        parts.push(if j == i {
            format!("{}", v[i] + 1)
        } else {
            format!("{}-{}", v[i] + 1, v[j] + 1)
        });
        i = j + 1;
    }
    parts.join(", ")
}

pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Compare file names the way people read them: `file2` before `file10`, case ignored.
pub(crate) fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n = String::new();
                while let Some(&c) = x.peek().filter(|c| c.is_ascii_digit()) {
                    n.push(c);
                    x.next();
                }
                let mut m = String::new();
                while let Some(&c) = y.peek().filter(|c| c.is_ascii_digit()) {
                    m.push(c);
                    y.next();
                }
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let ord = n.len().cmp(&m.len()).then_with(|| n.cmp(m));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Inputs in the order the job asks for: by natural name, or as given.
pub(crate) fn ordered(inputs: &[PathBuf], order: &str) -> Vec<PathBuf> {
    let mut v = inputs.to_vec();
    if order == "natural" {
        v.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
    }
    v
}

#[cfg(test)]
pub(crate) mod testpdf {
    //! Small PDFs for tests, with a classic cross-reference table.

    /// A PDF of `n` pages. Page `i` draws the text "Page i" and has size 200 x 200.
    pub fn pdf(n: usize) -> Vec<u8> {
        let mut objs: Vec<String> = Vec::new();
        objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
        objs.push(format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            kids.join(" ")
        ));
        objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        for i in 0..n {
            objs.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                5 + i * 2
            ));
            let content = format!("BT /F1 24 Tf 20 100 Td (Page {}) Tj ET", i + 1);
            objs.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// A fresh scratch folder.
    pub fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-pdfmod-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Run a module by id on files, returning the written files and notes.
    pub fn run(
        id: &str,
        inputs: &[std::path::PathBuf],
        options: serde_json::Value,
    ) -> Result<(Vec<std::path::PathBuf>, Vec<String>), crate::ModuleError> {
        let reg = crate::Registry::with_builtins();
        let spec = panoply_core::JobSpec {
            module_id: id.into(),
            inputs: inputs.to_vec(),
            options,
        };
        let o = reg.run_job(&spec, None, panoply_core::CancelToken::new(), |_| {})?;
        Ok((o.outputs, o.notes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_puts_2_before_10() {
        let mut v = vec!["file10.pdf", "File2.pdf", "file1.pdf", "file02b.pdf"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec!["file1.pdf", "File2.pdf", "file02b.pdf", "file10.pdf"]
        );
    }

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(size_text(880_000), "880 KB");
        assert_eq!(size_text(4_200_000), "4.2 MB");
        assert_eq!(size_text(412), "412 bytes");
    }

    #[test]
    fn all_six_manifests_are_valid_and_unique() {
        let ms = modules();
        assert_eq!(ms.len(), 6);
        for m in &ms {
            m.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", m.manifest.module.id));
            assert!(m.manifest.module.id.starts_with("pdf."));
        }
    }
}
