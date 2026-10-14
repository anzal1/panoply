//! The built-in media modules (`media.convert`, `media.extract_audio` and the rest of wave 1).
//!
//! The work lives in `panoply-media`. This file only connects it to the host: inputs are checked
//! against the capability guard before the helper program ever sees a path, results go in the
//! folder the guard allows (next to the original unless an output folder was chosen) under a
//! temporary name and are never overwritten, cancel and progress are forwarded, and a job that
//! stops with an error leaves nothing behind.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use panoply_core::CancelToken;
use panoply_manifest::Manifest;
use panoply_media::error::MediaError;
use panoply_media::io::Io;
use panoply_media::modules::{self, MediaModule};

use crate::{Module, ModuleError, RunContext};

/// One media module behind the host's [`Module`] trait.
pub struct MediaHostModule {
    inner: MediaModule,
}

/// Every media module.
pub fn all() -> Vec<MediaHostModule> {
    modules::modules()
        .into_iter()
        .map(|inner| MediaHostModule { inner })
        .collect()
}

/// [`Io`] over the host's guard and run context. A host error that must keep its own kind (a
/// denied path, a missing file) is parked here and returned unchanged after the job stops.
struct GuardIo<'a> {
    ctx: &'a RunContext,
    parked: Mutex<Option<ModuleError>>,
}

impl GuardIo<'_> {
    fn park(&self, e: ModuleError) -> MediaError {
        let msg = format!("{} {}", e.plain_message(), e.next_step());
        *self.parked.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
        MediaError::NotAllowed(msg)
    }
}

impl Io for GuardIo<'_> {
    fn read_path(&self, path: &Path) -> Result<PathBuf, MediaError> {
        // Opening through the guard checks the grant and that the file is there.
        self.ctx.guard.open_read(path).map_err(|e| self.park(e))?;
        path.canonicalize().map_err(|e| {
            self.park(ModuleError::Io {
                path: path.to_path_buf(),
                source: e,
            })
        })
    }

    fn output_dir_for(&self, input: &Path) -> Result<PathBuf, MediaError> {
        self.ctx
            .guard
            .output_dir_for(input)
            .map_err(|e| self.park(e))
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

    fn network_allowed(&self) -> bool {
        self.ctx.guard.require_network().is_ok()
    }
}

impl Module for MediaHostModule {
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
            parked: Mutex::new(None),
        };
        match self.inner.run(&io, inputs, options) {
            Ok(outputs) => Ok(outputs),
            Err(e) => {
                if let Some(parked) = io.parked.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    return Err(parked);
                }
                Err(match e {
                    MediaError::Cancelled => ModuleError::Cancelled,
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

    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("panoply-host-media-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_registry_has_the_media_modules_with_matching_manifests() {
        let r = Registry::with_builtins();
        for m in all() {
            let id = m.manifest().module.id.clone();
            let found = r
                .get(&id)
                .unwrap_or_else(|| panic!("{id} is not registered"));
            assert_eq!(found.manifest().module.id, id);
        }
        assert!(r.get("media.convert").is_some());
    }

    #[test]
    fn a_missing_file_and_an_empty_job_speak_plainly() {
        let d = scratch("plain");
        let r = Registry::with_builtins();
        let spec = JobSpec {
            module_id: "media.convert".into(),
            inputs: vec![d.join("missing.mov")],
            options: json!({}),
        };
        let err = r
            .run_job(&spec, None, CancelToken::new(), |_| {})
            .unwrap_err();
        assert!(
            err.plain_message().contains("Could not find"),
            "{}",
            err.plain_message()
        );
        let spec = JobSpec {
            module_id: "media.convert".into(),
            inputs: vec![],
            options: json!({}),
        };
        let err = r
            .run_job(&spec, None, CancelToken::new(), |_| {})
            .unwrap_err();
        assert!(
            err.plain_message().contains("Choose at least one file"),
            "{}",
            err.plain_message()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A corpus file, or `None` when it is not on disk or there is no ffmpeg to run it.
    fn corpus_with_engine(rel: &str) -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/cache")
            .join(rel);
        if !p.is_file() {
            eprintln!("skipping: {} is not in the corpus", p.display());
            return None;
        }
        if !panoply_media::engine::Engine::use_path_programs() {
            eprintln!("skipping: no ffmpeg on PATH");
            return None;
        }
        Some(p)
    }

    #[test]
    fn jobs_run_through_the_guard_and_write_next_to_the_original() {
        // The grant covers what the user chose, so everything a job reads back about its own
        // results (probe, decode, measure) must work without it.
        let Some(src) = corpus_with_engine("video/blender/bbb-360p-h264-10s.mp4") else {
            return;
        };
        let d = scratch("guard");
        let input = d.join("clip.mp4");
        std::fs::copy(&src, &input).unwrap();
        let r = Registry::with_builtins();
        for (id, options, ext) in [
            ("media.convert", json!({"to": "mkv"}), "mkv"),
            ("media.trim", json!({"keep": "1-3"}), "mp4"),
            ("media.compress", json!({"quality": 20}), "mp4"),
        ] {
            let spec = JobSpec {
                module_id: id.into(),
                inputs: vec![input.clone()],
                options,
            };
            let out = r
                .run_job(&spec, None, CancelToken::new(), |_| {})
                .unwrap_or_else(|e| panic!("{id}: {}", e.plain_message()));
            let notes = out.notes.join(" | ");
            for o in &out.outputs {
                assert_eq!(o.extension().unwrap(), ext, "{id}: {notes}");
                assert_eq!(o.parent().unwrap(), d.canonicalize().unwrap());
            }
            assert!(!notes.is_empty(), "{id} reports what it did");
        }
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".part"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files were left: {leftovers:?}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_cancelled_media_job_leaves_nothing() {
        let Some(src) = corpus_with_engine("video/blender/bbb-1080p-h264-10s.mp4") else {
            return;
        };
        let d = scratch("cancel");
        let input = d.join("clip.mp4");
        std::fs::copy(&src, &input).unwrap();
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let r = Registry::with_builtins();
        let spec = JobSpec {
            module_id: "media.compress".into(),
            inputs: vec![input.clone()],
            options: json!({"under": "3mb"}),
        };
        // Cancel as soon as the first progress arrives, while the helper is running.
        let err = r
            .run_job(&spec, None, cancel, move |e| {
                if matches!(e, panoply_core::JobEvent::Progress { .. }) {
                    c2.cancel();
                }
            })
            .unwrap_err();
        assert!(
            matches!(err, ModuleError::Cancelled),
            "{}",
            err.plain_message()
        );
        let names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["clip.mp4"], "a cancelled job leaves nothing behind");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bad_option_is_a_plain_sentence() {
        let d = scratch("opt");
        let input = d.join("a.mov");
        std::fs::write(&input, "x").unwrap();
        let r = Registry::with_builtins();
        let spec = JobSpec {
            module_id: "media.convert".into(),
            inputs: vec![input],
            options: json!({"to": "docx"}),
        };
        let err = r
            .run_job(&spec, None, CancelToken::new(), |_| {})
            .unwrap_err();
        assert!(
            err.plain_message().contains("must be one of"),
            "{}",
            err.plain_message()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
