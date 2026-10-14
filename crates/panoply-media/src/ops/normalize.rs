//! `media.normalize`: make the loudness even and meet a standard (spec 5.4).
//!
//! Measured with Panoply's own EBU R128 meter ([`crate::loudness`]), not ffmpeg's `loudnorm`.
//! Gain is linear when the peaks allow it. When they do not, a short look-ahead true-peak
//! limiter takes only the peaks down, never dynamic compression. The result is measured again
//! and corrected, so what the card shows is the loudness of the file that was written.

use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::loudness::Loudness;
use crate::opts::Opts;
use crate::presets;
use crate::probe::{Kind, Profile, Stream};
use crate::units::name_of;

use super::audio;
use super::rewrite::{self, Treatment};
use super::scan;
use super::{Args, Job, with_name};

/// How the gain is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reach a loudness target.
    Loudness,
    /// Bring the true peak to a ceiling.
    Peak,
    /// Only write ReplayGain tags, change no sound.
    Tags,
}

/// What to do to one track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainPlan {
    /// Gain in dB.
    pub gain_db: f64,
    /// Limiter ceiling in dBFS when a limiter is needed.
    pub limit_db: Option<f64>,
}

/// Choose the gain for a measurement.
pub fn plan_gain(mode: Mode, l: &Loudness, target: f64, ceiling: f64) -> GainPlan {
    match mode {
        Mode::Peak => GainPlan {
            gain_db: ceiling - l.true_peak_dbtp,
            limit_db: None,
        },
        _ => {
            let gain = target - l.integrated_lufs;
            let after = l.true_peak_dbtp + gain;
            GainPlan {
                gain_db: gain,
                // A small margin under the ceiling: the limiter works on samples, the ceiling
                // is a true peak, and the result is measured and corrected anyway.
                limit_db: (after > ceiling).then_some(ceiling - 0.3),
            }
        }
    }
}

/// The filter chain for a plan.
pub fn filter_for(p: &GainPlan) -> String {
    let mut f = format!("volume={:.3}dB", p.gain_db);
    if let Some(l) = p.limit_db {
        f.push_str(&format!(
            ",alimiter=level_in=1:level_out=1:limit={:.6}:attack=5:release=50:asc=0:level=0:latency=1",
            10f64.powf(l / 20.0)
        ));
    }
    f
}

/// The attempt that was kept.
struct Kept {
    part: PathBuf,
    dir: PathBuf,
    ext: String,
    notes: Vec<String>,
    plans: Vec<(u32, audio::AudioPlan)>,
    after: Vec<Loudness>,
}

/// Run `media.normalize`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mode = match opts.text("mode", "loudness") {
        "peak" => Mode::Peak,
        "tags" => Mode::Tags,
        _ => Mode::Loudness,
    };
    let preset_id = opts.text("preset", "custom").to_string();
    let preset = if preset_id == "custom" {
        None
    } else {
        Some(presets::loudness(&preset_id).ok_or_else(|| {
            MediaError::BadOption(format!("There is no loudness preset called {preset_id}"))
        })?)
    };
    let ceiling_opt = opts.f64("true_peak").unwrap_or(-1.0);
    let target_opt = opts.f64("target").unwrap_or(-16.0);
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        let ceiling = match preset {
            Some(p) if p.true_peak_dbtp != 0.0 => p.true_peak_dbtp,
            _ => ceiling_opt,
        };
        one(
            &mut job,
            input,
            mode,
            preset,
            target_opt,
            ceiling,
            opts.text("track", "first"),
        )
        .map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn target_for(preset: Option<&presets::LoudnessPreset>, custom: f64, s: &Stream) -> f64 {
    match preset {
        Some(p) if s.channels == 1 && p.target_lufs_mono != 0.0 => p.target_lufs_mono,
        Some(p) => p.target_lufs,
        None => custom,
    }
}

