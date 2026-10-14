//! `media.extract_audio`: pull the sound out of a video (or convert an audio file).
//!
//! The default is a stream copy: the sound is written exactly as it is stored, with no decode,
//! so it is bit-exact and takes seconds (spec 4.10). A format is chosen only when the user asks
//! for one.

use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::probe::Profile;
use crate::units::name_of;

use super::audio::{self, AudioFormat, AudioWish};
use super::{Job, change_text, with_name};

/// Run `media.extract_audio`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        extract_one(&mut job, input, opts).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn extract_one(job: &mut Job, input: &Path, opts: &Opts) -> Result<()> {
    let input = job.input(input)?;
    let profile = job.probe(&input)?;
    let tracks = audio::pick_tracks(&profile, opts.text("track", "first"))?;
    let wanted = opts.text("format", "copy");
    let bitrate = opts
        .f64("bitrate")
        .filter(|b| *b > 0.0)
        .map(|b| (b * 1000.0) as u64);
    for (k, s) in tracks.iter().enumerate() {
        let (fmt, forced) = if wanted == "copy" {
            match AudioFormat::for_codec(&s.codec) {
                Some(f) => (f, false),
                None => {
                    // A codec no audio file holds as it is: keep it lossless or lossy as it was.
                    let lossy = crate::encoders::is_lossy_audio(&s.codec);
                    (
                        if lossy {
                            AudioFormat::M4a
                        } else {
                            AudioFormat::Flac
                        },
                        true,
                    )
                }
            }
        } else {
            (
                AudioFormat::from_name(wanted).ok_or_else(|| {
                    MediaError::BadOption(format!(
                        "\"{wanted}\" is not an audio format Panoply can write"
                    ))
                })?,
                false,
            )
        };
        let wish = AudioWish {
            force_encode: forced,
            bitrate,
            channels: None,
        };
        let plan = audio::plan_audio(&job.engine, s, fmt, &wish)?;
        let naming = if tracks.len() > 1 {
            Naming::Labelled(format!("audio {}", k + 1))
        } else {
            Naming::Plain
        };
        let out = audio::write_audio(job, &input, &profile, s, fmt, &plan, &naming, None)?;
        let after = std::fs::metadata(&out)?.len();
        job.verify(&out, s.duration.or(profile.duration))?;
        let mut line = format!("{}: {}", name_of(&out), plan.describe);
        if plan.copy {
            line.push_str(&format!(
                " ({}). No re-encode, lossless.",
                crate::units::size_text(after)
            ));
        } else {
            line.push_str(&format!(" ({}).", change_text(profile.size, after)));
        }
        job.note(line);
        report_track(job, &profile, s.index);
        for n in plan.notes {
            job.note(n);
        }
    }
    Ok(())
}

fn report_track(job: &Job, profile: &Profile, index: u32) {
    if let Some(s) = profile.streams.iter().find(|s| s.index == index)
        && profile.of(crate::probe::Kind::Audio).count() > 1
    {
        let lang = if s.language.is_empty() {
            "unknown language".to_string()
        } else {
            s.language.clone()
        };
        let title = if s.title.is_empty() {
            String::new()
        } else {
            format!(", {}", s.title)
        };
        job.note(format!("Track: stream {} ({lang}{title}).", s.index));
    }
}
