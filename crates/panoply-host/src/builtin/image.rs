//! The five built-in image modules (`image.convert`, `image.compress`, `image.resize`,
//! `image.resize_to_kb`, `image.strip_metadata`).
//!
//! The work lives in `panoply-image`. This file only connects it to the host: reads go through
//! the [`CapabilityGuard`], new files are created inside the folder the guard allows (next to
//! the original unless an output folder was chosen) and are never overwritten, cancel and
//! progress are forwarded, and a job that stops with an error removes the files it had already
//! written, so a failed or cancelled run leaves nothing behind.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use panoply_core::CancelToken;
use panoply_image::error::ImageError;
use panoply_image::modules::{self, ImageModule};
use panoply_image::ops::{Io, NameSpec, pick_name};
use panoply_manifest::Manifest;

use crate::{Module, ModuleError, RunContext};

/// One image module behind the host's [`Module`] trait.
pub struct ImageHostModule {
    inner: ImageModule,
}

/// All five image modules.
pub fn all() -> Vec<ImageHostModule> {
    modules::modules()
        .into_iter()
        .map(|inner| ImageHostModule { inner })
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
            // Read and write: Compress checks a file by reading it back before it commits.
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
                        "Could not write {} ({e}). Check the folder and the free space, then try again.",
                        path.display()
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

/// A first preview of an image: a small upright picture, made without decoding the whole
/// picture where the format allows it (the file's own thumbnail for HEIC, RAW and JPEG, a reduced
/// decode otherwise). The file is read through the capability guard like a job's input.
///
/// With `fast` the file's thumbnail is taken at any size, for a first look that the caller
/// replaces with a sharper preview (`fast = false`) when it arrives; see
/// `panoply_image::preview` for what each format costs.
pub fn preview(
    ctx: &RunContext,
    input: &Path,
    max_side: u32,
    fast: bool,
) -> Result<panoply_image::preview::Preview, ModuleError> {
    ctx.check_cancel()?;
    let file = ctx.guard.open_read(input)?;
    let ext = input.extension().and_then(|e| e.to_str());
    panoply_image::preview::preview(file, ext, max_side, fast)
        .map_err(|e| ModuleError::Failed(e.to_string()))
}

impl Module for ImageHostModule {
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
                // Nothing half done: take back every file this job created. Files that were
                // already removed (a partial file dropped by its writer) are simply skipped.
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
        let d =
            std::env::temp_dir().join(format!("panoply-host-image-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A 4 x 4 24 bit BMP with a gradient.
    fn bmp() -> Vec<u8> {
        let (w, h) = (4usize, 4usize);
        let row = (w * 3).div_ceil(4) * 4;
        let mut pix = Vec::new();
        for y in 0..h {
            for x in 0..w {
                pix.extend_from_slice(&[(x * 60) as u8, (y * 60) as u8, 128]);
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
    fn the_registry_has_all_five_image_modules_with_matching_manifests() {
        let r = Registry::with_builtins();
        for id in [
            "image.convert",
            "image.compress",
            "image.resize",
            "image.resize_to_kb",
            "image.strip_metadata",
        ] {
            let m = r
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not registered"));
            assert_eq!(m.manifest().module.id, id);
        }
    }

    #[test]
    fn a_job_writes_next_to_the_original_and_never_overwrites() {
        let d = scratch("next");
        let input = d.join("pic.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let r = Registry::with_builtins();
        let a = r
            .run_job(
                &spec("image.convert", &input, json!({"to": "png"})),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap();
        let b = r
            .run_job(
                &spec("image.convert", &input, json!({"to": "png"})),
                None,
                CancelToken::new(),
                |_| {},
            )
            .unwrap();
        assert_eq!(a.outputs[0].file_name().unwrap(), "pic.png");
        assert_eq!(b.outputs[0].file_name().unwrap(), "pic (1).png");
        assert!(
            a.notes.iter().any(|n| n.contains("pic.bmp")),
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
        let input = d.join("pic.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let r = Registry::with_builtins();
        let o = r
            .run_job(
                &spec(
                    "image.resize",
                    &input,
                    json!({"mode": "percent", "percent": 50}),
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
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_failed_or_cancelled_job_leaves_nothing_and_speaks_plainly() {
        let d = scratch("fail");
        let bad = d.join("notes.txt");
        std::fs::write(&bad, "this is not a picture").unwrap();
        let good = d.join("pic.bmp");
        std::fs::write(&good, bmp()).unwrap();
        let r = Registry::with_builtins();
        let err = r
            .run_job(
                &spec("image.convert", &bad, json!({})),
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
        assert!(!err.plain_message().contains("panicked"));
        let cancel = CancelToken::new();
        cancel.cancel();
        let err = r
            .run_job(
                &spec("image.convert", &good, json!({})),
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
            ["notes.txt", "pic.bmp"],
            "a failed job left files behind"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bad_option_is_a_plain_sentence() {
        let d = scratch("opt");
        let input = d.join("pic.bmp");
        std::fs::write(&input, bmp()).unwrap();
        let r = Registry::with_builtins();
        let err = r
            .run_job(
                &spec("image.convert", &input, json!({"to": "doc"})),
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
