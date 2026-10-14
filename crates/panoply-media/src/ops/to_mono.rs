//! `media.to_mono`: mono, swap, split and merge channels (spec 5.15).
//!
//! The default looks before it mixes: a channel that is silent (a microphone plugged into one
//! side), channels that are out of phase (a plain sum would cancel them) and the ordinary case
//! are three different jobs, and the report says which one it did. Swaps and splits of lossless
//! sound are exact, sample for sample.

use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::probe::{Profile, Stream};
use crate::units::{name_of, stem_of};

use super::audio::{self, AudioFormat, AudioWish};
use super::rewrite::{self, Treatment};
use super::scan;
use super::{Args, Job, with_name};

/// What was decided for one stereo track.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The filter chain.
    pub filter: String,
    /// What it did, in words.
    pub words: String,
}

/// Decide how to make a stereo pair mono from what the channels look like.
pub fn decide(b: &scan::Balance) -> Decision {
    let (l, r) = (b.db(0), b.db(1));
    let silent = |db: f64| !db.is_finite() || db < -60.0;
    if silent(l) && !silent(r) {
        return Decision {
            filter: "pan=mono|c0=c1".into(),
            words: "The left channel is silent, so the right channel was used for both sides."
                .into(),
        };
    }
    if silent(r) && !silent(l) {
        return Decision {
            filter: "pan=mono|c0=c0".into(),
            words: "The right channel is silent, so the left channel was used for both sides."
                .into(),
        };
    }
    let corr = b.correlation();
    if corr < -0.5 {
        return Decision {
            filter: "pan=mono|c0=0.5*c0-0.5*c1".into(),
            words: "The two channels are out of phase and would cancel when summed, so one was flipped before mixing.".into(),
        };
    }
    Decision {
        filter: "pan=mono|c0=0.5*c0+0.5*c1".into(),
        words: "The two channels were averaged.".into(),
    }
}

/// Run `media.to_mono`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let action = opts.text("action", "auto").to_string();
    let mut job = Job::new(io)?;
    if action == "merge" {
        if inputs.len() != 2 {
            return Err(MediaError::BadOption(
                "Merge needs exactly two files: the left channel first, then the right".into(),
            ));
        }
        merge(&mut job, &inputs[0], &inputs[1])?;
        return Ok(job.finish());
    }
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, &action).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn lossless_format(p: &Profile) -> AudioFormat {
    match p.audio().map(|a| a.codec.as_str()) {
        Some("flac") => AudioFormat::Flac,
        _ => AudioFormat::Wav,
    }
}

fn one(job: &mut Job, input: &Path, action: &str) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    let a = p
        .audio()
        .ok_or_else(|| {
            MediaError::Unsupported(
                "This file has no sound to work with. Check that the file is the one you meant."
                    .into(),
            )
        })?
        .clone();
    match action {
        "split" => return split(job, &input, &p, &a),
        "swap" => {
            if a.channels != 2 {
                return Err(MediaError::Unsupported(
                    "Swap works on stereo sound, and this has a different number of channels."
                        .into(),
                ));
            }
            return treat(
                job,
                &input,
                &p,
                &a,
                "swapped",
                Some("pan=stereo|c0=c1|c1=c0"),
                None,
                "The left and right channels were swapped.".into(),
            );
        }
        "mono_to_stereo" => {
            if a.channels != 1 {
                return Err(MediaError::Unsupported(
                    "Mono to stereo works on mono sound, and this has more than one channel."
                        .into(),
                ));
            }
            return treat(
                job,
                &input,
                &p,
                &a,
                "stereo",
                Some("pan=stereo|c0=c0|c1=c0"),
                Some(2),
                "The mono sound was copied to both sides.".into(),
            );
        }
        "left" | "right" if a.channels != 2 => {
            return Err(MediaError::Unsupported(
                "Left and right work on stereo sound, and this has a different number of channels."
                    .into(),
            ));
        }
        _ => {}
    }
    if a.channels == 1 {
        job.note(format!(
            "{} is already mono, so there was nothing to change.",
            name_of(&input)
        ));
        return Ok(());
    }
    let (filter, channels, words) = match action {
        "left" => (
            Some("pan=mono|c0=c0".to_string()),
            Some(1),
            "The left channel was kept.".to_string(),
        ),
        "right" => (
            Some("pan=mono|c0=c1".to_string()),
            Some(1),
            "The right channel was kept.".to_string(),
        ),
        "downmix" if a.channels == 2 => (
            Some("pan=mono|c0=0.5*c0+0.5*c1".to_string()),
            Some(1),
            "The two channels were averaged.".to_string(),
        ),
        _ if a.channels == 2 => {
            let s = scan::scan(
                &job.engine,
                job.io,
                &input,
                &a,
                p.duration,
                (job.lo, job.lo + (job.hi - job.lo) * 0.4),
                "Looking at the channels",
            )?;
            let bal = s.balance.unwrap_or_default();
            let d = decide(&bal);
            (Some(d.filter), Some(1), d.words)
        }
        _ => (
            None,
            Some(1),
            format!(
                "The {} channels were mixed down with the standard coefficients.",
                a.channels
            ),
        ),
    };
    treat(
        job,
        &input,
        &p,
        &a,
        "mono",
        filter.as_deref(),
        channels,
        words,
    )
}