fn one(
    job: &mut Job,
    input: &Path,
    mode: Mode,
    preset: Option<&presets::LoudnessPreset>,
    custom_target: f64,
    ceiling: f64,
    track: &str,
) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    let tracks: Vec<Stream> = audio::pick_tracks(&p, track)?
        .into_iter()
        .cloned()
        .collect();
    let span = job.hi - job.lo;
    // 1. Measure every track to work on.
    let mut before: Vec<Loudness> = Vec::new();
    for (k, s) in tracks.iter().enumerate() {
        let f = |x: f32| job.lo + span * (x * 0.3 * (k as f32 + 1.0) / tracks.len() as f32);
        let sc = scan::scan(
            &job.engine,
            job.io,
            &input,
            s,
            s.duration.or(p.duration),
            (f(0.0).min(job.hi), f(1.0).min(job.hi)),
            "Measuring the loudness",
        )?;
        if !sc.loudness.integrated_lufs.is_finite() {
            return Err(MediaError::Unsupported(
                "This sound is too quiet to measure (everything is under -70 LUFS), so there is no loudness to set. Check that it is the file you meant."
                    .into(),
            ));
        }
        before.push(sc.loudness);
    }
    if mode == Mode::Tags {
        return write_tags(job, &input, &p, &tracks, &before);
    }
    if let Some(pr) = preset
        && pr.measure.contains("dialogue")
    {
        job.note(format!(
            "{} defines its target on dialogue loudness; Panoply measured the integrated loudness of the whole file.",
            pr.name
        ));
    }
    let targets: Vec<f64> = tracks
        .iter()
        .map(|s| target_for(preset, custom_target, s))
        .collect();
    let mut plans: Vec<GainPlan> = tracks
        .iter()
        .zip(&before)
        .zip(&targets)
        .map(|((_, l), t)| plan_gain(mode, l, *t, ceiling))
        .collect();
    // Already there: nothing to do.
    let tol = 0.3;
    if mode == Mode::Loudness
        && plans
            .iter()
            .all(|g| g.gain_db.abs() < 0.1 && g.limit_db.is_none())
    {
        job.note(format!(
            "{} is already at {:.1} LUFS (target {:.1}), so it was left as it is.",
            name_of(&input),
            before[0].integrated_lufs,
            targets[0]
        ));
        return Ok(());
    }
    // 2. Write, measure the result, correct, at most three times.
    let mut final_part: Option<Kept> = None;
    for attempt in 0..3 {
        let idx: Vec<u32> = tracks.iter().map(|s| s.index).collect();
        let filters: Vec<String> = plans.iter().map(filter_for).collect();
        let rw = rewrite::rewrite_audio(job, &input, &p, &|s: &Stream| {
            idx.iter().position(|i| *i == s.index).map(|k| Treatment {
                filter: Some(filters[k].clone()),
                channels: None,
            })
        })?;
        let outp = {
            let io = job.io;
            crate::probe::probe_trusted(&job.engine, io, &rw.part)?
        };
        let out_audio: Vec<Stream> = outp.of(Kind::Audio).cloned().collect();
        let all_audio: Vec<usize> = p.of(Kind::Audio).map(|s| s.index as usize).collect();
        let mut after = Vec::new();
        for s in &tracks {
            let k = all_audio
                .iter()
                .position(|i| *i == s.index as usize)
                .unwrap_or(0);
            let os = out_audio
                .get(k)
                .ok_or_else(|| MediaError::Failed("The result lost a sound track.".into()))?;
            let sc = scan::scan(
                &job.engine,
                job.io,
                &rw.part,
                os,
                outp.duration,
                (job.lo + span * 0.6, job.lo + span * 0.9),
                "Checking the result",
            )?;
            after.push(sc.loudness);
        }
        let mut ok = true;
        for (k, a) in after.iter().enumerate() {
            let off = a.integrated_lufs - targets[k];
            let over = a.true_peak_dbtp - ceiling;
            let bad = (mode == Mode::Loudness && off.abs() > tol) || over > 0.05;
            if bad {
                ok = false;
                if mode == Mode::Loudness {
                    plans[k].gain_db -= off;
                    if over > 0.05 {
                        let cur = plans[k].limit_db.unwrap_or(ceiling - 0.3);
                        plans[k].limit_db = Some(cur - over - 0.1);
                    } else if plans[k].limit_db.is_none() && a.true_peak_dbtp - off > ceiling {
                        plans[k].limit_db = Some(ceiling - 0.3);
                    }
                } else {
                    plans[k].gain_db -= over + 0.05;
                }
            }
        }
        let keep = ok || attempt == 2;
        if keep {
            if let Some(old) = final_part.take() {
                job.stage.discard(&old.part);
            }
            final_part = Some(Kept {
                part: rw.part,
                dir: rw.dir,
                ext: rw.ext,
                notes: rw.notes,
                plans: rw.plans,
                after,
            });
            break;
        }
        job.stage.discard(&rw.part);
    }
    let Kept {
        part,
        dir,
        ext,
        notes,
        plans: enc_plans,
        after,
    } = final_part.expect("the loop always keeps an attempt");
    let out = job.commit(
        &part,
        &dir,
        &input,
        &Naming::Labelled("normalized".into()),
        &ext,
    )?;
    job.verify(&out, p.duration)?;
    for (k, s) in tracks.iter().enumerate() {
        let (b, a) = (&before[k], &after[k]);
        let label = if tracks.len() > 1 {
            format!("Track {}: ", k + 1)
        } else {
            String::new()
        };
        let limiter = if plans[k].limit_db.is_some() {
            "a true-peak limiter took the peaks down"
        } else {
            "linear gain only, no limiter"
        };
        job.note(format!(
            "{}{}: {:.1} LUFS to {:.1} LUFS (target {:.1}), true peak {:.1} to {:.1} dBTP (ceiling {:.1}), gain {:+.1} dB, {limiter}.",
            label,
            name_of(&out),
            b.integrated_lufs,
            a.integrated_lufs,
            targets[k],
            b.true_peak_dbtp,
            a.true_peak_dbtp,
            ceiling,
            plans[k].gain_db
        ));
        let _ = s;
        if (mode == Mode::Loudness && (a.integrated_lufs - targets[k]).abs() > 0.5)
            || a.true_peak_dbtp > ceiling + 0.1
        {
            job.note(format!(
                "{label}The result is not within 0.5 LU of the target or over the peak ceiling: this sound is too peaky for that target without squashing it. Pick a lower target or a higher ceiling."
            ));
        }
    }
    if enc_plans
        .iter()
        .any(|(_, ep)| !ep.copy && !ep.describe.is_empty())
    {
        job.note(format!(
            "Sound written as {}.",
            rewrite::describe(&enc_plans)
        ));
    }
    for n in notes {
        job.note(n);
    }
    Ok(())
}

