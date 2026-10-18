//! `archive.create` and `archive.extract`, on the `panoply-archive` engine.
//!
//! Extract always unpacks into a new folder next to the archive (a taken name gets ` (2)`), so
//! nothing that exists is overwritten, and a failed or cancelled job removes every folder it made.
//! Create writes under a temporary name and moves the result into place when it is complete.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use panoply_archive::{ArchiveError, CreateOptions, Format, Limits, size_text};
use panoply_manifest::Manifest;
use serde_json::Value;

use crate::{Module, ModuleError, Outputs, RunContext};

/// Which operation a module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `archive.create`
    Create,
    /// `archive.extract`
    Extract,
}

const MANIFESTS: [(Op, &str); 2] = [
    (
        Op::Create,
        include_str!("../../../../modules/builtin/archive-create/module.toml"),
    ),
    (
        Op::Extract,
        include_str!("../../../../modules/builtin/archive-extract/module.toml"),
    ),
];

/// One archive module: its manifest and the operation it runs.
pub struct ArchiveModule {
    op: Op,
    manifest: Manifest,
}

/// The two archive modules.
pub fn all() -> Vec<ArchiveModule> {
    MANIFESTS
        .iter()
        .map(|(op, text)| ArchiveModule {
            op: *op,
            manifest: Manifest::from_toml_str(text).expect("built-in archive manifest parses"),
        })
        .collect()
}