#[allow(clippy::too_many_arguments)]
fn treat(
    job: &mut Job,
    input: &Path,
    p: &Profile,
    a: &Stream,
    label: &str,
    filter: Option<&str>,
    channels: Option<u32>,
    words: String,
) -> Result<()> {
    let target_index = a.index;
    let filter = filter.map(str::to_string);
    let rw = rewrite::rewrite_audio(job, input, p, &|s: &Stream| {
        (s.index == target_index).then(|| Treatment {
            filter: filter.clone(),
            channels,
        })
    })?;
    let out = job.commit(
        &rw.part,
        &rw.dir,
        input,
        &Naming::Labelled(label.into()),
        &rw.ext,
    )?;
    let (notes, plans) = (rw.notes, rw.plans);
    let outp = job.verify(&out, p.duration)?;
    let oa = outp.audio().cloned();
    let after_ch = oa.as_ref().map(|s| s.channels).unwrap_or(0);
    let mut line = format!(
        "{}: {} ({}).",
        name_of(&out),
        words,
        rewrite::describe(&plans)
    );
    if let (Some(want), true) = (channels, after_ch != 0) {
        line.push_str(&format!(" Channels: {} to {}.", a.channels, after_ch));
        let _ = want;
    }
    job.note(line);
    // Loudness before and after for a mix-down, so the change is a number and not a guess.
    if label == "mono"
        && let Some(oa) = oa
    {
        let (lo, hi) = (job.lo + (job.hi - job.lo) * 0.7, job.hi);
        if let (Ok(b), Ok(af)) = (
            scan::scan(
                &job.engine,
                job.io,
                input,
                a,
                p.duration,
                (job.lo, lo),
                "Measuring the loudness",
            ),
            scan::scan(
                &job.engine,
                job.io,
                &out,
                &oa,
                outp.duration,
                (lo, hi),
                "Measuring the result",
            ),
        ) {
            let (x, y) = (b.loudness.integrated_lufs, af.loudness.integrated_lufs);
            if x.is_finite() && y.is_finite() {
                job.note(format!(
                    "Loudness: {x:.1} LUFS before, {y:.1} LUFS after ({:+.1} LU).",
                    y - x
                ));
            }
        }
    }
    for n in notes {
        job.note(n);
    }
    Ok(())
}

