//! The four built-in image AI modules (`image.remove_background`, `image.erase`,
//! `image.remove_watermark`, `image.upscale`).
//!
//! The work lives in `panoply-vision`. This file only connects it to the host, the same way
//! `builtin/image.rs` does for the other image modules: reads go through the [`CapabilityGuard`],
//! new files are created inside the folder the guard allows (next to the original unless an
//! output folder was chosen) and are never overwritten, cancel and progress are forwarded, and a
//! job that stops with an error removes the files it had already written. The network is used
//! only when the manifest asked for it and the guard grants it (model and runtime downloads).

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use panoply_core::CancelToken;
use panoply_image::error::ImageError;
use panoply_image::ops::{Io, NameSpec, pick_name};
use panoply_manifest::Manifest;
use panoply_vision::VisionIo;
use panoply_vision::modules::{self, VisionModule};

use crate::{Module, ModuleError, RunContext};

/// One image AI module behind the host's [`Module`] trait.
pub struct VisionHostModule {
    inner: VisionModule,
}

/// All four image AI modules.
pub fn all() -> Vec<VisionHostModule> {
    modules::modules()
        .into_iter()
        .map(|inner| VisionHostModule { inner })
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

impl VisionIo for GuardIo<'_> {
    fn network_allowed(&self) -> bool {
        self.ctx.guard.require_network().is_ok()
    }
}

impl Module for VisionHostModule {
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
    use panoply_core::JobSpec;
    use serde_json::json;

    #[test]
    fn the_registry_has_all_four_vision_modules_with_matching_manifests() {
        let r = Registry::with_builtins();
        for id in [
            "image.remove_background",
            "image.erase",
            "image.remove_watermark",
            "image.upscale",
        ] {
            let m = r
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not registered"));
            assert_eq!(m.manifest().module.id, id);
            assert!(m.manifest().capabilities.network);
        }
    }

    #[test]
    fn the_watermark_module_refuses_without_the_ownership_confirmation() {
        let d = std::env::temp_dir().join(format!("panoply-host-vision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let input = d.join("pic.png");
        std::fs::write(&input, b"not really a picture").unwrap();
        let r = Registry::with_builtins();
        let spec = JobSpec {
            module_id: "image.remove_watermark".into(),
            inputs: vec![input.clone()],
            options: json!({}),
        };
        let err = r
            .run_job(&spec, None, CancelToken::new(), |_| {})
            .unwrap_err();
        assert!(
            err.plain_message().contains("own_content"),
            "{}",
            err.plain_message()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_job_with_no_network_grant_cannot_download_a_model() {
        // The manifest asks for the network, but a guard built without it (as a host does
        // when the user has not approved) refuses: the plain message names the download.
        let d =
            std::env::temp_dir().join(format!("panoply-host-vision-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let ctx = RunContext::new(crate::CapabilityGuard::none());
        let io = GuardIo {
            ctx: &ctx,
            created: Mutex::new(Vec::new()),
        };
        assert!(!io.network_allowed());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
