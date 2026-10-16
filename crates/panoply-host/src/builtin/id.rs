//! The three built-in ID modules (`id.passport_photo`, `id.print_sheet`, `id.signature_cleaner`).
//!
//! The work lives in `panoply-id`. This file only connects it to the host, the same way
//! `image.rs` does for the image modules: reads go through the [`CapabilityGuard`], new files are
//! created inside the folder the guard allows (next to the original unless an output folder was
//! chosen) and are never overwritten, cancel and progress are forwarded, and a job that stops
//! with an error removes the files it had already written.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use panoply_core::CancelToken;
use panoply_id::modules::{self, IdModule};
use panoply_image::error::ImageError;
use panoply_image::ops::{Io, NameSpec, pick_name};
use panoply_manifest::Manifest;

use crate::{Module, ModuleError, RunContext};

/// One ID module behind the host's [`Module`] trait.
pub struct IdHostModule {
    inner: IdModule,
}

/// All three ID modules.
pub fn all() -> Vec<IdHostModule> {
    modules::modules()
        .into_iter()
        .map(|inner| IdHostModule { inner })
        .collect()
}

/// [`Io`] over the host's guard and run context.
struct GuardIo<'a> {
    ctx: &'a RunContext,
    created: Mutex<Vec<PathBuf>>,
}

fn to_image(e: ModuleError) -> ImageError {
    match e {
        ModuleError::Cancelled => ImageError::Cancelled,
        other => ImageError::Io(format!("{} {}", other.plain_message(), other.next_step())),
    }
}

impl Io for GuardIo<'_> {
    fn open_read(&self, path: &Path) -> Result<File, ImageError> {
        self.ctx.guard.open_read(path).map_err(to_image)
    }

    fn create_output(
        &self,
        input: &Path,
        name: &NameSpec,
        ext: &str,
    ) -> Result<(PathBuf, File), ImageError> {
        let dir = self.ctx.guard.output_dir_for(input).map_err(to_image)?;
        loop {
            let path = pick_name(input, &dir, name, ext, |p| p.exists());
            // Read and write: the size search checks a file by reading it back before it commits.
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(f) => {
                    self.created
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(path.clone());
                    return Ok((path, f));
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(ImageError::Io(format!(
                        "Could not write {}: {}. Check the folder and the free space, then try again.",
                        path.display(),
                        panoply_core::naming::plain_io(&e)
                    )));
                }
            }
        }
    }

    fn cancel(&self) -> &CancelToken {
        &self.ctx.cancel
    }

    fn progress(&self, fraction: f32, label: &str) {
        self.ctx.progress(fraction, label);
    }

    fn note(&self, line: String) {
        self.ctx.note(line);
    }
}

