//! The built-in text to speech module, `text.to_speech`, on the `panoply-tts` engine: pasted
//! text or a `.txt`, `.md` or PDF file becomes a WAV file, spoken by the operating system's own
//! voice. Nothing is downloaded and nothing is played.
//!
//! On Linux there is no engine (see `docs/KNOWN-LIMITATIONS.md`), so [`all`] returns nothing
//! there and the tool does not appear.

use std::path::PathBuf;

use panoply_manifest::Manifest;
use panoply_tts::{Options, TtsError};
use serde_json::Value;

use crate::{Module, ModuleError, Outputs, RunContext};

const TEXT_TO_SPEECH: &str = include_str!("../../../../modules/builtin/text-to-speech/module.toml");

/// The text to speech module, or nothing on a system with no speech engine.
pub fn all() -> Vec<TtsModule> {
    if !cfg!(any(target_os = "macos", windows)) {
        return Vec::new();
    }
    vec![TtsModule {
        manifest: Manifest::from_toml_str(TEXT_TO_SPEECH).expect("built-in tts manifest parses"),
    }]
}

/// `text.to_speech`.
pub struct TtsModule {
    manifest: Manifest,
}

fn map_err(e: TtsError) -> ModuleError {
    match e {
        TtsError::Cancelled => ModuleError::Cancelled,
        other => ModuleError::Failed(other.to_string()),
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(p: &std::path::Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "speech".into())
}

fn clock(seconds: f64) -> String {
    let s = seconds.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Module for TtsModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        let opts = crate::options::resolve(&self.manifest, options)?;
        let pasted = opts
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let speak = Options {
            voice: opts
                .get("voice")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            speed: opts.get("speed").and_then(Value::as_f64).unwrap_or(1.0) as f32,
        };
        // What to read: the pasted text as one job, or each chosen file as its own job.
        let mut jobs: Vec<(String, String, Option<PathBuf>)> = Vec::new();
        let mut skipped: Option<String> = None;
        if inputs.is_empty() {
            if pasted.is_empty() {
                return Err(ModuleError::Failed(
                    "Paste some text or choose a .txt, .md or PDF file first.".into(),
                ));
            }
            jobs.push(("speech".into(), pasted.to_string(), None));
        } else {
            for path in inputs {
                ctx.check_cancel()?;
                ctx.guard.open_read(path)?;
                ctx.progress(0.0, format!("Reading {}", file_name(path)));
                let text = match panoply_tts::text::read_file(path, &ctx.cancel) {
                    Ok(t) => t,
                    Err(TtsError::Cancelled) => return Err(ModuleError::Cancelled),
                    // With several files, one that cannot be read aloud (a scan with no text
                    // layer) is skipped and said so, and the others are still read.
                    Err(TtsError::BadInput(why)) if inputs.len() > 1 => {
                        ctx.note(format!("{}: skipped. {why}", file_name(path)));
                        skipped = Some(why);
                        continue;
                    }
                    Err(other) if inputs.len() > 1 => {
                        return Err(ModuleError::Failed(format!("{}: {other}", file_name(path))));
                    }
                    Err(other) => return Err(map_err(other)),
                };
                jobs.push((stem(path), text, Some(path.clone())));
            }
        }

        if jobs.is_empty() {
            return Err(ModuleError::Failed(skipped.unwrap_or_else(|| {
                "There is nothing to read. Choose a .txt, .md or PDF file with words in it.".into()
            })));
        }
        let mut out = Outputs::new(&ctx.guard);
        let n = jobs.len();
        let mut last = None;
        for (k, (name, text, beside)) in jobs.into_iter().enumerate() {
            ctx.check_cancel()?;
            let pending = match &beside {
                Some(path) => out.file(path, "spoken", "wav")?,
                None => {
                    let dir = ctx.guard.out_dir().ok_or_else(|| {
                        ModuleError::Failed(
                            "Choose a folder for the audio file first, for example --out ~/Music."
                                .into(),
                        )
                    })?;
                    out.file_in(dir, &format!("{name}.wav"))?
                }
            };
            let part = pending.part_path().to_path_buf();
            let share = |f: f32| (k as f32 + f) / n as f32;
            let mut progress = |f: f32, l: &str| ctx.progress(share(f), l.to_string());
            let report =
                panoply_tts::speak_to_wav(&text, &speak, &part, &mut progress, &ctx.cancel)
                    .map_err(|e| match e {
                        TtsError::Cancelled => ModuleError::Cancelled,
                        other if n > 1 => ModuleError::Failed(format!("{name}: {other}")),
                        other => map_err(other),
                    })?;
            let (_, rms) = panoply_tts::wav_stats(&part).map_err(map_err)?;
            if rms < 0.001 {
                return Err(ModuleError::Failed(
                    "The voice produced only silence. Pick another voice and try again.".into(),
                ));
            }
            let path = out.commit(pending)?;
            ctx.note(format!(
                "{}: {} of audio, {} characters in {} {}, {}.",
                file_name(&path),
                clock(report.seconds),
                report.characters,
                report.chunks,
                if report.chunks == 1 { "part" } else { "parts" },
                report
                    .voice
                    .as_deref()
                    .map_or("system voice".to_string(), |v| format!("voice {v}")),
            ));
            last = Some(path);
        }
        let _ = last;
        ctx.progress(1.0, "Done");
        Ok(out.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The speech itself is tested end to end by `crates/panoply-cli/tests/speak.rs`: macOS
    // answers on the main thread, which a unit test's worker thread is not.
    #[test]
    fn the_manifest_parses_and_the_tool_is_only_offered_where_an_engine_exists() {
        let m = Manifest::from_toml_str(TEXT_TO_SPEECH).unwrap();
        assert_eq!(m.module.id, "text.to_speech");
        let ids: Vec<_> = all()
            .iter()
            .map(|m| m.manifest().module.id.clone())
            .collect();
        assert_eq!(ids.is_empty(), !cfg!(any(target_os = "macos", windows)));
    }
}