impl Module for ArchiveModule {
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
        let opts = crate::options::resolve(&self.manifest, options)?;
        match self.op {
            Op::Extract => extract(ctx, inputs, &opts),
            Op::Create => create(ctx, inputs, &opts),
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// An engine error as the host's error, with the file name first when a job has several files.
fn map_err(e: ArchiveError, name: Option<&str>) -> ModuleError {
    let with = |m: String| match name {
        Some(n) => ModuleError::Failed(format!("{n}: {m}")),
        None => ModuleError::Failed(m),
    };
    match e {
        ArchiveError::Cancelled => ModuleError::Cancelled,
        ArchiveError::Io { path, source } if !path.as_os_str().is_empty() => {
            ModuleError::Io { path, source }
        }
        ArchiveError::Io { source, .. } => with(format!(
            "The files could not be read or written: {}. Free some space or choose another folder, \
             then try again.",
            panoply_core::naming::plain_io(&source)
        )),
        other => with(other.to_string()),
    }
}

fn extract(
    ctx: &RunContext,
    inputs: &[PathBuf],
    opts: &Value,
) -> Result<Vec<PathBuf>, ModuleError> {
    let defaults = Limits::default();
    let limits = Limits {
        max_ratio: opts
            .get("max_ratio")
            .and_then(Value::as_f64)
            .map_or(defaults.max_ratio, |v| v as u64),
        max_total: opts
            .get("max_size_gb")
            .and_then(Value::as_f64)
            .map_or(defaults.max_total, |v| (v * 1e9) as u64),
        ..defaults
    };
    let many = inputs.len() > 1;
    let n = inputs.len() as f32;
    let mut out = Outputs::new(&ctx.guard);
    let mut made: Vec<PathBuf> = Vec::new();
    let mut result: Result<(), ModuleError> = Ok(());
    for (i, path) in inputs.iter().enumerate() {
        let name = file_name(path);
        let step = (|| -> Result<(), ModuleError> {
            ctx.check_cancel()?;
            let file = ctx.guard.open_read(path)?;
            let dir = out.dir_named(path, &panoply_archive::archive_stem(&name))?;
            made.push(dir.clone());
            let report = panoply_archive::extract(
                file,
                &name,
                &dir,
                &limits,
                &ctx.cancel,
                &mut |f, label| ctx.progress((i as f32 + f) / n, label.to_string()),
            )
            .map_err(|e| map_err(e, many.then_some(name.as_str())))?;
            let prefix = if many {
                format!("{name}: ")
            } else {
                String::new()
            };
            let folder = file_name(&dir);
            let mut line = format!(
                "{prefix}Unpacked {}, {}, {} into \"{folder}\".",
                plural(report.files, "file", "files"),
                plural(report.folders, "folder", "folders"),
                size_text(report.bytes)
            );
            if report.links > 0 {
                line.push_str(&format!(" {} kept.", plural(report.links, "link", "links")));
            }
            ctx.note(line);
            if report.renamed > 0 {
                ctx.note(format!(
                    "{prefix}{} had the same name as another file in the archive, so the later one got a number.",
                    plural(report.renamed, "file", "files")
                ));
            }
            if !report.skipped.is_empty() {
                let shown: Vec<String> = report
                    .skipped
                    .iter()
                    .take(5)
                    .map(|s| format!("\"{}\" {}", s.name, s.reason))
                    .collect();
                let more = report.skipped.len().saturating_sub(5);
                let mut m = format!(
                    "{prefix}Left out {} that could harm your files: {}",
                    plural(report.skipped.len() as u64, "entry", "entries"),
                    shown.join("; ")
                );
                if more > 0 {
                    m.push_str(&format!("; and {more} more"));
                }
                m.push('.');
                ctx.note(m);
            }
            if let Some(d) = &report.damaged {
                ctx.note(format!(
                    "{prefix}{d} The files before the damage were unpacked; the rest could not be read."
                ));
            }
            Ok(())
        })();
        if let Err(e) = step {
            result = Err(e);
            break;
        }
    }
    if let Err(e) = result {
        // Every folder was made by this job, so removing them whole is safe.
        for d in made.iter().rev() {
            let _ = std::fs::remove_dir_all(d);
        }
        return Err(e);
    }
    Ok(out.finish_dirs(made))
}

fn create(ctx: &RunContext, inputs: &[PathBuf], opts: &Value) -> Result<Vec<PathBuf>, ModuleError> {
    let format = match opts.get("format").and_then(Value::as_str) {
        Some("tar.gz") => Format::TarGz,
        Some("tar.zst") => Format::TarZst,
        _ => Format::Zip,
    };
    let level = match opts.get("level").and_then(Value::as_str) {
        Some("fast") => 2,
        Some("small") => 9,
        _ => 6,
    };
    let dir = ctx.guard.output_dir_for(&inputs[0])?;
    let base = if inputs.len() == 1 {
        let p = &inputs[0];
        if p.is_dir() {
            file_name(p)
        } else {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    } else {
        "Archive".to_string()
    };
    let base = if base.is_empty() {
        "Archive".to_string()
    } else {
        base
    };
    let ext = format.extension();
    let mut k = 1;
    let name = loop {
        let n = if k == 1 {
            format!("{base}.{ext}")
        } else {
            format!("{base} ({k}).{ext}")
        };
        if !dir.join(&n).exists() {
            break n;
        }
        k += 1;
    };
    let mut out = Outputs::new(&ctx.guard);
    let mut pending = out.file_in(&dir, &name)?;
    let skip = pending.temp_path().to_path_buf();
    let refused: RefCell<Option<ModuleError>> = RefCell::new(None);
    let report = panoply_archive::create(
        inputs,
        pending.file(),
        &CreateOptions { format, level },
        &skip,
        &ctx.cancel,
        &mut |f, label| ctx.progress(f, label.to_string()),
        &mut |p| {
            ctx.guard.open_read(p).map_err(|e| {
                *refused.borrow_mut() = Some(e);
                ArchiveError::Failed(String::new())
            })
        },
    )
    .map_err(|e| match refused.borrow_mut().take() {
        Some(m) => m,
        None => map_err(e, None),
    })?;
    let path = out.commit(pending)?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    ctx.note(format!(
        "{}, {}, {} packed into {} ({}).",
        plural(report.files, "file", "files"),
        plural(report.folders, "folder", "folders"),
        size_text(report.bytes_in),
        size_text(size),
        file_name(&path)
    ));
    if report.links > 0 {
        ctx.note(format!(
            "{} stored as links, not followed.",
            plural(report.links, "link", "links")
        ));
    }
    for (name, why) in &report.skipped {
        ctx.note(format!("Left out \"{name}\": {why}."));
    }
    Ok(out.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-archmod-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(
        id: &str,
        inputs: &[PathBuf],
        options: Value,
    ) -> Result<(Vec<PathBuf>, Vec<String>), ModuleError> {
        let spec = panoply_core::JobSpec {
            module_id: id.into(),
            inputs: inputs.to_vec(),
            options,
        };
        let o = crate::Registry::with_builtins().run_job(
            &spec,
            None,
            panoply_core::CancelToken::new(),
            |_| {},
        )?;
        Ok((o.outputs, o.notes))
    }

    #[test]
    fn manifests_are_valid() {
        let ms = all();
        assert_eq!(ms.len(), 2);
        for m in &ms {
            m.manifest.validate().unwrap();
            assert!(m.manifest.module.id.starts_with("archive."));
        }
    }

    #[test]
    fn pack_then_unpack_next_to_the_archive_never_overwrites() {
        let d = scratch("rt");
        let src = d.join("project");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "alpha").unwrap();
        std::fs::write(src.join("sub").join("b.txt"), "beta").unwrap();
        for fmt in ["zip", "tar.gz", "tar.zst"] {
            let (out, notes) = run(
                "archive.create",
                std::slice::from_ref(&src),
                json!({"format": fmt}),
            )
            .unwrap();
            assert!(notes.iter().any(|n| n.starts_with("2 files")), "{notes:?}");
            let arc = &out[0];
            assert!(
                file_name(arc).starts_with("project") && file_name(arc).ends_with(fmt),
                "{arc:?}"
            );
            let (dirs, _) = run("archive.extract", std::slice::from_ref(arc), json!({})).unwrap();
            assert_eq!(dirs.len(), 1);
            assert_eq!(dirs[0].parent().unwrap(), d.canonicalize().unwrap());
            assert_eq!(
                std::fs::read_to_string(dirs[0].join("project").join("sub").join("b.txt")).unwrap(),
                "beta"
            );
            // A second extract goes to a new folder.
            let (again, _) = run("archive.extract", std::slice::from_ref(arc), json!({})).unwrap();
            assert_ne!(dirs[0], again[0]);
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}