impl Module for IdHostModule {
    fn manifest(&self) -> &Manifest {
        &self.inner.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        ctx.check_cancel()?;
        let io = GuardIo {
            ctx,
            created: Mutex::new(Vec::new()),
        };
        match self.inner.run(&io, inputs, options) {
            Ok(outputs) => Ok(outputs),
            Err(e) => {
                // Nothing half done: take back every file this job created.
                for p in io
                    .created
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .drain(..)
                {
                    let _ = std::fs::remove_file(p);
                }
                Err(match e {
                    ImageError::Cancelled => ModuleError::Cancelled,
                    other => ModuleError::Failed(other.to_string()),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Registry;
    use panoply_core::{CancelToken, JobSpec};
    use serde_json::json;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-host-id-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A 64 x 40 24 bit BMP with a gradient.
    fn bmp() -> Vec<u8> {
        let (w, h) = (64usize, 40usize);
        let row = (w * 3).div_ceil(4) * 4;
        let mut pix = Vec::new();
        for y in 0..h {
            for x in 0..w {
                pix.extend_from_slice(&[(x * 4) as u8, (y * 6) as u8, 128]);
            }
            pix.resize((y + 1) * row, 0);
        }
        let mut v = b"BM".to_vec();
        v.extend_from_slice(&((54 + pix.len()) as u32).to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&54u32.to_le_bytes());
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&(w as u32).to_le_bytes());
        v.extend_from_slice(&(h as u32).to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&24u16.to_le_bytes());
        v.extend_from_slice(&[0; 24]);
        v.extend_from_slice(&pix);
        v
    }

    fn spec(id: &str, input: &Path, options: serde_json::Value) -> JobSpec {
        JobSpec {
            module_id: id.into(),
            inputs: vec![input.to_path_buf()],
            options,
        }
    }

    #[test]
    fn the_registry_has_the_three_id_modules_with_matching_manifests() {
        let r = Registry::with_builtins();
        for id in [
            "id.passport_photo",
            "id.print_sheet",
            "id.signature_cleaner",
        ] {
            let m = r
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not registered"));
            assert_eq!(m.manifest().module.id, id);
        }
    }

    #[test]
    fn a_print_sheet_job_writes_next_to_the_original_and_never_overwrites() {
        let d = scratch("sheet");
        let input = d.join("photo.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let r = Registry::with_builtins();
        let opts = json!({"size": "35x45", "paper": "4x6"});
        let a = r
            .run_job(
                &spec("id.print_sheet", &input, opts.clone()),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap();
        let b = r
            .run_job(
                &spec("id.print_sheet", &input, opts),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap();
        assert_eq!(a.outputs[0].file_name().unwrap(), "photo (sheet).jpg");
        assert_eq!(b.outputs[0].file_name().unwrap(), "photo (sheet 2).jpg");
        assert!(
            a.notes
                .iter()
                .any(|n| n.contains("photos, 35 x 45 mm each")),
            "{:?}",
            a.notes
        );
        assert_eq!(
            std::fs::read(&input).unwrap(),
            bmp(),
            "the original must not change"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_chosen_output_folder_gets_the_result() {
        let d = scratch("outdir");
        let input = d.join("photo.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let r = Registry::with_builtins();
        let o = r
            .run_job(
                &spec(
                    "id.print_sheet",
                    &input,
                    json!({"size": "35x45", "format": "pdf"}),
                ),
                Some(&out),
                CancelToken::new(),
                |_| {},
            )
            .unwrap();
        assert!(
            o.outputs[0].starts_with(out.canonicalize().unwrap()),
            "{:?}",
            o.outputs
        );
        assert_eq!(o.outputs[0].extension().unwrap(), "pdf");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_failed_job_leaves_nothing_and_speaks_plainly() {
        let d = scratch("fail");
        let bad = d.join("notes.txt");
        std::fs::write(&bad, "this is not a picture").unwrap();
        let good = d.join("photo.bmp");
        std::fs::write(&good, bmp()).unwrap();
        let r = Registry::with_builtins();
        let err = r
            .run_job(
                &spec("id.print_sheet", &bad, json!({"size": "35x45"})),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap_err();
        assert!(
            err.plain_message().contains("not an image"),
            "{}",
            err.plain_message()
        );
        // A print size is never guessed.
        let err = r
            .run_job(
                &spec("id.print_sheet", &good, json!({})),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap_err();
        assert!(
            err.plain_message().contains("will not guess"),
            "{}",
            err.plain_message()
        );
        // A cancelled job says so and leaves nothing behind.
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = r
            .run_job(
                &spec("id.signature_cleaner", &good, json!({})),
                None,
                cancel,
                |_| {},
            )
            .unwrap_err();
        assert!(matches!(err, ModuleError::Cancelled));
        let mut names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["notes.txt", "photo.bmp"],
            "a failed job left files behind"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bad_option_is_a_plain_sentence() {
        let d = scratch("opt");
        let input = d.join("photo.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let r = Registry::with_builtins();
        let err = r
            .run_job(
                &spec(
                    "id.print_sheet",
                    &input,
                    json!({"paper": "tabloid", "size": "35x45"}),
                ),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap_err();
        assert!(
            err.plain_message().contains("not a paper size"),
            "{}",
            err.plain_message()
        );
        let err = r
            .run_job(
                &spec("id.passport_photo", &input, json!({"engine": "magic"})),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap_err();
        assert!(
            err.plain_message().contains("must be one of"),
            "{}",
            err.plain_message()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
