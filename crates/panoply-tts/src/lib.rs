//! Text to speech on the operating system's own voices.
//!
//! [`speak_to_wav`] splits the text into chunks of whole sentences, has the system voice
//! render them one after another and appends the audio to a WAV file as it arrives, reporting
//! progress after each chunk and checking for a cancel between chunks and between audio
//! blocks. Nothing is ever played: the engines write to memory and this writes to a file.
//!
//! Engines: `AVSpeechSynthesizer` on macOS, `Windows.Media.SpeechSynthesis` on Windows. Linux
//! has none (see the README), so [`available`] is false there.

mod engine;
mod error;
pub mod text;
pub mod wav;

use std::path::Path;

pub use error::{Result, TtsError};
pub use panoply_core::CancelToken;

/// One installed voice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// What to pass as [`Options::voice`].
    pub id: String,
    /// The name people know it by.
    pub name: String,
    /// A language tag such as `en-US`.
    pub language: String,
    /// `standard`, `enhanced` or `premium`.
    pub quality: &'static str,
}

/// How to speak.
#[derive(Debug, Clone)]
pub struct Options {
    /// A voice id, name or language tag. `None` is the system default voice.
    pub voice: Option<String>,
    /// 1.0 is normal speed. 0.5 to 3.0.
    pub speed: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            voice: None,
            speed: 1.0,
        }
    }
}

/// What a finished job tells the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Length of the audio.
    pub seconds: f64,
    /// Sample rate of the WAV.
    pub sample_rate: u32,
    /// Root mean square level of everything written, 0.0 to 1.0 (a silent file is near 0).
    pub rms: f64,
    /// How many chunks were spoken.
    pub chunks: usize,
    /// Characters spoken.
    pub characters: usize,
    /// The voice asked for, or `None` for the default.
    pub voice: Option<String>,
}

/// macOS only. Runs the main thread's run loop for up to `seconds`. A process that renders on a
/// worker thread while its main thread waits (an MCP server reading stdin) calls this in a loop
/// on the main thread until the worker is done, because the system voice answers on the main
/// queue. See `panoply_platform_macos::speech`.
#[cfg(target_os = "macos")]
pub fn pump_main_run_loop(seconds: f64) {
    panoply_platform_macos::speech::pump_main_run_loop(seconds);
}

/// True when this system has a speech engine with at least one voice.
pub fn available() -> bool {
    engine::available()
}

/// The installed voices. Empty when there is no engine.
pub fn voices() -> Vec<Voice> {
    engine::voices()
}

/// Speak `text` into the WAV file `out`.
///
/// `progress` gets a fraction (0.0 to 1.0, by characters) and a short label after each chunk.
/// A cancel removes the partial file and returns [`TtsError::Cancelled`].
pub fn speak_to_wav(
    text: &str,
    opts: &Options,
    out: &Path,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &CancelToken,
) -> Result<Report> {
    let result = speak_inner(text, opts, out, progress, cancel);
    if result.is_err() {
        let _ = std::fs::remove_file(out);
    }
    result
}

fn speak_inner(
    text: &str,
    opts: &Options,
    out: &Path,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &CancelToken,
) -> Result<Report> {
    let chunks = text::chunks(text, text::MAX_CHUNK);
    if chunks.is_empty() {
        return Err(TtsError::BadInput(
            "There is nothing to read. Paste some text or choose a file with words in it.".into(),
        ));
    }
    if !available() {
        return Err(TtsError::Unsupported(
            "Reading aloud is not available on this system.".into(),
        ));
    }
    let total: usize = chunks.iter().map(|c| c.chars().count()).sum();
    let settings = engine::Settings {
        voice: opts.voice.as_deref().filter(|v| !v.trim().is_empty()),
        speed: opts.speed,
    };
    let mut writer: Option<wav::WavWriter> = None;
    let mut done_chars = 0usize;
    // Level of what was written, summed as the blocks arrive instead of reading the file back.
    let (mut sum_squares, mut sample_count) = (0f64, 0u64);
    for (i, chunk) in chunks.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        let mut failure: Option<TtsError> = None;
        engine::render(chunk, &settings, &mut |pcm| {
            if cancel.is_cancelled() {
                return false;
            }
            let w = match writer.as_mut() {
                Some(w) => w,
                None => match wav::WavWriter::create(out, pcm.sample_rate, pcm.channels) {
                    Ok(w) => writer.insert(w),
                    Err(e) => {
                        failure = Some(e);
                        return false;
                    }
                },
            };
            if pcm.sample_rate != w.sample_rate() || pcm.channels != w.channels() {
                failure = Some(TtsError::Engine(
                    "The voice changed its audio format in the middle of a job.".into(),
                ));
                return false;
            }
            for &x in &pcm.samples {
                let v = f64::from(x) / 32768.0;
                sum_squares += v * v;
            }
            sample_count += pcm.samples.len() as u64;
            if let Err(e) = w.write(&pcm.samples) {
                failure = Some(e);
                return false;
            }
            true
        })?;
        if let Some(e) = failure {
            return Err(e);
        }
        if cancel.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        done_chars += chunk.chars().count();
        progress(
            done_chars as f32 / total as f32,
            &format!("Spoke part {} of {}", i + 1, chunks.len()),
        );
    }
    let writer = writer.ok_or_else(|| {
        TtsError::Engine(
            "The voice produced no sound. Pick another voice or check the text.".into(),
        )
    })?;
    let sample_rate = writer.sample_rate();
    let seconds = writer.finish()?;
    Ok(Report {
        seconds,
        sample_rate,
        rms: if sample_count == 0 {
            0.0
        } else {
            (sum_squares / sample_count as f64).sqrt()
        },
        chunks: chunks.len(),
        characters: total,
        voice: settings.voice.map(str::to_string),
    })
}

/// Length in seconds and root mean square level (0.0 to 1.0) of a 16 bit PCM WAV file. Used by
/// tests and by the result card to refuse a silent file.
pub fn wav_stats(path: &Path) -> Result<(f64, f64)> {
    let (rate, channels, samples) = wav::parse(&std::fs::read(path)?)?;
    if samples.is_empty() {
        return Ok((0.0, 0.0));
    }
    let seconds = samples.len() as f64 / (f64::from(rate) * f64::from(channels));
    let sum: f64 = samples
        .iter()
        .map(|&s| {
            let v = f64::from(s) / 32768.0;
            v * v
        })
        .sum();
    Ok((seconds, (sum / samples.len() as f64).sqrt()))
}

#[cfg(all(test, any(target_os = "macos", windows)))]
mod tests {
    use super::*;

    // The speech itself is tested end to end by `crates/panoply-cli/tests/speak.rs`: macOS
    // answers on the main thread, which a unit test's worker thread is not.
    #[test]
    fn a_job_cancelled_before_it_starts_leaves_no_file() {
        let out =
            std::env::temp_dir().join(format!("panoply-tts-cancel-{}.wav", std::process::id()));
        let cancel = CancelToken::new();
        cancel.cancel();
        let r = speak_to_wav(
            "One sentence.",
            &Options::default(),
            &out,
            &mut |_, _| {},
            &cancel,
        );
        assert!(matches!(r, Err(TtsError::Cancelled)));
        assert!(!out.exists());
    }

    #[test]
    fn empty_text_is_refused_in_plain_words() {
        let out = std::env::temp_dir().join("panoply-tts-empty.wav");
        let r = speak_to_wav(
            "  \n ",
            &Options::default(),
            &out,
            &mut |_, _| {},
            &CancelToken::new(),
        );
        assert!(matches!(r, Err(TtsError::BadInput(_))));
    }
}
