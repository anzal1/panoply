//! `pdf.redact`, on the `panoply-ocr` redaction engine: find matches, show them (dry run), and
//! write a file with the matches truly removed, checked before it is handed over.

use std::collections::HashSet;
use std::io::{BufWriter, Read};
use std::path::PathBuf;

use panoply_manifest::Manifest;
use panoply_ocr::redact::{self, Options as RedactOptions, Preset, Where};
use panoply_ocr::{Fetch, Lang, Ocr, OcrError};
use serde_json::Value;

use crate::{Module, ModuleError, Outputs, RunContext};

const PDF_REDACT: &str = include_str!("../../../../modules/builtin/pdf-redact/module.toml");

/// The `pdf.redact` module.
pub struct RedactModule {
    manifest: Manifest,
}

/// The module.
pub fn all() -> Vec<RedactModule> {
    vec![RedactModule {
        manifest: Manifest::from_toml_str(PDF_REDACT).expect("built-in redact manifest parses"),
    }]
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn describe(m: &redact::Match) -> String {
    let kind = Preset::parse(&m.kind).map_or("Word".to_string(), |p| p.label().to_string());
    let place = match &m.at {
        Where::Page(p) => format!("page {}", p + 1),
        Where::Object { what, .. } => (*what).to_string(),
    };
    format!("#{} {kind}: {} ({place})", m.id, m.text)
}

impl Module for RedactModule {
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
        let o = crate::options::resolve(&self.manifest, options)?;
        let terms: Vec<String> = s(&o, "terms")
            .split(['\n', ','])
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let presets = Preset::parse_list(s(&o, "presets")).map_err(ModuleError::BadOption)?;
        let langs = Lang::parse_list(s(&o, "languages"))
            .map_err(|e| ModuleError::BadOption(e.to_string()))?;
        let skip: HashSet<usize> = s(&o, "skip")
            .split([',', ' '])
            .filter_map(|t| t.trim().trim_start_matches('#').parse().ok())
            .collect();
        let dry = o.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
        let opts = RedactOptions {
            terms,
            presets,
            dpi: o.get("dpi").and_then(Value::as_f64).unwrap_or(200.0) as f32,
            langs,
            ..RedactOptions::default()
        };
        let many = inputs.len() > 1;
        let ocr = Ocr::new();
        let mut out = Outputs::new(&ctx.guard);
        for (k, path) in inputs.iter().enumerate() {
            ctx.check_cancel()?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let prefix = if many {
                format!("{name}: ")
            } else {
                String::new()
            };
            let share = |f: f32| (k as f32 + f) / inputs.len() as f32;
            let mut head = [0u8; 1024];
            let n = ctx
                .guard
                .open_read(path)?
                .read(&mut head)
                .map_err(|source| ModuleError::Io {
                    path: path.clone(),
                    source,
                })?;
            if !head[..n].windows(5).any(|w| w == b"%PDF-") {
                return Err(ModuleError::Failed(format!("{prefix}This is not a PDF.")));
            }
            let file = ctx.guard.open_read(path)?;
            let fail = |e: OcrError| match e {
                OcrError::Cancelled => ModuleError::Cancelled,
                other if many => ModuleError::Failed(format!("{name}: {other}")),
                other => ModuleError::Failed(other.to_string()),
            };
            let mut notice = |t: &str| {
                ctx.progress(0.0, t.to_string());
                ctx.note(t.to_string());
            };
            let mut fetch = Fetch::new(&mut notice);
            fetch.allow_download = ctx.guard.require_network().is_ok();
            let mut progress = |f: f32, l: &str| ctx.progress(share(f * 0.5), l.to_string());
            let found = redact::find(&file, &opts, &ocr, &mut fetch, &ctx.cancel, &mut progress)
                .map_err(fail)?;
            if found.matches.is_empty() {
                ctx.note(format!("{prefix}Nothing matched, so no file was written."));
                continue;
            }
            for m in &found.matches {
                ctx.note(format!("{prefix}{}", describe(m)));
            }
            if dry {
                ctx.note(format!(
                    "{prefix}{} in {}. Nothing was written.",
                    plural(found.matches.len(), "match", "matches"),
                    plural(found.pages, "page", "pages")
                ));
                continue;
            }
            let mut all = found.matches;
            for m in &mut all {
                m.selected = !skip.contains(&m.id);
            }
            if !all.iter().any(|m| m.selected) {
                ctx.note(format!(
                    "{prefix}Every match was kept, so no file was written."
                ));
                continue;
            }
            let mut pending = out.file(path, "redacted", "pdf")?;
            let sink = pending
                .file()
                .try_clone()
                .map_err(|source| ModuleError::Io {
                    path: path.clone(),
                    source,
                })?;
            let mut progress = |f: f32, l: &str| ctx.progress(share(0.5 + f * 0.45), l.to_string());
            let report = redact::apply(
                &file,
                BufWriter::new(sink),
                &opts,
                &all,
                &ocr,
                &mut fetch,
                &ctx.cancel,
                &mut progress,
            )
            .map_err(fail)?;
            ctx.progress(share(0.97), "Checking the new file");
            let leaks = redact::verify(pending.file(), &report.needles).map_err(fail)?;
            if !leaks.is_empty() {
                drop(pending);
                let places: Vec<String> = leaks.into_iter().map(|l| l.place).collect();
                return Err(ModuleError::Failed(format!(
                    "{prefix}A redacted string was still found in the new file ({}), so it was not saved. The original is untouched.",
                    places.join(", ")
                )));
            }
            out.commit(pending)?;
            if report.signed {
                ctx.note(format!("{prefix}This PDF was digitally signed. The new file is not, because redacting breaks the signature."));
            }
            if report.repaired {
                ctx.note(format!("{prefix}This PDF was damaged and was repaired in memory. The original is untouched."));
            }
            let skipped = all.iter().filter(|m| !m.selected).count();
            ctx.note(format!(
                "{prefix}Redacted {} on {}. Dropped from the file: {} (annotations, form fields, attachments, info and metadata that held a match). Checked: the redacted text is not in the text, the streams or the bytes.{}",
                plural(report.applied, "match", "matches"),
                plural(report.redacted_pages.len(), "page", "pages"),
                plural(report.dropped.len(), "object", "objects"),
                if skipped > 0 { format!(" {skipped} kept as chosen.") } else { String::new() }
            ));
            ctx.note(format!(
                "{prefix}Redacted pages are redrawn as pictures at {} DPI with a hidden text layer, so links and form fields on them are flattened.",
                opts.dpi
            ));
        }
        Ok(out.finish())
    }
}