/// ReplayGain tags only: nothing is decoded again and no sample changes.
fn write_tags(
    job: &mut Job,
    input: &Path,
    p: &Profile,
    tracks: &[Stream],
    loud: &[Loudness],
) -> Result<()> {
    let ext = crate::units::ext_of(input);
    let (dir, part) = job.part(input, &ext)?;
    let first = &loud[0];
    let mut a = Args::new();
    a.a("-y");
    a.input(input);
    a.kv("-map", "0");
    a.kv("-c", "copy");
    a.kv("-map_metadata", "0");
    // ReplayGain 2 references -18 LUFS; Opus uses its own R128 tag, a Q7.8 number against -23.
    if ext == "opus" {
        let q = ((-23.0 - first.integrated_lufs) * 256.0).round() as i64;
        a.kv("-metadata", format!("R128_TRACK_GAIN={q}"));
    } else {
        a.kv(
            "-metadata",
            format!(
                "REPLAYGAIN_TRACK_GAIN={:.2} dB",
                -18.0 - first.integrated_lufs
            ),
        );
        a.kv(
            "-metadata",
            format!(
                "REPLAYGAIN_TRACK_PEAK={:.6}",
                10f64.powf(first.true_peak_dbtp / 20.0)
            ),
        );
    }
    match ext.as_str() {
        "mp3" => {
            a.kv("-id3v2_version", "3");
        }
        "m4a" | "mp4" | "mov" => {
            a.kv("-movflags", "+faststart+use_metadata_tags");
        }
        _ => {}
    }
    a.a(crate::io::tool_path(&part));
    job.ffmpeg(a, p.duration, "Writing the tags", (0.5, 1.0), input)?;
    let out = job.commit(
        &part,
        &dir,
        input,
        &Naming::Labelled("replaygain".into()),
        &ext,
    )?;
    job.note(format!(
        "{}: ReplayGain tags written (track gain {:+.2} dB, measured {:.1} LUFS, true peak {:.1} dBTP). No sound was changed, so there is no quality loss; players that read ReplayGain apply the gain themselves.",
        name_of(&out),
        -18.0 - first.integrated_lufs,
        first.integrated_lufs,
        first.true_peak_dbtp
    ));
    let _ = tracks;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(lufs: f64, tp: f64) -> Loudness {
        Loudness {
            integrated_lufs: lufs,
            range_lu: 5.0,
            short_term_max_lufs: lufs + 3.0,
            momentary_max_lufs: lufs + 4.0,
            true_peak_dbtp: tp,
            sample_peak_dbfs: tp - 0.2,
            clipped_samples: 0,
            seconds: 60.0,
        }
    }

    #[test]
    fn linear_gain_is_used_when_the_peaks_allow_it() {
        let p = plan_gain(Mode::Loudness, &loud(-23.0, -9.0), -16.0, -1.0);
        assert!((p.gain_db - 7.0).abs() < 1e-9);
        assert!(
            p.limit_db.is_none(),
            "peak after gain is -2 dBTP, under the ceiling"
        );
        assert_eq!(filter_for(&p), "volume=7.000dB");
    }

    #[test]
    fn a_limiter_is_added_only_when_the_peak_would_pass_the_ceiling() {
        let p = plan_gain(Mode::Loudness, &loud(-23.0, -3.0), -16.0, -1.0);
        assert!(p.limit_db.is_some());
        let f = filter_for(&p);
        assert!(
            f.contains("alimiter") && f.contains("level=0") && f.contains("latency=1"),
            "{f}"
        );
    }

    #[test]
    fn peak_mode_sets_the_true_peak_to_the_ceiling() {
        let p = plan_gain(Mode::Peak, &loud(-20.0, -6.0), -16.0, -1.0);
        assert!((p.gain_db - 5.0).abs() < 1e-9);
        assert!(p.limit_db.is_none());
    }

    #[test]
    fn mono_files_use_the_mono_target_of_a_preset() {
        let pr = presets::loudness("podcast-general").unwrap();
        let mut s = crate::probe::parse_profile(
            std::path::Path::new("a.wav"),
            br#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"pcm_s16le","channels":1}],"format":{"format_name":"wav"}}"#,
            1,
        )
        .unwrap()
        .streams
        .remove(0);
        assert_eq!(target_for(Some(pr), -16.0, &s), -19.0);
        s.channels = 2;
        assert_eq!(target_for(Some(pr), -16.0, &s), -16.0);
        assert_eq!(target_for(None, -14.0, &s), -14.0);
    }
}