fn split(job: &mut Job, input: &Path, p: &Profile, a: &Stream) -> Result<()> {
    if a.channels != 2 {
        return Err(MediaError::Unsupported(
            "Split works on stereo sound, and this has a different number of channels.".into(),
        ));
    }
    let fmt = lossless_format(p);
    let plan = audio::plan_audio(
        &job.engine,
        a,
        fmt,
        &AudioWish {
            force_encode: true,
            channels: Some(1),
            ..Default::default()
        },
    )?;
    let (dir, left) = job.part(input, fmt.ext())?;
    let right = job.stage.part(&dir, fmt.ext())?;
    let mut args = Args::new();
    args.a("-y");
    args.input(input);
    args.kv(
        "-filter_complex",
        format!("[0:{}]channelsplit=channel_layout=stereo[L][R]", a.index),
    );
    for (label, path) in [("L", &left), ("R", &right)] {
        args.kv("-map", format!("[{label}]"));
        args.all(&plan.args);
        args.kv("-f", fmt.muxer());
        args.a(crate::io::tool_path(path));
    }
    job.ffmpeg(
        args,
        p.duration,
        "Splitting the channels",
        (0.0, 1.0),
        input,
    )?;
    let l = job.commit(
        &left,
        &dir,
        input,
        &Naming::Labelled("left".into()),
        fmt.ext(),
    )?;
    let r = job.commit(
        &right,
        &dir,
        input,
        &Naming::Labelled("right".into()),
        fmt.ext(),
    )?;
    job.note(format!(
        "{} and {}: the two channels as separate mono files ({}).",
        name_of(&l),
        name_of(&r),
        plan.describe
    ));
    Ok(())
}

fn merge(job: &mut Job, left: &Path, right: &Path) -> Result<()> {
    let (l, r) = (job.input(left)?, job.input(right)?);
    let (pl, pr) = (job.probe(&l)?, job.probe(&r)?);
    let (al, ar) = (
        pl.audio()
            .ok_or_else(|| MediaError::Unsupported(format!("{} has no sound.", name_of(&l))))?
            .clone(),
        pr.audio()
            .ok_or_else(|| MediaError::Unsupported(format!("{} has no sound.", name_of(&r))))?
            .clone(),
    );
    if al.channels != 1 || ar.channels != 1 {
        return Err(MediaError::Unsupported(
            "Merge needs two mono files.".into(),
        ));
    }
    let fmt = lossless_format(&pl);
    let plan = audio::plan_audio(
        &job.engine,
        &al,
        fmt,
        &AudioWish {
            force_encode: true,
            channels: Some(2),
            ..Default::default()
        },
    )?;
    let (dir, part) = job.part(&l, fmt.ext())?;
    let mut a = Args::new();
    a.a("-y");
    a.input(&l);
    a.input(&r);
    a.kv(
        "-filter_complex",
        format!(
            "[0:{}][1:{}]join=inputs=2:channel_layout=stereo:map=0.0-FL|1.0-FR[a]",
            al.index, ar.index
        ),
    );
    a.kv("-map", "[a]");
    a.all(&plan.args);
    a.kv("-f", fmt.muxer());
    a.a(crate::io::tool_path(&part));
    let secs = match (pl.duration, pr.duration) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    };
    job.ffmpeg(a, secs, "Merging the channels", (0.0, 1.0), &l)?;
    let out = job.commit(
        &part,
        &dir,
        &l,
        &Naming::Labelled("stereo".into()),
        fmt.ext(),
    )?;
    job.note(format!(
        "{}: {} is the left channel and {} is the right ({}).",
        name_of(&out),
        stem_of(&l),
        stem_of(&r),
        plan.describe
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bal(l: f64, r: f64, cross: f64) -> scan::Balance {
        scan::Balance {
            energy: [l, r],
            cross,
            frames: 1,
        }
    }

    #[test]
    fn a_silent_side_is_replaced_by_the_live_one() {
        let d = decide(&bal(0.0, 0.01, 0.0));
        assert!(d.words.contains("left channel is silent"), "{}", d.words);
        assert_eq!(d.filter, "pan=mono|c0=c1");
        let d = decide(&bal(0.01, 1e-12, 0.0));
        assert!(d.words.contains("right channel is silent"), "{}", d.words);
    }

    #[test]
    fn out_of_phase_channels_are_flipped_not_cancelled() {
        let d = decide(&bal(0.01, 0.01, -0.0099));
        assert!(d.words.contains("out of phase"), "{}", d.words);
        assert!(d.filter.contains("-0.5*c1"));
    }

    #[test]
    fn ordinary_stereo_is_averaged() {
        let d = decide(&bal(0.01, 0.01, 0.002));
        assert!(d.words.contains("averaged"));
        assert_eq!(d.filter, "pan=mono|c0=0.5*c0+0.5*c1");
    }
}
